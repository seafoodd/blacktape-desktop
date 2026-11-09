//! Seekable view over an HLS media playlist made of separate files (SoundCloud's fragmented mp4).
//!
//! [`parse_hls_playlist`] turns a media playlist into the list of its parts: the optional init
//! segment (`#EXT-X-MAP`) followed by the media segments. Concatenated in order, they form one
//! valid fragmented mp4 that symphonia's isomp4 reader plays. [`SegmentedSource`] exposes that
//! concatenation as `Read + Seek` with an exact length, which is what rodio's `Decoder` needs.
//!
//! # Design
//!
//! * `open` learns the size of every part in parallel ([`SIZE_WORKERS`] threads): `HEAD` and its
//!   `Content-Length`, or, when `HEAD` is refused or has no length, `GET` with `Range: bytes=0-0`
//!   and the total of the `Content-Range`. Meanwhile the download pool already fetches the first
//!   [`EARLY_PARTS`] parts. `open` returns once all sizes are known (fails fast on the first part
//!   whose size cannot be learned, or after [`OPEN_TIMEOUT`]).
//! * A pool of [`POOL_WORKERS`] threads downloads whole parts into memory, in order of need: the
//!   part under the reader's position first, then the following ones (never the ones behind),
//!   within a [`MEMORY_CAP`] budget (parts far behind the position are evicted to make room).
//!   Bytes of a part are readable while it is still arriving (symphonia only needs the `moof`
//!   header at the start of each part while it scans the file).
//! * When the reader needs a part that nobody is fetching and every pool worker is busy (typically
//!   right after a seek far ahead), an extra one-shot "urgent" thread fetches it at once (at most
//!   [`MAX_URGENT`] at a time), so a seek never waits behind slow prefetches.
//! * A failed download is retried once; then the part is marked failed and only reads that need it
//!   return the error. A downloaded part whose size differs from the announced one also fails.
//! * Reads block on a [`Condvar`] and fail with `TimedOut` after [`STALL_TIMEOUT`] without progress
//!   on the part they wait for. `Seek` never touches the network, it only moves the priority.
//! * `Drop` sets a cancel flag: idle threads exit at once, downloading ones at their next chunk
//!   (dropping the response closes the connection).
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE, RANGE};
use reqwest::StatusCode;
use url::Url;

/// Threads learning the part sizes during `open`.
const SIZE_WORKERS: usize = 8;
/// Threads of the download pool.
const POOL_WORKERS: usize = 4;
/// Extra one-shot threads for a needed part while the pool is busy.
const MAX_URGENT: usize = 4;
/// Parts fetched before the sizes are known (init + first segment).
const EARLY_PARTS: usize = 2;
/// Overall budget of `open`.
const OPEN_TIMEOUT: Duration = Duration::from_secs(20);
/// A waiting read fails with `TimedOut` when its part made no progress for this long.
const STALL_TIMEOUT: Duration = Duration::from_secs(15);
/// Total timeout of one part download.
const PART_TIMEOUT: Duration = Duration::from_secs(120);
/// Prefetch budget; the part under the read position is always fetched.
const MEMORY_CAP: u64 = 64 * 1024 * 1024;
/// Size of one read from an HTTP body.
const CHUNK_SIZE: usize = 32 * 1024;
/// Idle pool workers re-check the cancel flag at least this often.
const IDLE_POLL: Duration = Duration::from_millis(500);

/// A parsed HLS media playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HlsPlaylist {
    /// Absolute url of the init segment (`#EXT-X-MAP:URI`).
    pub init: Option<String>,
    /// Absolute urls of the media segments, in order.
    pub segments: Vec<String>,
    /// `#EXT-X-KEY` with a method other than `NONE` (we cannot play it).
    pub encrypted: bool,
}

/// Parses a media playlist. Relative urls are resolved against `base_url`.
pub fn parse_hls_playlist(text: &str, base_url: &str) -> Result<HlsPlaylist, String> {
    let base = Url::parse(base_url).map_err(|e| format!("invalid playlist url: {e}"))?;
    let resolve = |u: &str| {
        base.join(u)
            .map(String::from)
            .map_err(|e| format!("invalid url in playlist ({u}): {e}"))
    };
    let mut init = None;
    let mut segments = Vec::new();
    let mut encrypted = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !line.starts_with('#') {
            segments.push(resolve(line)?);
            continue;
        }
        if line.starts_with("#EXT-X-STREAM-INF") {
            return Err("master playlist".into());
        }
        if let Some(attrs) = line.strip_prefix("#EXT-X-MAP:") {
            if let Some(uri) = attribute(attrs, "URI") {
                init = Some(resolve(&uri)?);
            }
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-KEY:") {
            if attribute(attrs, "METHOD").is_some_and(|m| !m.eq_ignore_ascii_case("NONE")) {
                encrypted = true;
            }
        }
    }
    if segments.is_empty() {
        return Err("playlist has no segments".into());
    }
    Ok(HlsPlaylist { init, segments, encrypted })
}

/// Value of `name` in an HLS attribute list (`A=1,B="x,y"`), without quotes.
fn attribute(list: &str, name: &str) -> Option<String> {
    let mut rest = list;
    loop {
        let (key, after) = rest.split_once('=')?;
        let (value, next) = if let Some(quoted) = after.strip_prefix('"') {
            let (v, tail) = quoted.split_once('"').unwrap_or((quoted, ""));
            (v, tail.split_once(',').map_or("", |(_, t)| t))
        } else {
            after.split_once(',').unwrap_or((after, ""))
        };
        if key.trim().eq_ignore_ascii_case(name) {
            return Some(value.trim().to_string());
        }
        if next.is_empty() {
            return None;
        }
        rest = next;
    }
}

