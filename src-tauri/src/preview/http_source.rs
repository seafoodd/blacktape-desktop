//! Seekable, prefetching HTTP media source for rodio/symphonia.
//!
//! [`HttpSource`] exposes a remote resource (progressive mp3, m4a, webm...) as `Read + Seek`
//! so that playback can start as soon as the first bytes arrive.
//!
//! # Design
//!
//! All network I/O happens on background threads ("downloaders"); the reader only copies bytes
//! out of in-memory buffers and blocks on a [`Condvar`] when the bytes it needs are not there yet.
//!
//! The shared state holds up to two *windows*. A window is a contiguous byte range
//! `[start, start + data.len())` filled by one open-ended request `Range: bytes=start-`:
//!
//! * `main`: the sequential download. [`HttpSource::open`] starts it at offset 0 and returns as
//!   soon as its first chunk is in (or fails fast on non-2xx status / empty body / errors).
//! * `aux`: a side window used for reads far outside `main` (typically symphonia probing the
//!   tail of the file for ID3v1/APE tags or an mp4 `moov` atom, or a user seek far ahead).
//!
//! Reading at position `pos`:
//!
//! 1. `pos` inside `main` or `aux` data: copy, no network. Reading from `main` stops an active
//!    `aux` download that is no longer useful (its data is kept as a cache).
//! 2. `pos` not buffered but at most [`WAIT_AHEAD`] beyond the head of an active window: wait for
//!    that window's download.
//! 3. Otherwise (far forward, or backward before `main.start`) and the server supports ranges:
//!    start a new `aux` window at `pos` (replacing the previous one) and wait for its first bytes.
//!    The main download is NOT cancelled, so a probe of the tail followed by a seek back to 0 is
//!    cheap: one small extra request, and the sequential download keeps running.
//! 4. Promotion: once more than [`PROMOTE_AFTER`] bytes have been read sequentially from an
//!    active `aux` window (and it does not start within [`TAIL_ZONE`] of the end, which is where
//!    tag/index probes live), the reader is evidently playing from there (user seek). `aux` and
//!    `main` are swapped: the old main download is stopped (its data kept in the `aux` slot as a
//!    cache for seeking back), and the side window continues as the sequential download.
//!
//! Without range support (neither a `206` answer nor `Accept-Ranges: bytes`), everything is kept
//! from offset 0, backward seeks are served from memory and forward seeks simply wait for the
//! single download.
//!
//! EOF is reported only at the true end of the resource (`Content-Range` total, `Content-Length`
//! of a `200`, or the end of an open-ended download). A download that ends before the announced
//! length, or fails, records its error; the reader returns it once the buffered bytes before it
//! are exhausted. Reads waiting for data fail with `TimedOut` after [`STALL_TIMEOUT`] without
//! progress.
//!
//! Memory: nothing is evicted, but each window stops downloading at [`MEMORY_CAP`]. Reading past a
//! capped window restarts a range request there (or fails without range support).
//!
//! Chunked mode ([`HttpSource::open_chunked`]): some servers (YouTube's googlevideo) reject
//! open-ended ranges (`bytes=N-`, 403) but serve bounded ones. In this mode each window's
//! downloader issues consecutive bounded requests back to back on its own thread: first
//! `bytes=start-(start+128KiB-1)` for a fast start, then 1 MiB ranges until the total length (from
//! the first `Content-Range`). Each response is still streamed incrementally into the window, so
//! the window logic above is unchanged. [`HttpSource::open`] switches to chunked mode by itself
//! when the first open-ended request answers 403 or 416.
//!
//! Cancellation: a downloader checks, after every chunk, that its window still exists and is
//! active; dropping the [`HttpSource`] (or replacing/stopping a window) makes it exit and drop the
//! response, which closes the connection. A downloader blocked on a completely stalled
//! connection only notices at its next chunk or when the request timeout expires; it never blocks
//! the reader.
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use reqwest::header::{ACCEPT_ENCODING, ACCEPT_RANGES, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use reqwest::StatusCode;

/// Size of one read from the HTTP body.
const CHUNK_SIZE: usize = 32 * 1024;
/// A waiting read fails with `TimedOut` when no byte arrived for this long.
const STALL_TIMEOUT: Duration = Duration::from_secs(15);
/// Maximum time `open` waits for the headers and the first bytes.
const OPEN_TIMEOUT: Duration = Duration::from_secs(20);
/// Total timeout of one request (headers + whole body); generous for long tracks on slow links.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Positions at most this far beyond a running download's head are waited for, not re-requested.
const WAIT_AHEAD: u64 = 256 * 1024;
/// Sequential bytes read from a side window after which it becomes the main download.
const PROMOTE_AFTER: u64 = 256 * 1024;
/// Side windows starting this close to the end of the resource are never promoted.
const TAIL_ZONE: u64 = 1024 * 1024;
/// Safety cap per window; the download stops beyond it.
const MEMORY_CAP: usize = 128 * 1024 * 1024;
/// Chunked mode: size of the first bounded range of every window (fast start / fast seek).
const FIRST_RANGE_CHUNK: u64 = 128 * 1024;
/// Chunked mode: size of the following bounded ranges.
const RANGE_CHUNK: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
enum Status {
    /// Request sent, no body bytes yet.
    Pending,
    /// Body bytes are arriving.
    Running,
    /// Reached the end of the resource.
    Done,
    /// Cancelled or capped; the data is kept but nothing more will arrive.
    Stopped,
    /// The download failed after the buffered data.
    Failed(io::ErrorKind, String),
}

impl Status {
    fn active(&self) -> bool {
        matches!(self, Status::Pending | Status::Running)
    }
}

struct Window {
    id: u64,
    start: u64,
    data: Vec<u8>,
    status: Status,
}

impl Window {
    fn new(id: u64, start: u64) -> Self {
        Self { id, start, data: Vec::new(), status: Status::Pending }
    }

    fn end(&self) -> u64 {
        self.start + self.data.len() as u64
    }

    fn contains(&self, pos: u64) -> bool {
        pos >= self.start && pos < self.end()
    }

    /// `pos` is at or shortly after this window's head (anywhere after it without ranges).
    fn in_reach(&self, pos: u64, ranges: bool) -> bool {
        pos >= self.start && (!ranges || pos <= self.end().saturating_add(WAIT_AHEAD))
    }

    fn reaches(&self, pos: u64, ranges: bool) -> bool {
        self.status.active() && self.in_reach(pos, ranges)
    }

    /// Copies from `pos` (which must be contained) into `buf`.
    fn copy_to(&self, pos: u64, buf: &mut [u8]) -> usize {
        let off = (pos - self.start) as usize;
        let n = buf.len().min(self.data.len() - off);
        buf[..n].copy_from_slice(&self.data[off..off + n]);
        n
    }

    fn error(&self) -> Option<io::Error> {
        match &self.status {
            Status::Failed(kind, msg) => Some(io::Error::new(*kind, msg.clone())),
            _ => None,
        }
    }
}

struct State {
    main: Window,
    aux: Option<Window>,
    next_id: u64,
    /// Total length of the resource when known.
    total: Option<u64>,
    /// Whether the server honours `Range` requests.
    ranges: bool,
    /// Downloads use consecutive bounded ranges instead of one open-ended request.
    chunked: bool,
    content_type: Option<String>,
}

impl State {
    fn window_mut(&mut self, id: u64) -> Option<&mut Window> {
        if self.main.id == id {
            return Some(&mut self.main);
        }
        self.aux.as_mut().filter(|w| w.id == id)
    }

    fn is_active(&mut self, id: u64) -> bool {
        self.window_mut(id).is_some_and(|w| w.status.active())
    }
}

struct Shared {
    state: Mutex<State>,
    cond: Condvar,
}

impl Shared {
    /// Locks the state, recovering from poisoning (the state stays consistent between chunks).
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wait<'a>(&self, guard: MutexGuard<'a, State>, dur: Duration) -> MutexGuard<'a, State> {
        match self.cond.wait_timeout(guard, dur) {
            Ok((guard, _)) => guard,
            Err(poisoned) => poisoned.into_inner().0,
        }
    }
}