#[derive(Debug, Clone, PartialEq)]
enum PartState {
    Missing,
    Fetching,
    Done,
    Failed(String),
}

struct Part {
    data: Vec<u8>,
    state: PartState,
}

struct State {
    parts: Vec<Part>,
    sizes: Vec<Option<u64>>,
    /// All sizes known (`open` finished): prefetch beyond [`EARLY_PARTS`] may start.
    sizes_ready: bool,
    size_error: Option<(io::ErrorKind, String)>,
    /// Part under the read position: the fetch order starts here.
    pos_part: usize,
    /// Pool workers waiting for something to do.
    idle: usize,
    /// Running urgent threads.
    urgent: usize,
    cancelled: bool,
}

impl State {
    /// Bytes held or being fetched.
    fn used(&self) -> u64 {
        self.parts
            .iter()
            .zip(&self.sizes)
            .map(|(p, s)| match p.state {
                PartState::Fetching | PartState::Done => s.unwrap_or(p.data.len() as u64),
                _ => 0,
            })
            .sum()
    }

    /// Evicts finished parts far behind the position (farthest first) until `need` more bytes fit.
    fn make_room(&mut self, need: u64) -> bool {
        let mut used = self.used();
        while used + need > MEMORY_CAP {
            let victim = (0..self.parts.len())
                .find(|&i| i + 1 < self.pos_part && self.parts[i].state == PartState::Done);
            let Some(i) = victim else { return false };
            used -= self.sizes[i].unwrap_or(self.parts[i].data.len() as u64);
            self.parts[i] = Part { data: Vec::new(), state: PartState::Missing };
        }
        true
    }

    /// Next part a pool worker should fetch.
    fn choose(&mut self) -> Option<usize> {
        let first = self.pos_part;
        let i = (first..self.parts.len()).find(|&i| self.parts[i].state == PartState::Missing)?;
        if i == first {
            self.make_room(self.sizes[i].unwrap_or(0));
            return Some(i);
        }
        if !self.sizes_ready && i >= EARLY_PARTS {
            return None;
        }
        self.make_room(self.sizes[i].unwrap_or(0)).then_some(i)
    }

    fn fail(&mut self, i: usize, msg: String) {
        self.parts[i] = Part { data: Vec::new(), state: PartState::Failed(msg) };
    }

    /// Records a size learned from HEAD / the range probe; checks it against a finished body.
    fn set_size(&mut self, i: usize, size: u64) {
        match self.sizes[i] {
            None => self.sizes[i] = Some(size),
            Some(known) if known != size && self.parts[i].state == PartState::Done => {
                self.fail(i, format!("segment {i}: downloaded {known} bytes but the server announced {size}"));
            }
            Some(_) => {}
        }
    }

    /// A download of part `i` finished: checks its size.
    fn complete(&mut self, i: usize) {
        let len = self.parts[i].data.len() as u64;
        match self.sizes[i] {
            Some(size) if size != len => {
                self.fail(i, format!("segment {i}: downloaded {len} bytes but the server announced {size}"));
            }
            Some(_) => self.parts[i].state = PartState::Done,
            None => {
                self.sizes[i] = Some(len);
                self.parts[i].state = PartState::Done;
            }
        }
    }
}

struct Shared {
    state: Mutex<State>,
    cond: Condvar,
    cancel: AtomicBool,
    /// Live background threads (for tests and diagnostics).
    threads: AtomicUsize,
    client: Client,
    urls: Vec<String>,
}

impl Shared {
    /// Locks the state, recovering from poisoning (it stays consistent between operations).
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wait<'a>(&self, guard: MutexGuard<'a, State>, dur: Duration) -> MutexGuard<'a, State> {
        match self.cond.wait_timeout(guard, dur) {
            Ok((guard, _)) => guard,
            Err(poisoned) => poisoned.into_inner().0,
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

struct ThreadGuard<'a>(&'a AtomicUsize);

impl Drop for ThreadGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn spawn(shared: &Arc<Shared>, name: &str, f: impl FnOnce(&Shared) + Send + 'static) -> io::Result<()> {
    let s = Arc::clone(shared);
    shared.threads.fetch_add(1, Ordering::SeqCst);
    let res = std::thread::Builder::new().name(name.into()).spawn(move || {
        let _guard = ThreadGuard(&s.threads);
        f(&s);
    });
    if res.is_err() {
        shared.threads.fetch_sub(1, Ordering::SeqCst);
    }
    res.map(|_| ())
}

/// `Read + Seek` over the concatenation of remote parts. See the module docs.
pub struct SegmentedSource {
    shared: Arc<Shared>,
    /// Start offset of every part, plus the total at the end.
    offsets: Vec<u64>,
    total: u64,
    pos: u64,
}

impl SegmentedSource {
    /// Parts = `init` (if any) followed by `segments`. Returns once the sizes of all parts are
    /// known; the first parts are already being downloaded by then.
    pub fn open(client: Client, init: Option<String>, segments: Vec<String>) -> io::Result<Self> {
        let urls: Vec<String> = init.into_iter().chain(segments).collect();
        let n = urls.len();
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "no segments to stream"));
        }
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                parts: (0..n).map(|_| Part { data: Vec::new(), state: PartState::Missing }).collect(),
                sizes: vec![None; n],
                sizes_ready: false,
                size_error: None,
                pos_part: 0,
                idle: 0,
                urgent: 0,
                cancelled: false,
            }),
            cond: Condvar::new(),
            cancel: AtomicBool::new(false),
            threads: AtomicUsize::new(0),
            client,
            urls,
        });
        // From here on, dropping `source` (early return) stops every thread.
        let mut source = SegmentedSource { shared: Arc::clone(&shared), offsets: Vec::new(), total: 0, pos: 0 };

        for _ in 0..POOL_WORKERS {
            spawn(&shared, "hls-fetch", pool_worker)?;
        }
        let deadline = Instant::now() + OPEN_TIMEOUT;
        let next = Arc::new(AtomicUsize::new(0));
        for _ in 0..SIZE_WORKERS.min(n) {
            let next = Arc::clone(&next);
            spawn(&shared, "hls-size", move |s| size_worker(s, &next, deadline))?;
        }

        let mut st = shared.lock();
        let sizes = loop {
            if let Some((kind, msg)) = &st.size_error {
                return Err(io::Error::new(*kind, msg.clone()));
            }
            if let Some(sizes) = st.sizes.iter().copied().collect::<Option<Vec<u64>>>() {
                break sizes;
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "timed out learning the segment sizes"));
            }
            st = shared.wait(st, deadline - now);
        };
        st.sizes_ready = true;
        drop(st);
        shared.cond.notify_all();

        let mut offsets = Vec::with_capacity(n + 1);
        let mut acc = 0u64;
        for s in sizes {
            offsets.push(acc);
            acc += s;
        }
        offsets.push(acc);
        source.offsets = offsets;
        source.total = acc;
        Ok(source)
    }

    /// Exact total size in bytes.
    pub fn content_length(&self) -> u64 {
        self.total
    }

    /// Index of the part containing `pos` (`pos < total`).
    fn part_at(&self, pos: u64) -> usize {
        self.offsets.partition_point(|&o| o <= pos).saturating_sub(1)
    }

    fn part_len(&self, i: usize) -> u64 {
        self.offsets[i + 1] - self.offsets[i]
    }

    /// Moves the fetch priority to the part under the current position.
    fn focus(&self, st: &mut State) {
        let i = if self.pos < self.total { self.part_at(self.pos) } else { st.parts.len() };
        if st.pos_part != i {
            st.pos_part = i;
            self.shared.cond.notify_all();
        }
    }
}

impl Drop for SegmentedSource {
    fn drop(&mut self) {
        self.shared.cancel.store(true, Ordering::SeqCst);
        self.shared.lock().cancelled = true;
        self.shared.cond.notify_all();
    }
}

impl Read for SegmentedSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.pos >= self.total {
            return Ok(0);
        }
        let shared = Arc::clone(&self.shared);
        let mut st = shared.lock();
        self.focus(&mut st);
        let i = self.part_at(self.pos);
        let off = self.pos - self.offsets[i];
        let mut progress: Option<usize> = None;
        let mut deadline = Instant::now() + STALL_TIMEOUT;
        loop {
            let part = &st.parts[i];
            if (part.data.len() as u64) > off {
                break;
            }
            match &part.state {
                PartState::Failed(msg) => return Err(io::Error::other(msg.clone())),
                PartState::Done => {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, format!("segment {i} is truncated")))
                }
                PartState::Missing => spawn_urgent(&shared, &mut st, i),
                PartState::Fetching => {}
            }
            let got = st.parts[i].data.len();
            let now = Instant::now();
            if progress != Some(got) {
                progress = Some(got);
                deadline = now + STALL_TIMEOUT;
            }
            if now >= deadline {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "HLS stream stalled: no data received"));
            }
            st = shared.wait(st, deadline - now);
        }

        // Copy what is available, across part boundaries, without waiting again.
        let mut n = 0;
        while n < buf.len() && self.pos < self.total {
            let i = self.part_at(self.pos);
            let off = (self.pos - self.offsets[i]) as usize;
            let data = &st.parts[i].data;
            let avail = data.len().min(self.part_len(i) as usize);
            if off >= avail {
                break;
            }
            let k = (buf.len() - n).min(avail - off);
            buf[n..n + k].copy_from_slice(&data[off..off + k]);
            n += k;
            self.pos += k as u64;
        }
        self.focus(&mut st);
        Ok(n)
    }
}

impl Seek for SegmentedSource {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target: i128 = match pos {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::Current(d) => i128::from(self.pos) + i128::from(d),
            SeekFrom::End(d) => i128::from(self.total) + i128::from(d),
        };
        let target = u64::try_from(target).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid seek to a negative or overflowing position")
        })?;
        self.pos = target;
        let shared = Arc::clone(&self.shared);
        let mut st = shared.lock();
        self.focus(&mut st);
        Ok(target)
    }
}

/// Starts a one-shot fetch of part `i` when the pool is fully busy.
fn spawn_urgent(shared: &Arc<Shared>, st: &mut State, i: usize) {
    if st.idle > 0 || st.urgent >= MAX_URGENT || st.cancelled {
        return; // an idle worker picks it up (it is the first in line), or we wait for one
    }
    st.parts[i].state = PartState::Fetching;
    st.urgent += 1;
    let res = spawn(shared, "hls-urgent", move |s| {
        run_fetch(s, i);
        s.lock().urgent -= 1;
        s.cond.notify_all();
    });
    if res.is_err() {
        st.parts[i].state = PartState::Missing;
        st.urgent -= 1;
    }
}