/// `Read + Seek` over an HTTP(S) resource, streaming in the background. See the module docs.
pub struct HttpSource {
    shared: Arc<Shared>,
    client: Client,
    url: Arc<str>,
    pos: u64,
    len: Option<u64>,
    content_type: Option<String>,
}

impl HttpSource {
    /// Starts streaming `url` and returns as soon as the response headers and the first bytes are
    /// available. Fails on non-2xx status, HTTP errors, an empty body or a timeout.
    ///
    /// If the first open-ended range request is answered with 403 or 416, the source retries
    /// once in chunked mode (see [`HttpSource::open_chunked`]).
    pub fn open(client: Client, url: &str) -> io::Result<Self> {
        Self::open_with(client, url, false)
    }

    /// Like [`HttpSource::open`], but every request is a bounded range (128 KiB first, then
    /// 1 MiB chunks), for servers that reject open-ended ranges.
    pub fn open_chunked(client: Client, url: &str) -> io::Result<Self> {
        Self::open_with(client, url, true)
    }

    fn open_with(client: Client, url: &str, chunked: bool) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                main: Window::new(1, 0),
                aux: None,
                next_id: 2,
                total: None,
                ranges: chunked,
                chunked,
                content_type: None,
            }),
            cond: Condvar::new(),
        });
        let url: Arc<str> = Arc::from(url);
        spawn_download(&shared, &client, &url, 1, 0, true)?;
        // From here on, dropping `source` (early return) cancels the download.
        let mut source = HttpSource {
            shared: Arc::clone(&shared),
            client,
            url,
            pos: 0,
            len: None,
            content_type: None,
        };

        let deadline = Instant::now() + OPEN_TIMEOUT;
        let mut st = shared.lock();
        loop {
            if !st.main.data.is_empty() {
                break;
            }
            match &st.main.status {
                Status::Done => {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "empty response body"))
                }
                Status::Failed(kind, msg) => return Err(io::Error::new(*kind, msg.clone())),
                Status::Stopped => return Err(io::Error::other("download stopped")),
                Status::Pending | Status::Running => {}
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "timed out waiting for the first bytes of the stream",
                ));
            }
            st = shared.wait(st, deadline - now);
        }
        source.len = st.total;
        source.content_type = st.content_type.clone();
        drop(st);
        Ok(source)
    }

    /// Total size in bytes when the server told us.
    pub fn content_length(&self) -> Option<u64> {
        self.len
    }

    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    /// Starts a new side window at `pos` (replacing the previous one) and returns its id.
    fn start_aux(&self, st: &mut State, pos: u64) -> io::Result<u64> {
        let id = st.next_id;
        st.next_id += 1;
        // Replacing the old side window makes its downloader exit at its next chunk.
        st.aux = Some(Window::new(id, pos));
        if let Err(e) = spawn_download(&self.shared, &self.client, &self.url, id, pos, false) {
            st.aux = None;
            return Err(e);
        }
        Ok(id)
    }
}

impl Drop for HttpSource {
    fn drop(&mut self) {
        let mut st = self.shared.lock();
        if st.main.status.active() {
            st.main.status = Status::Stopped;
        }
        if let Some(aux) = st.aux.as_mut() {
            if aux.status.active() {
                aux.status = Status::Stopped;
            }
        }
        drop(st);
        self.shared.cond.notify_all();
    }
}

impl Read for HttpSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let shared = Arc::clone(&self.shared);
        let mut st = shared.lock();
        let mut progress: Option<(u64, usize)> = None;
        let mut deadline = Instant::now() + STALL_TIMEOUT;
        loop {
            let pos = self.pos;
            if st.total.is_some_and(|total| pos >= total) {
                return Ok(0);
            }
            let ranges = st.ranges;

            if st.main.contains(pos) {
                let n = st.main.copy_to(pos, buf);
                if let Some(aux) = st.aux.as_mut() {
                    if aux.status.active() && !aux.in_reach(pos, ranges) {
                        aux.status = Status::Stopped;
                    }
                }
                self.pos += n as u64;
                return Ok(n);
            }

            if let Some(aux) = st.aux.as_ref().filter(|a| a.contains(pos)) {
                let n = aux.copy_to(pos, buf);
                let consumed = pos + n as u64 - aux.start;
                let near_tail = st.total.is_some_and(|t| t.saturating_sub(aux.start) <= TAIL_ZONE);
                let promote = aux.status.active() && consumed > PROMOTE_AFTER && !near_tail;
                if promote {
                    if let Some(mut window) = st.aux.take() {
                        std::mem::swap(&mut st.main, &mut window);
                        if window.status.active() {
                            window.status = Status::Stopped;
                        }
                        st.aux = Some(window);
                    }
                }
                self.pos += n as u64;
                return Ok(n);
            }

            // Not buffered: find a download to wait for, or start one.
            let waiting = if st.main.reaches(pos, ranges) {
                st.main.id
            } else if let Some(aux) = st.aux.as_ref().filter(|a| a.reaches(pos, ranges)) {
                aux.id
            } else {
                if st.main.in_reach(pos, ranges) {
                    if let Some(e) = st.main.error() {
                        return Err(e);
                    }
                }
                if let Some(e) = st.aux.as_ref().filter(|a| a.in_reach(pos, ranges)).and_then(Window::error) {
                    return Err(e);
                }
                if !ranges {
                    if pos < st.main.start {
                        return Err(io::Error::new(
                            io::ErrorKind::Unsupported,
                            "server does not support range requests",
                        ));
                    }
                    return match st.main.status {
                        Status::Done => Ok(0),
                        _ => Err(io::Error::other(
                            "stream exceeds the memory cap and the server does not support range requests",
                        )),
                    };
                }
                self.start_aux(&mut st, pos)?
            };

            let len_now = st.window_mut(waiting).map_or(0, |w| w.data.len());
            let now = Instant::now();
            if progress != Some((waiting, len_now)) {
                progress = Some((waiting, len_now));
                deadline = now + STALL_TIMEOUT;
            }
            if now >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "HTTP stream stalled: no data received",
                ));
            }
            st = shared.wait(st, deadline - now);
        }
    }
}

impl Seek for HttpSource {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target: i128 = match pos {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::Current(d) => i128::from(self.pos) + i128::from(d),
            SeekFrom::End(d) => {
                let total = self.shared.lock().total.ok_or_else(|| {
                    io::Error::new(io::ErrorKind::Unsupported, "stream length is unknown")
                })?;
                i128::from(total) + i128::from(d)
            }
        };
        let target = u64::try_from(target).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid seek to a negative or overflowing position")
        })?;
        let st = self.shared.lock();
        if !st.ranges && target < st.main.start {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "cannot seek back: data was dropped and the server does not support range requests",
            ));
        }
        drop(st);
        self.pos = target;
        Ok(target)
    }
}