fn pool_worker(shared: &Shared) {
    loop {
        let i = {
            let mut st = shared.lock();
            loop {
                if st.cancelled {
                    return;
                }
                if let Some(i) = st.choose() {
                    st.parts[i].state = PartState::Fetching;
                    break i;
                }
                st.idle += 1;
                st = shared.wait(st, IDLE_POLL);
                st.idle -= 1;
            }
        };
        run_fetch(shared, i);
    }
}

enum Fetched {
    Done,
    Cancelled,
    Error(String),
}

/// Downloads part `i` (state already `Fetching`), retrying once, and records the outcome.
fn run_fetch(shared: &Shared, i: usize) {
    for attempt in 0..2 {
        let outcome = fetch_part(shared, i);
        let mut st = shared.lock();
        match outcome {
            Fetched::Cancelled => return,
            Fetched::Done => st.complete(i),
            Fetched::Error(msg) if attempt == 0 => {
                st.parts[i].data.clear();
                drop(st);
                tracing::warn!("HLS {msg}; retrying");
                continue;
            }
            Fetched::Error(msg) => st.fail(i, msg),
        }
        drop(st);
        shared.cond.notify_all();
        return;
    }
}

/// Streams part `i` into its buffer.
fn fetch_part(shared: &Shared, i: usize) -> Fetched {
    let sent = shared.client
        .get(&shared.urls[i])
        .header(ACCEPT_ENCODING, "identity")
        .timeout(PART_TIMEOUT)
        .send();
    let mut resp = match sent {
        Ok(r) => r,
        Err(e) => return Fetched::Error(format!("segment {i}: HTTP request failed: {e}")),
    };
    if shared.cancelled() {
        return Fetched::Cancelled;
    }
    let status = resp.status();
    if !status.is_success() {
        return Fetched::Error(format!("segment {i}: HTTP {status}"));
    }
    let mut buf = vec![0u8; CHUNK_SIZE];
    loop {
        let n = match resp.read(&mut buf) {
            Ok(0) => return Fetched::Done,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Fetched::Error(format!("segment {i}: download failed: {e}")),
        };
        let mut st = shared.lock();
        if st.cancelled {
            return Fetched::Cancelled; // dropping `resp` closes the connection
        }
        let size = st.sizes[i];
        let data = &mut st.parts[i].data;
        if data.is_empty() {
            if let Some(size) = size {
                data.reserve(usize::try_from(size).unwrap_or(0));
            }
        }
        data.extend_from_slice(&buf[..n]);
        drop(st);
        shared.cond.notify_all();
    }
}

fn size_worker(shared: &Shared, next: &AtomicUsize, deadline: Instant) {
    loop {
        if shared.cancelled() {
            return;
        }
        let i = next.fetch_add(1, Ordering::SeqCst);
        if i >= shared.urls.len() {
            return;
        }
        {
            let st = shared.lock();
            if st.size_error.is_some() {
                return;
            }
            if st.sizes[i].is_some() {
                continue; // the body already arrived
            }
        }
        let result = probe_size(shared, i, deadline);
        let mut st = shared.lock();
        match result {
            Ok(size) => st.set_size(i, size),
            Err(e) => {
                if st.size_error.is_none() {
                    st.size_error = Some(e);
                }
            }
        }
        drop(st);
        shared.cond.notify_all();
    }
}

fn header_u64(resp: &reqwest::blocking::Response, name: reqwest::header::HeaderName) -> Option<u64> {
    resp.headers().get(name)?.to_str().ok()?.trim().parse().ok()
}

/// Total from `bytes a-b/total`.
fn content_range_total(value: &str) -> Option<u64> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (_, total) = rest.split_once('/')?;
    total.trim().parse().ok()
}

fn timeout_left(deadline: Instant, i: usize) -> Result<Duration, (io::ErrorKind, String)> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err((io::ErrorKind::TimedOut, format!("segment {i}: timed out learning its size")));
    }
    Ok(left)
}

fn request_error(i: usize, what: &str, e: &reqwest::Error) -> (io::ErrorKind, String) {
    let kind = if e.is_timeout() { io::ErrorKind::TimedOut } else { io::ErrorKind::Other };
    (kind, format!("segment {i}: {what} request failed: {e}"))
}