fn http_error(e: reqwest::Error) -> io::Error {
    let kind = if e.is_timeout() { io::ErrorKind::TimedOut } else { io::ErrorKind::Other };
    io::Error::new(kind, format!("HTTP request failed: {e}"))
}

/// Parses `bytes a-b/total` (total may be `*`).
fn parse_content_range(value: &str) -> Option<(u64, u64, Option<u64>)> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (range, total) = rest.split_once('/')?;
    let total = match total.trim() {
        "*" => None,
        t => Some(t.parse().ok()?),
    };
    let (a, b) = range.trim().split_once('-')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?, total))
}

/// Parses the `bytes */total` of a 416 answer.
fn parse_unsatisfied_range(value: &str) -> Option<u64> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (range, total) = rest.split_once('/')?;
    if range.trim() != "*" {
        return None;
    }
    total.trim().parse().ok()
}

fn spawn_download(
    shared: &Arc<Shared>,
    client: &Client,
    url: &Arc<str>,
    id: u64,
    start: u64,
    first: bool,
) -> io::Result<()> {
    let shared = Arc::clone(shared);
    let client = client.clone();
    let url = Arc::clone(url);
    std::thread::Builder::new()
        .name("http-source".into())
        .spawn(move || {
            let result = download(&shared, &client, &url, id, start, first);
            let mut st = shared.lock();
            let total = st.total;
            let mut eof_at = None;
            if let Some(w) = st.window_mut(id) {
                if w.status.active() {
                    let end = w.end();
                    w.status = match result {
                        Ok(()) => match total {
                            Some(t) if end < t => Status::Failed(
                                io::ErrorKind::UnexpectedEof,
                                format!("connection closed after {end} of {t} bytes"),
                            ),
                            _ => {
                                eof_at = Some(end);
                                Status::Done
                            }
                        },
                        Err(e) => Status::Failed(e.kind(), e.to_string()),
                    };
                }
            }
            if st.total.is_none() {
                st.total = eof_at;
            }
            drop(st);
            shared.cond.notify_all();
        })?;
    Ok(())
}

/// Downloads `url` from `start` into window `id` until EOF, cancellation, the cap, or an error.
/// `first` marks the very first request of the source (made by `open`).
fn download(shared: &Shared, client: &Client, url: &str, id: u64, start: u64, first: bool) -> io::Result<()> {
    let chunked = shared.lock().chunked;
    if !chunked {
        match fetch(shared, client, url, id, start, None, first)? {
            Fetch::Rejected(_) => {
                // Open-ended range refused (e.g. googlevideo): retry once with bounded ranges.
                let mut st = shared.lock();
                st.chunked = true;
            }
            Fetch::End | Fetch::Stop | Fetch::Body { .. } => return Ok(()),
        }
    }

    let mut pos = start;
    let mut size = FIRST_RANGE_CHUNK;
    let mut first = first;
    loop {
        let total = shared.lock().total;
        if total.is_some_and(|t| pos >= t) {
            return Ok(());
        }
        let mut end = pos + size - 1;
        if let Some(t) = total {
            end = end.min(t - 1);
        }
        match fetch(shared, client, url, id, pos, Some(end), first)? {
            Fetch::Rejected(status) => return Err(io::Error::other(format!("HTTP {status}"))),
            Fetch::End | Fetch::Stop => return Ok(()),
            Fetch::Body { got, whole } => {
                if whole || got == 0 {
                    return Ok(());
                }
                pos += got;
            }
        }
        first = false;
        size = RANGE_CHUNK;
    }
}

/// Outcome of one HTTP request of a window.
enum Fetch {
    /// `first` request answered 403/416: the caller may fall back to chunked mode.
    Rejected(StatusCode),
    /// The start offset is at or beyond the end of the resource (416).
    End,
    /// The window was cancelled or hit the memory cap.
    Stop,
    /// The body was read completely: `got` bytes appended; `whole` if the server ignored the
    /// range and sent everything up to the end of the resource.
    Body { got: u64, whole: bool },
}

/// One request `Range: bytes=start-[end]`, streamed into window `id`.
fn fetch(
    shared: &Shared,
    client: &Client,
    url: &str,
    id: u64,
    start: u64,
    end: Option<u64>,
    first: bool,
) -> io::Result<Fetch> {
    let range = match end {
        Some(end) => format!("bytes={start}-{end}"),
        None => format!("bytes={start}-"),
    };
    let mut resp = client
        .get(url)
        .header(RANGE, range)
        .header(ACCEPT_ENCODING, "identity")
        .timeout(REQUEST_TIMEOUT)
        .send()
        .map_err(http_error)?;
    let status = resp.status();
    let headers = resp.headers();

    if first && (status == StatusCode::FORBIDDEN || status == StatusCode::RANGE_NOT_SATISFIABLE) {
        return Ok(Fetch::Rejected(status));
    }
    if status == StatusCode::RANGE_NOT_SATISFIABLE {
        // Start is at or beyond the end of the resource.
        let total = headers
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_unsatisfied_range)
            .unwrap_or(start);
        let mut st = shared.lock();
        if st.total.is_none() {
            st.total = Some(total);
        }
        return Ok(Fetch::End);
    }
    if !status.is_success() {
        return Err(io::Error::other(format!("HTTP {status}")));
    }

    let (skip, total) = if status == StatusCode::PARTIAL_CONTENT {
        let (range_start, _, total) = headers
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_content_range)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "206 response without a valid Content-Range"))?;
        if range_start > start {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("server answered range starting at {range_start} instead of {start}"),
            ));
        }
        (start - range_start, total)
    } else if status == StatusCode::OK {
        // Range ignored: the body starts at offset 0.
        (start, resp.content_length())
    } else {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("unexpected HTTP {status}")));
    };

    {
        let mut st = shared.lock();
        if !st.is_active(id) {
            return Ok(Fetch::Stop);
        }
        if first {
            let accepts = headers
                .get(ACCEPT_RANGES)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.to_ascii_lowercase().contains("bytes"));
            st.ranges = status == StatusCode::PARTIAL_CONTENT || accepts;
            st.content_type = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(str::to_owned);
        }
        if st.total.is_none() {
            st.total = total;
        }
    }

    let whole = status == StatusCode::OK;
    let mut got = 0u64;
    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut skip = skip;
    loop {
        let n = match resp.read(&mut buf) {
            Ok(0) => return Ok(Fetch::Body { got, whole }),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let mut chunk = &buf[..n];
        if skip > 0 {
            let s = skip.min(n as u64) as usize;
            skip -= s as u64;
            chunk = &chunk[s..];
        }
        let mut st = shared.lock();
        let Some(w) = st.window_mut(id).filter(|w| w.status.active()) else {
            return Ok(Fetch::Stop); // cancelled: dropping `resp` closes the connection
        };
        if chunk.is_empty() {
            continue;
        }
        w.data.extend_from_slice(chunk);
        got += chunk.len() as u64;
        w.status = Status::Running;
        let capped = w.data.len() >= MEMORY_CAP;
        if capped {
            w.status = Status::Stopped;
        }
        drop(st);
        shared.cond.notify_all();
        if capped {
            return Ok(Fetch::Stop);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{Shutdown, TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    #[derive(Clone, Default)]
    struct Cfg {
        ranges: bool,
        chunk: usize,
        delay: Duration,
        not_found: bool,
        close_after: Option<usize>,
        /// googlevideo-like: 403 unless the request has a bounded range `bytes=a-b`.
        bounded_only: bool,
    }

    struct TestServer {
        url: String,
        log: Arc<Mutex<Vec<Option<String>>>>,
        aborted: Arc<AtomicUsize>,
        completed: Arc<AtomicUsize>,
    }

    impl TestServer {
        fn ranges_seen(&self) -> Vec<Option<String>> {
            self.log.lock().unwrap().clone()
        }
    }

    fn payload(len: usize) -> Arc<Vec<u8>> {
        let mut x: u64 = 0x1234_5678_9abc_def1;
        Arc::new(
            (0..len)
                .map(|_| {
                    x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    (x >> 33) as u8
                })
                .collect(),
        )
    }

    fn client() -> Client {
        Client::builder().no_proxy().build().unwrap()
    }

    fn serve(payload: Arc<Vec<u8>>, cfg: Cfg) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let log = Arc::new(Mutex::new(Vec::new()));
        let aborted = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));
        let (l, a, c) = (log.clone(), aborted.clone(), completed.clone());
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let (p, cfg, l, a, c) = (payload.clone(), cfg.clone(), l.clone(), a.clone(), c.clone());
                thread::spawn(move || handle(stream, &p, &cfg, &l, &a, &c));
            }
        });
        TestServer { url: format!("http://{addr}/track.mp3"), log, aborted, completed }
    }

    fn parse_range(v: &str) -> Option<(usize, Option<usize>)> {
        let (a, b) = v.trim().strip_prefix("bytes=")?.split_once('-')?;
        let b = if b.trim().is_empty() { None } else { Some(b.trim().parse().ok()?) };
        Some((a.trim().parse().ok()?, b))
    }

    fn handle(
        mut stream: TcpStream,
        payload: &[u8],
        cfg: &Cfg,
        log: &Mutex<Vec<Option<String>>>,
        aborted: &AtomicUsize,
        completed: &AtomicUsize,
    ) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
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
        log.lock().unwrap().push(range.clone());
        if cfg.not_found {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            return;
        }
        let len = payload.len();
        let parsed = range.as_deref().filter(|_| cfg.ranges).and_then(parse_range);
        if cfg.bounded_only && !matches!(parsed, Some((_, Some(_)))) {
            let _ = stream.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            return;
        }
        let (status, start, end, extra) = match parsed {
            Some((a, _)) if a >= len => {
                let _ = stream.write_all(
                    format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{len}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .as_bytes(),
                );
                return;
            }
            Some((a, b)) => {
                let b = b.unwrap_or(len - 1).min(len - 1);
                ("206 Partial Content", a, b + 1, format!("Content-Range: bytes {a}-{b}/{len}\r\n"))
            }
            None => ("200 OK", 0, len, String::new()),
        };
        let accept = if cfg.ranges { "Accept-Ranges: bytes\r\n" } else { "" };
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: audio/mpeg\r\nContent-Length: {}\r\n{accept}{extra}Connection: close\r\n\r\n",
            end - start
        );
        if stream.write_all(head.as_bytes()).is_err() {
            aborted.fetch_add(1, Ordering::SeqCst);
            return;
        }
        let body = &payload[start..end];
        let limit = cfg.close_after.map_or(body.len(), |c| c.min(body.len()));
        let chunk = if cfg.chunk == 0 { 64 * 1024 } else { cfg.chunk };
        for piece in body[..limit].chunks(chunk) {
            if stream.write_all(piece).and_then(|_| stream.flush()).is_err() {
                aborted.fetch_add(1, Ordering::SeqCst);
                return;
            }
            if !cfg.delay.is_zero() {
                thread::sleep(cfg.delay);
            }
        }
        if cfg.close_after.is_some() {
            let _ = stream.shutdown(Shutdown::Both);
            return;
        }
        completed.fetch_add(1, Ordering::SeqCst);
    }

    fn throttled(chunk: usize, delay_ms: u64) -> Cfg {
        Cfg { ranges: true, chunk, delay: Duration::from_millis(delay_ms), ..Cfg::default() }
    }

    fn read_at(src: &mut HttpSource, pos: u64, n: usize) -> Vec<u8> {
        src.seek(SeekFrom::Start(pos)).unwrap();
        let mut out = vec![0u8; n];
        src.read_exact(&mut out).unwrap();
        out
    }

    #[test]
    fn is_send_sync() {
        fn assert_send_sync<T: Send + Sync + 'static>() {}
        assert_send_sync::<HttpSource>();
    }

    #[test]
    fn content_range_parsing() {
        assert_eq!(parse_content_range("bytes 0-99/1000"), Some((0, 99, Some(1000))));
        assert_eq!(parse_content_range("bytes 5-9/*"), Some((5, 9, None)));
        assert_eq!(parse_content_range("garbage"), None);
        assert_eq!(parse_unsatisfied_range("bytes */1000"), Some(1000));
        assert_eq!(parse_unsatisfied_range("bytes 0-1/1000"), None);
    }

    #[test]
    fn sequential_read_matches_payload() {
        let data = payload(2 * 1024 * 1024);
        let server = serve(data.clone(), Cfg { ranges: true, ..Cfg::default() });
        let mut src = HttpSource::open(client(), &server.url).unwrap();
        assert_eq!(src.content_length(), Some(data.len() as u64));
        assert_eq!(src.content_type(), Some("audio/mpeg"));
        let mut out = Vec::new();
        src.read_to_end(&mut out).unwrap();
        assert!(out == *data);
        assert_eq!(server.ranges_seen(), vec![Some("bytes=0-".to_string())]);
    }

    #[test]
    fn content_length_from_plain_200() {
        let data = payload(100_000);
        let server = serve(data.clone(), Cfg::default());
        let mut src = HttpSource::open(client(), &server.url).unwrap();
        assert_eq!(src.content_length(), Some(100_000));
        let mut out = Vec::new();
        src.read_to_end(&mut out).unwrap();
        assert!(out == *data);
    }

    #[test]
    fn open_returns_before_slow_body_completes() {
        let data = payload(1024 * 1024);
        let server = serve(data.clone(), throttled(16 * 1024, 30)); // ~2 s for the whole body
        let t = Instant::now();
        let mut src = HttpSource::open(client(), &server.url).unwrap();
        assert!(t.elapsed() < Duration::from_secs(1), "open took {:?}", t.elapsed());
        assert_eq!(server.completed.load(Ordering::SeqCst), 0);
        let mut out = Vec::new();
        src.read_to_end(&mut out).unwrap();
        assert!(out == *data);
    }

    #[test]
    fn tail_probe_then_back_to_start_keeps_main_download() {
        let data = payload(2 * 1024 * 1024);
        let len = data.len();
        let server = serve(data.clone(), throttled(16 * 1024, 10));
        let mut src = HttpSource::open(client(), &server.url).unwrap();

        src.seek(SeekFrom::End(-128)).unwrap();
        let mut tail = vec![0u8; 128];
        src.read_exact(&mut tail).unwrap();
        assert_eq!(tail, data[len - 128..]);
        let mut extra = [0u8; 16];
        assert_eq!(src.read(&mut extra).unwrap(), 0, "EOF after tail");

        assert_eq!(read_at(&mut src, 0, 4096), data[..4096]);
        assert_eq!(
            server.ranges_seen(),
            vec![Some("bytes=0-".to_string()), Some(format!("bytes={}-", len - 128))]
        );

        // Larger tail read (mp4 moov-like) and back again.
        assert_eq!(read_at(&mut src, (len - 200_000) as u64, 200_000), data[len - 200_000..]);
        src.seek(SeekFrom::Start(0)).unwrap();
        let mut out = Vec::new();
        src.read_to_end(&mut out).unwrap();
        assert!(out == *data);
        // The main download was never restarted.
        let seen = server.ranges_seen();
        assert_eq!(seen.iter().filter(|r| r.as_deref() == Some("bytes=0-")).count(), 1);
    }

    #[test]
    fn random_seeks_match_payload() {
        let data = payload(2 * 1024 * 1024);
        let len = data.len() as u64;
        let server = serve(data.clone(), Cfg { ranges: true, ..Cfg::default() });
        let mut src = HttpSource::open(client(), &server.url).unwrap();
        let mut x: u64 = 42;
        let mut next = || {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            x >> 17
        };
        for i in 0..80 {
            let pos = next() % len;
            let n = (1 + next() % 100_000).min(len - pos) as usize;
            if i % 3 == 0 {
                let cur = src.stream_position().unwrap() as i64;
                src.seek(SeekFrom::Current(pos as i64 - cur)).unwrap();
            } else {
                src.seek(SeekFrom::Start(pos)).unwrap();
            }
            let mut out = vec![0u8; n];
            src.read_exact(&mut out).unwrap();
            assert_eq!(out, data[pos as usize..pos as usize + n], "pos {pos} n {n}");
        }
        src.seek(SeekFrom::Start(len + 10)).unwrap();
        let mut b = [0u8; 4];
        assert_eq!(src.read(&mut b).unwrap(), 0);
        assert!(src.seek(SeekFrom::Current(-(len as i64) - 100)).is_err());
    }

    #[test]
    fn far_forward_seek_uses_new_request_and_promotes() {
        let data = payload(2 * 1024 * 1024);
        let server = serve(data.clone(), throttled(16 * 1024, 20));
        let mut src = HttpSource::open(client(), &server.url).unwrap();
        let target = 1536 * 1024;
        assert_eq!(read_at(&mut src, target as u64, 64 * 1024), data[target..target + 64 * 1024]);
        assert!(server.ranges_seen().contains(&Some(format!("bytes={target}-"))));

        // Keep playing from there (triggers promotion), then go back to the start.
        let mut more = vec![0u8; 400 * 1024];
        src.read_exact(&mut more).unwrap();
        assert_eq!(more, data[target + 64 * 1024..target + 464 * 1024]);
        assert_eq!(read_at(&mut src, 0, 64 * 1024), data[..64 * 1024]);
        src.seek(SeekFrom::Start(0)).unwrap();
        let mut out = Vec::new();
        src.read_to_end(&mut out).unwrap();
        assert!(out == *data);
    }

    #[test]
    fn no_range_server_backward_seek_and_forward_wait() {
        let data = payload(1024 * 1024);
        let len = data.len();
        let server = serve(data.clone(), Cfg { ranges: false, chunk: 16 * 1024, delay: Duration::from_millis(10), ..Cfg::default() });
        let mut src = HttpSource::open(client(), &server.url).unwrap();
        assert_eq!(src.content_length(), Some(len as u64));
        let mut first = vec![0u8; 100_000];
        src.read_exact(&mut first).unwrap();
        assert_eq!(first, data[..100_000]);
        assert_eq!(read_at(&mut src, 10, 1000), data[10..1010]);
        assert_eq!(read_at(&mut src, 900_000, 50_000), data[900_000..950_000]);
        src.seek(SeekFrom::End(-100)).unwrap();
        let mut tail = Vec::new();
        src.read_to_end(&mut tail).unwrap();
        assert_eq!(tail, data[len - 100..]);
        assert_eq!(read_at(&mut src, 0, 10), data[..10]);
        assert_eq!(server.ranges_seen().len(), 1, "a single request");
    }

    #[test]
    fn not_found_fails_open() {
        let server = serve(payload(10), Cfg { not_found: true, ..Cfg::default() });
        let err = HttpSource::open(client(), &server.url).err().expect("open must fail");
        assert!(err.to_string().contains("404"), "{err}");
    }

    #[test]
    fn mid_body_close_errors_after_buffered_data() {
        let data = payload(1024 * 1024);
        let cut = 300 * 1024;
        let server = serve(data.clone(), Cfg { close_after: Some(cut), chunk: 16 * 1024, ..Cfg::default() });
        let mut src = HttpSource::open(client(), &server.url).unwrap();
        let mut got = Vec::new();
        let mut buf = [0u8; 8192];
        let err = loop {
            match src.read(&mut buf) {
                Ok(0) => panic!("unexpected EOF after {} bytes", got.len()),
                Ok(n) => got.extend_from_slice(&buf[..n]),
                Err(e) => break e,
            }
        };
        assert!(got.len() <= cut);
        assert_eq!(got, data[..got.len()]);
        // The error is sticky.
        assert!(src.read(&mut buf).is_err(), "{err}");
    }

    #[test]
    fn drop_stops_background_download() {
        let data = payload(4 * 1024 * 1024);
        let server = serve(data.clone(), throttled(16 * 1024, 20));
        let mut src = HttpSource::open(client(), &server.url).unwrap();
        let mut b = [0u8; 1000];
        src.read_exact(&mut b).unwrap();
        drop(src);
        let t = Instant::now();
        while server.aborted.load(Ordering::SeqCst) == 0 {
            assert!(t.elapsed() < Duration::from_secs(5), "server never saw the connection close");
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(server.completed.load(Ordering::SeqCst), 0);
    }

    fn bounded_only(chunk: usize, delay_ms: u64) -> Cfg {
        Cfg { bounded_only: true, ..throttled(chunk, delay_ms) }
    }

    fn assert_all_bounded(seen: &[Option<String>]) {
        for r in seen {
            let r = r.as_deref().expect("range header");
            let (_, b) = parse_range(r).expect("valid range");
            assert!(b.is_some(), "open-ended range sent in chunked mode: {r}");
        }
    }

    #[test]
    fn open_chunked_sequential_read_uses_bounded_ranges() {
        let data = payload(3 * 1024 * 1024 + 12_345);
        let len = data.len();
        let server = serve(data.clone(), bounded_only(64 * 1024, 0));
        let mut src = HttpSource::open_chunked(client(), &server.url).unwrap();
        assert_eq!(src.content_length(), Some(len as u64));
        assert_eq!(src.content_type(), Some("audio/mpeg"));
        let mut out = Vec::new();
        src.read_to_end(&mut out).unwrap();
        assert!(out == *data);
        let seen = server.ranges_seen();
        assert_all_bounded(&seen);
        assert_eq!(seen[0].as_deref(), Some("bytes=0-131071"), "small first chunk");
        assert_eq!(seen[1].as_deref(), Some("bytes=131072-1179647"));
        assert_eq!(seen.last().unwrap().as_deref(), Some(format!("bytes=2228224-{}", len - 1).as_str()));
        assert_eq!(seen.len(), 4);
    }

    #[test]
    fn open_falls_back_to_chunked_on_403() {
        let data = payload(2 * 1024 * 1024);
        let server = serve(data.clone(), bounded_only(64 * 1024, 0));
        let mut src = HttpSource::open(client(), &server.url).unwrap();
        assert_eq!(src.content_length(), Some(data.len() as u64));
        let mut out = Vec::new();
        src.read_to_end(&mut out).unwrap();
        assert!(out == *data);
        let seen = server.ranges_seen();
        assert_eq!(seen[0].as_deref(), Some("bytes=0-"));
        assert_eq!(seen[1].as_deref(), Some("bytes=0-131071"));
        assert_all_bounded(&seen[1..]);
    }

    #[test]
    fn chunked_open_returns_before_first_chunk_completes() {
        let data = payload(1024 * 1024);
        // First chunk (128 KiB) takes ~800 ms to arrive.
        let server = serve(data.clone(), bounded_only(16 * 1024, 100));
        let t = Instant::now();
        let mut src = HttpSource::open_chunked(client(), &server.url).unwrap();
        assert!(t.elapsed() < Duration::from_millis(500), "open took {:?}", t.elapsed());
        assert_eq!(server.completed.load(Ordering::SeqCst), 0);
        let mut b = vec![0u8; 1000];
        src.read_exact(&mut b).unwrap();
        assert_eq!(b, data[..1000]);
    }

    #[test]
    fn chunked_tail_probe_and_seeks() {
        let data = payload(3 * 1024 * 1024);
        let len = data.len();
        let server = serve(data.clone(), bounded_only(16 * 1024, 5));
        let mut src = HttpSource::open_chunked(client(), &server.url).unwrap();

        src.seek(SeekFrom::End(-128)).unwrap();
        let mut tail = vec![0u8; 128];
        src.read_exact(&mut tail).unwrap();
        assert_eq!(tail, data[len - 128..]);
        assert_eq!(read_at(&mut src, 0, 4096), data[..4096]);
        assert!(server.ranges_seen().contains(&Some(format!("bytes={}-{}", len - 128, len - 1))));

        // Far seek, keep playing (promotion), then random seeks and a full read.
        let target = 2 * 1024 * 1024;
        assert_eq!(read_at(&mut src, target as u64, 500 * 1024), data[target..target + 500 * 1024]);
        let first_aux = format!("bytes={target}-{}", target + FIRST_RANGE_CHUNK as usize - 1);
        assert!(server.ranges_seen().contains(&Some(first_aux)));
        for pos in [10usize, 1_000_000, 2_900_000, 500_000, len - 50_000] {
            assert_eq!(read_at(&mut src, pos as u64, 40_000), data[pos..pos + 40_000], "pos {pos}");
        }
        src.seek(SeekFrom::Start(0)).unwrap();
        let mut out = Vec::new();
        src.read_to_end(&mut out).unwrap();
        assert!(out == *data);
        assert_all_bounded(&server.ranges_seen());
    }
}