/// Size of part `i`: `HEAD`, or `GET` with `Range: bytes=0-0` when HEAD gives no usable length.
fn probe_size(shared: &Shared, i: usize, deadline: Instant) -> Result<u64, (io::ErrorKind, String)> {
    let url = &shared.urls[i];
    let head = shared.client
        .head(url)
        .header(ACCEPT_ENCODING, "identity")
        .timeout(timeout_left(deadline, i)?)
        .send()
        .map_err(|e| request_error(i, "HEAD", &e))?;
    let head_status = head.status();
    if head_status.is_success() {
        // Read the header itself: the body size hint of a HEAD answer is 0.
        if let Some(len) = header_u64(&head, CONTENT_LENGTH).filter(|&l| l > 0) {
            return Ok(len);
        }
    }
    drop(head);

    let resp = shared.client
        .get(url)
        .header(RANGE, "bytes=0-0")
        .header(ACCEPT_ENCODING, "identity")
        .timeout(timeout_left(deadline, i)?)
        .send()
        .map_err(|e| request_error(i, "GET", &e))?;
    let status = resp.status();
    let size = match status {
        StatusCode::PARTIAL_CONTENT => resp
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(content_range_total),
        StatusCode::OK => header_u64(&resp, CONTENT_LENGTH),
        _ => None,
    };
    // Dropping a 200 answer before its body closes the connection: we only wanted the headers.
    drop(resp);
    size.ok_or_else(|| {
        (
            io::ErrorKind::Other,
            format!("segment {i}: cannot learn its size (HEAD: HTTP {head_status}, GET bytes=0-0: HTTP {status})"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    // ---------------------------------------------------------------- playlist parsing

    #[test]
    fn parses_realistic_playlist() {
        let text = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:10\n#EXT-X-MEDIA-SEQUENCE:0\n\
#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-MAP:URI=\"https://host/p/init.mp4?x=1&y=2\"\n\
#EXTINF:9.98,\nhttps://host/p/data000.m4s?x=1&y=2\n#EXTINF:9.98,\nhttps://host/p/data001.m4s?x=1&y=2\n\
#EXTINF:4.2,\nhttps://cdn2.host/p/data002.m4s\n#EXT-X-ENDLIST\n";
        let p = parse_hls_playlist(text, "https://playlist.host/media/playlist.m3u8?sig=abc").unwrap();
        assert_eq!(p.init.as_deref(), Some("https://host/p/init.mp4?x=1&y=2"));
        assert_eq!(
            p.segments,
            vec![
                "https://host/p/data000.m4s?x=1&y=2",
                "https://host/p/data001.m4s?x=1&y=2",
                "https://cdn2.host/p/data002.m4s",
            ]
        );
        assert!(!p.encrypted);
    }

    #[test]
    fn resolves_relative_urls() {
        let text = "#EXTM3U\r\n#EXT-X-MAP:URI=\"init.mp4\",BYTERANGE=\"700@0\"\r\n#EXTINF:10,\r\ndata000.m4s?t=1\r\n\
#EXTINF:10,\r\n../other/data001.m4s\r\n#EXTINF:10,\r\n/root/data002.m4s\r\n";
        let p = parse_hls_playlist(text, "https://host/a/b/playlist.m3u8?sig=1").unwrap();
        assert_eq!(p.init.as_deref(), Some("https://host/a/b/init.mp4"));
        assert_eq!(
            p.segments,
            vec!["https://host/a/b/data000.m4s?t=1", "https://host/a/other/data001.m4s", "https://host/root/data002.m4s"]
        );
    }

    #[test]
    fn playlist_without_map_has_no_init() {
        let p = parse_hls_playlist("#EXTM3U\n#EXTINF:10,\nhttps://h/a.ts\n", "https://h/p.m3u8").unwrap();
        assert_eq!(p.init, None);
        assert_eq!(p.segments, vec!["https://h/a.ts"]);
    }

    #[test]
    fn rejects_master_and_empty_playlists() {
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=128000,CODECS=\"mp4a.40.2\"\nlow/playlist.m3u8\n";
        assert_eq!(parse_hls_playlist(master, "https://h/m.m3u8").unwrap_err(), "master playlist");
        assert!(parse_hls_playlist("#EXTM3U\n#EXT-X-ENDLIST\n", "https://h/p.m3u8").is_err());
        assert!(parse_hls_playlist("", "https://h/p.m3u8").is_err());
        assert!(parse_hls_playlist("#EXTM3U\na.ts\n", "not a url").is_err());
    }

    #[test]
    fn detects_encryption() {
        let enc = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"https://h/key\",IV=0x1\n#EXTINF:10,\na.ts\n";
        assert!(parse_hls_playlist(enc, "https://h/p.m3u8").unwrap().encrypted);
        let sample = "#EXTM3U\n#EXT-X-KEY:URI=\"k\",METHOD=SAMPLE-AES\n#EXTINF:10,\na.ts\n";
        assert!(parse_hls_playlist(sample, "https://h/p.m3u8").unwrap().encrypted);
        let none = "#EXTM3U\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:10,\na.ts\n";
        assert!(!parse_hls_playlist(none, "https://h/p.m3u8").unwrap().encrypted);
    }

    #[test]
    fn attribute_parsing() {
        let list = "METHOD=AES-128,URI=\"https://h/k?a=1,b=2\",IV=0x10";
        assert_eq!(attribute(list, "URI").as_deref(), Some("https://h/k?a=1,b=2"));
        assert_eq!(attribute(list, "IV").as_deref(), Some("0x10"));
        assert_eq!(attribute(list, "method").as_deref(), Some("AES-128"));
        assert_eq!(attribute(list, "KEYFORMAT"), None);
        assert_eq!(content_range_total("bytes 0-0/12345"), Some(12345));
        assert_eq!(content_range_total("bytes 0-0/*"), None);
    }

    // ---------------------------------------------------------------- test server

    #[derive(Clone, Default)]
    struct Cfg {
        head_delay: Duration,
        /// HEAD answers 405.
        head_not_allowed: bool,
        /// Parts whose body is sent slowly: (chunk size, delay between chunks).
        slow: Vec<usize>,
        slow_chunk: usize,
        slow_delay: Duration,
        /// (part, n): the first n plain GETs of the part answer 500.
        fail_gets: Vec<(usize, usize)>,
        /// (part, extra): HEAD announces `len + extra`.
        head_extra: Vec<(usize, u64)>,
    }

    #[derive(Clone, Debug)]
    struct Req {
        method: String,
        part: usize,
        range: Option<String>,
    }

    struct Server {
        urls: Vec<String>,
        parts: Vec<Arc<Vec<u8>>>,
        log: Arc<Mutex<Vec<Req>>>,
        aborted: Arc<AtomicUsize>,
        /// Parts whose full body was sent.
        completed: Arc<Mutex<Vec<usize>>>,
        active: Arc<AtomicUsize>,
    }

    impl Server {
        fn concat(&self) -> Vec<u8> {
            self.parts.iter().flat_map(|p| p.iter().copied()).collect()
        }

        fn offset(&self, part: usize) -> u64 {
            self.parts[..part].iter().map(|p| p.len() as u64).sum()
        }

        fn requests(&self) -> Vec<Req> {
            self.log.lock().unwrap().clone()
        }

        fn open(&self) -> io::Result<SegmentedSource> {
            SegmentedSource::open(client(), Some(self.urls[0].clone()), self.urls[1..].to_vec())
        }
    }

    fn payload(len: usize, seed: u64) -> Vec<u8> {
        let mut x: u64 = 0x1234_5678_9abc_def1 ^ seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        (0..len)
            .map(|_| {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (x >> 33) as u8
            })
            .collect()
    }

    fn client() -> Client {
        Client::builder().no_proxy().build().unwrap()
    }

    struct ActiveGuard(Arc<AtomicUsize>);

    impl Drop for ActiveGuard {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn serve(sizes: &[usize], cfg: Cfg) -> Server {
        let parts: Vec<Arc<Vec<u8>>> =
            sizes.iter().enumerate().map(|(i, &s)| Arc::new(payload(s, i as u64))).collect();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let log = Arc::new(Mutex::new(Vec::new()));
        let aborted = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(Mutex::new(Vec::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let gets = Arc::new(Mutex::new(vec![0usize; sizes.len()]));
        let ctx = (parts.clone(), log.clone(), aborted.clone(), completed.clone(), active.clone());
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let (parts, log, aborted, completed, active) = ctx.clone();
                let (cfg, gets) = (cfg.clone(), gets.clone());
                active.fetch_add(1, Ordering::SeqCst);
                thread::spawn(move || {
                    let _g = ActiveGuard(active);
                    handle(stream, &parts, &cfg, &log, &aborted, &completed, &gets);
                });
            }
        });
        let urls = (0..sizes.len()).map(|i| format!("http://{addr}/seg/{i}.m4s?tok=1")).collect();
        Server { urls, parts, log, aborted, completed, active }
    }

    fn handle(
        mut stream: TcpStream,
        parts: &[Arc<Vec<u8>>],
        cfg: &Cfg,
        log: &Mutex<Vec<Req>>,
        aborted: &AtomicUsize,
        completed: &Mutex<Vec<usize>>,
        gets: &Mutex<Vec<usize>>,
    ) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let mut words = line.split_whitespace();
        let method = words.next().unwrap_or("").to_string();
        let path = words.next().unwrap_or("");
        let part: usize = path
            .strip_prefix("/seg/")
            .and_then(|p| p.split('.').next())
            .and_then(|p| p.parse().ok())
            .unwrap_or(usize::MAX);
        let mut range = None;
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let l = line.trim_end();
            if l.is_empty() {
                break;
            }
            if let Some((k, v)) = l.split_once(':') {
                if k.trim().eq_ignore_ascii_case("range") {
                    range = Some(v.trim().to_string());
                }
            }
        }
        log.lock().unwrap().push(Req { method: method.clone(), part, range: range.clone() });
        let Some(data) = parts.get(part) else {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            return;
        };
        let len = data.len();
        if method == "HEAD" {
            if cfg.head_not_allowed {
                let _ = stream
                    .write_all(b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                return;
            }
            thread::sleep(cfg.head_delay);
            let extra = cfg.head_extra.iter().find(|(p, _)| *p == part).map_or(0, |(_, e)| *e);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                len as u64 + extra
            );
            let _ = stream.write_all(head.as_bytes());
            return;
        }
        if range.as_deref() == Some("bytes=0-0") {
            let head = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-0/{len}\r\nContent-Length: 1\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(&data[..1]));
            return;
        }
        let nth = {
            let mut g = gets.lock().unwrap();
            g[part] += 1;
            g[part]
        };
        if cfg.fail_gets.iter().any(|&(p, n)| p == part && nth <= n) {
            let _ = stream.write_all(
                b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        }
        let head = format!("HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n");
        if stream.write_all(head.as_bytes()).is_err() {
            aborted.fetch_add(1, Ordering::SeqCst);
            return;
        }
        let slow = cfg.slow.contains(&part);
        let chunk = if slow { cfg.slow_chunk.max(1) } else { 64 * 1024 };
        for piece in data.chunks(chunk) {
            if stream.write_all(piece).and_then(|_| stream.flush()).is_err() {
                aborted.fetch_add(1, Ordering::SeqCst);
                return;
            }
            if slow {
                thread::sleep(cfg.slow_delay);
            }
        }
        completed.lock().unwrap().push(part);
    }

    fn slow_cfg(slow: Vec<usize>) -> Cfg {
        Cfg { slow, slow_chunk: 4 * 1024, slow_delay: Duration::from_millis(100), ..Cfg::default() }
    }

    fn read_at(src: &mut SegmentedSource, pos: u64, n: usize) -> io::Result<Vec<u8>> {
        src.seek(SeekFrom::Start(pos))?;
        let mut out = vec![0u8; n];
        src.read_exact(&mut out)?;
        Ok(out)
    }

    const SIZES: [usize; 6] = [700, 10_001, 65_536, 1, 300_000, 77_777];

    // ---------------------------------------------------------------- source tests

    #[test]
    fn is_send_sync() {
        fn assert_send_sync<T: Send + Sync + 'static>() {}
        assert_send_sync::<SegmentedSource>();
    }

    #[test]
    fn sequential_read_matches_concatenation() {
        let server = serve(&SIZES, Cfg::default());
        let mut src = server.open().unwrap();
        let all = server.concat();
        assert_eq!(src.content_length(), all.len() as u64);
        let mut out = Vec::new();
        src.read_to_end(&mut out).unwrap();
        assert!(out == all);
        // Every part body was fetched exactly once.
        let gets = server.requests().iter().filter(|r| r.method == "GET" && r.range.is_none()).count();
        assert_eq!(gets, SIZES.len());
    }

    #[test]
    fn open_without_init() {
        let server = serve(&[5000, 6000], Cfg::default());
        let mut src = SegmentedSource::open(client(), None, server.urls.clone()).unwrap();
        let mut out = Vec::new();
        src.read_to_end(&mut out).unwrap();
        assert!(out == server.concat());
        assert!(SegmentedSource::open(client(), None, Vec::new()).is_err());
    }

    #[test]
    fn sizes_are_learned_in_parallel() {
        let sizes = [700, 1000, 2000, 3000, 4000, 5000];
        let server = serve(&sizes, Cfg { head_delay: Duration::from_millis(200), ..Cfg::default() });
        let c = client();
        let t = Instant::now();
        let mut src = SegmentedSource::open(c, Some(server.urls[0].clone()), server.urls[1..].to_vec()).unwrap();
        let took = t.elapsed();
        assert!(took < Duration::from_millis(700), "open took {took:?} (sequential would be ~1.2 s)");
        let mut out = Vec::new();
        src.read_to_end(&mut out).unwrap();
        assert!(out == server.concat());
    }

    #[test]
    fn head_not_allowed_falls_back_to_range_probe() {
        let server = serve(&SIZES, Cfg { head_not_allowed: true, ..Cfg::default() });
        let mut src = server.open().unwrap();
        assert_eq!(src.content_length(), server.concat().len() as u64);
        let probes = server
            .requests()
            .iter()
            .filter(|r| r.method == "GET" && r.range.as_deref() == Some("bytes=0-0"))
            .count();
        assert!(probes >= SIZES.len() - EARLY_PARTS, "range probes: {probes}");
        let mut out = Vec::new();
        src.read_to_end(&mut out).unwrap();
        assert!(out == server.concat());
    }

    #[test]
    fn unknown_size_fails_open_with_status() {
        let server = serve(&SIZES, Cfg { head_not_allowed: true, ..Cfg::default() });
        let mut urls = server.urls.clone();
        let missing = urls[3].replace("/seg/3.", "/seg/99.");
        urls[3] = missing;
        let t = Instant::now();
        let err = SegmentedSource::open(client(), None, urls).err().expect("open must fail");
        assert!(t.elapsed() < Duration::from_secs(5));
        let msg = err.to_string();
        assert!(msg.contains("segment 3") && msg.contains("HEAD: HTTP 404") && msg.contains("bytes=0-0: HTTP 404"), "{msg}");
    }

    #[test]
    fn random_seeks_across_parts_match() {
        let server = serve(&SIZES, Cfg::default());
        let all = server.concat();
        let len = all.len() as u64;
        let mut src = server.open().unwrap();
        let mut x: u64 = 7;
        let mut next = || {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            x >> 17
        };
        for i in 0..150 {
            let pos = next() % len;
            let n = (1 + next() % 120_000).min(len - pos) as usize;
            if i % 3 == 0 {
                let cur = src.stream_position().unwrap() as i64;
                src.seek(SeekFrom::Current(pos as i64 - cur)).unwrap();
            } else {
                src.seek(SeekFrom::Start(pos)).unwrap();
            }
            let mut out = vec![0u8; n];
            src.read_exact(&mut out).unwrap();
            assert_eq!(out, all[pos as usize..pos as usize + n], "pos {pos} n {n}");
        }
        // Exactly on part boundaries.
        for p in 1..SIZES.len() {
            let off = server.offset(p);
            assert_eq!(read_at(&mut src, off - 1, 2).unwrap(), all[off as usize - 1..off as usize + 1]);
        }
    }

    #[test]
    fn seek_to_end_and_back() {
        let server = serve(&SIZES, Cfg::default());
        let all = server.concat();
        let len = all.len();
        let mut src = server.open().unwrap();
        assert_eq!(src.seek(SeekFrom::End(0)).unwrap(), len as u64);
        let mut b = [0u8; 16];
        assert_eq!(src.read(&mut b).unwrap(), 0);
        src.seek(SeekFrom::End(-10)).unwrap();
        let mut tail = Vec::new();
        src.read_to_end(&mut tail).unwrap();
        assert_eq!(tail, all[len - 10..]);
        assert_eq!(read_at(&mut src, 0, 1000).unwrap(), all[..1000]);
        src.seek(SeekFrom::Start(len as u64 + 50)).unwrap();
        assert_eq!(src.read(&mut b).unwrap(), 0);
        assert!(src.seek(SeekFrom::Current(-(len as i64) - 100)).is_err());
        src.seek(SeekFrom::Start(0)).unwrap();
        let mut out = Vec::new();
        src.read_to_end(&mut out).unwrap();
        assert!(out == all);
    }

    #[test]
    fn far_seek_fetches_new_part_with_priority() {
        // Parts 1..=6 take ~5 s each; part 9 is fast.
        let sizes = [700, 200_000, 200_000, 200_000, 200_000, 200_000, 200_000, 50_000, 50_000, 50_000];
        let server = serve(&sizes, slow_cfg((1..=6).collect()));
        let all = server.concat();
        let mut src = server.open().unwrap();
        thread::sleep(Duration::from_millis(100)); // let the pool get busy with the slow parts
        let off = server.offset(9) + 10;
        let t = Instant::now();
        let got = read_at(&mut src, off, 1000).unwrap();
        assert!(t.elapsed() < Duration::from_secs(1), "far read took {:?}", t.elapsed());
        assert_eq!(got, all[off as usize..off as usize + 1000]);
        // The tail also arrives quickly from there.
        let mut rest = Vec::new();
        src.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, all[off as usize + 1000..]);
        assert!(t.elapsed() < Duration::from_secs(2), "tail took {:?}", t.elapsed());
    }

    #[test]
    fn failing_part_only_breaks_reads_inside_it() {
        let server = serve(&SIZES, Cfg { fail_gets: vec![(3, 2), (4, 1)], ..Cfg::default() });
        let all = server.concat();
        let mut src = server.open().unwrap();
        let o2 = server.offset(2) as usize;
        assert_eq!(read_at(&mut src, o2 as u64, 100).unwrap(), all[o2..o2 + 100]);
        let err = read_at(&mut src, server.offset(3), 1).unwrap_err();
        assert!(err.to_string().contains("500"), "{err}");
        // Part 4 failed once only: the retry succeeded.
        let o4 = server.offset(4) as usize;
        assert_eq!(read_at(&mut src, o4 as u64, 300_000).unwrap(), all[o4..o4 + 300_000]);
        // Reading part 2 up to the boundary works, then the failed part errors again.
        src.seek(SeekFrom::Start(o2 as u64)).unwrap();
        let mut buf = vec![0u8; SIZES[2]];
        src.read_exact(&mut buf).unwrap();
        assert_eq!(buf, all[o2..o2 + SIZES[2]]);
        assert!(src.read(&mut [0u8; 4]).is_err());
        let gets3 = server.requests().iter().filter(|r| r.part == 3 && r.method == "GET" && r.range.is_none()).count();
        assert_eq!(gets3, 2, "one retry");
    }

    #[test]
    fn size_mismatch_fails_the_part() {
        let server = serve(&SIZES, Cfg { head_extra: vec![(4, 5)], ..Cfg::default() });
        let all = server.concat();
        let mut src = server.open().unwrap();
        assert_eq!(src.content_length(), all.len() as u64 + 5);
        let end4 = server.offset(5) + 5;
        let err = read_at(&mut src, end4 - 2, 1).unwrap_err();
        assert!(err.to_string().contains("announced"), "{err}");
        assert_eq!(read_at(&mut src, 0, 700).unwrap(), all[..700]);
    }

    #[test]
    fn first_reads_return_before_slow_parts_finish() {
        let sizes = [700, 20_000, 200_000, 200_000, 200_000, 200_000];
        let server = serve(&sizes, slow_cfg(vec![2, 3, 4, 5]));
        let all = server.concat();
        let t = Instant::now();
        let mut src = server.open().unwrap();
        let mut head = vec![0u8; 20_700];
        src.read_exact(&mut head).unwrap();
        assert!(t.elapsed() < Duration::from_millis(1500), "took {:?}", t.elapsed());
        assert_eq!(head, all[..20_700]);
        let done = server.completed.lock().unwrap().clone();
        assert!(done.iter().all(|&p| p < 2), "slow parts already completed: {done:?}");
    }

    #[test]
    fn drop_stops_threads_and_connections() {
        let sizes = [700, 200_000, 200_000, 200_000, 200_000, 200_000, 200_000];
        let server = serve(&sizes, slow_cfg((1..=6).collect()));
        let mut src = server.open().unwrap();
        let mut b = [0u8; 100];
        src.read_exact(&mut b).unwrap();
        let shared = Arc::clone(&src.shared);
        thread::sleep(Duration::from_millis(200));
        assert!(server.active.load(Ordering::SeqCst) > 0, "downloads should be running");
        drop(src);
        let t = Instant::now();
        while shared.threads.load(Ordering::SeqCst) > 0 || server.active.load(Ordering::SeqCst) > 0 {
            assert!(
                t.elapsed() < Duration::from_secs(3),
                "threads {} connections {}",
                shared.threads.load(Ordering::SeqCst),
                server.active.load(Ordering::SeqCst)
            );
            thread::sleep(Duration::from_millis(20));
        }
        assert!(server.aborted.load(Ordering::SeqCst) > 0);
        assert!(server.completed.lock().unwrap().iter().all(|&p| p == 0));
    }
}
