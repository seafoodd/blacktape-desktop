//! On-demand cover thumbnails, served through the `thumb://` URI scheme.
//!
//! The webview decodes images to full-size bitmaps (width x height x 4 bytes)
//! no matter how small they are displayed, so a 1400x1400 cover costs ~8 MB
//! even in a 64px list row. Lists request `thumb://.../<encoded path>?s=<px>`
//! instead and get a small JPEG. Originals are never modified or replaced; they
//! are still used wherever a cover is shown large.

use image::{
    codecs::jpeg::JpegEncoder, imageops::FilterType, DynamicImage, ImageFormat, ImageReader, Limits,
};
use std::fs;
use std::io::{BufWriter, Cursor};
use std::path::{Path, PathBuf};
use tauri::http::{header, Response, StatusCode};

/// Allowed thumbnail edge lengths (px). Requests are rounded UP to the next
/// bucket so the cache stays small and the result is never blurry.
pub const SIZE_BUCKETS: [u32; 4] = [96, 192, 384, 768];
const JPEG_QUALITY: u8 = 85;
/// Refuse absurdly large inputs (decompression bombs, non-cover files).
const MAX_SOURCE_BYTES: u64 = 50 * 1024 * 1024;
const MAX_DIMENSION: u32 = 12_000;

pub struct Rendered {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    /// True when the bytes are a newly encoded thumbnail (worth caching); false
    /// when the original file was passed through unchanged.
    pub resized: bool,
}

/// Rounds a requested size up to the next bucket (clamped to the largest).
pub fn bucket_size(requested: u32) -> u32 {
    SIZE_BUCKETS
        .iter()
        .copied()
        .find(|b| *b >= requested)
        .unwrap_or(SIZE_BUCKETS[SIZE_BUCKETS.len() - 1])
}

/// MIME type for a supported image extension, `None` for anything else.
pub fn mime_for(path: &Path) -> Option<&'static str> {
    match path
        .extension()?
        .to_str()?
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        _ => None,
    }
}

/// Parses `/<percent-encoded absolute path>` plus an optional `s=<px>` query.
/// Returns `None` for relative paths, parent-dir tricks and unsupported types.
pub fn parse_request(uri_path: &str, query: Option<&str>) -> Option<(PathBuf, u32)> {
    let encoded = uri_path.strip_prefix('/').unwrap_or(uri_path);
    let decoded = percent_encoding::percent_decode_str(encoded)
        .decode_utf8()
        .ok()?;
    let path = PathBuf::from(decoded.as_ref());
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        || mime_for(&path).is_none()
    {
        return None;
    }
    let requested = query
        .and_then(|q| {
            q.split('&')
                .find_map(|kv| kv.strip_prefix("s=").and_then(|v| v.parse::<u32>().ok()))
        })
        .unwrap_or(0);
    Some((path, bucket_size(requested)))
}

/// Cache file name; changes when the source file, its mtime or the size changes.
pub fn cache_file_name(src: &Path, mtime_secs: u64, size: u32) -> String {
    let key = format!("{}|{}", src.to_string_lossy(), mtime_secs);
    format!(
        "{:016x}_{}.jpg",
        xxhash_rust::xxh3::xxh3_64(key.as_bytes()),
        size
    )
}

fn encode_jpeg(img: &DynamicImage) -> Result<Vec<u8>, String> {
    let rgb = DynamicImage::ImageRgb8(img.to_rgb8());
    let mut out = Vec::new();
    let encoder = JpegEncoder::new_with_quality(BufWriter::new(Cursor::new(&mut out)), JPEG_QUALITY);
    rgb.write_with_encoder(encoder).map_err(|e| e.to_string())?;
    Ok(out)
}

fn mime_of_format(format: ImageFormat) -> Option<&'static str> {
    match format {
        ImageFormat::Jpeg => Some("image/jpeg"),
        ImageFormat::Png => Some("image/png"),
        ImageFormat::WebP => Some("image/webp"),
        ImageFormat::Gif => Some("image/gif"),
        ImageFormat::Bmp => Some("image/bmp"),
        _ => None,
    }
}

/// Opens `src` using the REAL format, sniffed from the file's leading bytes.
/// Covers are often misnamed (a PNG called `cover.jpg`), so the extension can't
/// be trusted, and content that matches no image format is rejected outright.
fn open_sniffed(src: &Path) -> Result<ImageReader<std::io::BufReader<fs::File>>, String> {
    use std::io::Read;
    let mut file = fs::File::open(src).map_err(|e| e.to_string())?;
    let mut head = [0u8; 32];
    let n = file.read(&mut head).map_err(|e| e.to_string())?;
    let format = image::guess_format(&head[..n]).map_err(|_| "not an image".to_string())?;
    let file = fs::File::open(src).map_err(|e| e.to_string())?;
    Ok(ImageReader::with_format(std::io::BufReader::new(file), format))
}

/// Returns the original bytes when the image already fits in `size`, otherwise
/// a downscaled JPEG whose longest edge is `size`. If the image can't be decoded
/// here but is a known image format, the original is passed through so the
/// browser can still try to show it (instead of a broken image).
pub fn render(src: &Path, size: u32) -> Result<Rendered, String> {
    let ext_mime = mime_for(src).ok_or("unsupported image type")?;
    let meta = fs::metadata(src).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > MAX_SOURCE_BYTES {
        return Err("not a usable image file".into());
    }

    let reader = open_sniffed(src)?;
    let format = reader.format().ok_or("not an image")?;
    let mime = mime_of_format(format).unwrap_or(ext_mime);
    let passthrough = || -> Result<Rendered, String> {
        Ok(Rendered {
            bytes: fs::read(src).map_err(|e| e.to_string())?,
            mime,
            resized: false,
        })
    };

    let Ok((w, h)) = reader.into_dimensions() else {
        return passthrough();
    };
    if w.max(h) <= size {
        return passthrough();
    }

    let mut reader = open_sniffed(src)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    reader.limits(limits);
    let Ok(img) = reader.decode() else {
        return passthrough();
    };

    // Triangle is an antialiased filter (support scales with the shrink ratio) and
    // several times faster than Lanczos3, which matters for multi-megapixel covers.
    let small = img.resize(size, size, FilterType::Triangle);
    Ok(Rendered {
        bytes: encode_jpeg(&small)?,
        mime: "image/jpeg",
        resized: true,
    })
}

/// Cached variant of [`render`]: generated once, then read from `cache_dir`.
pub fn get_or_create(cache_dir: &Path, src: &Path, size: u32) -> Result<Rendered, String> {
    let mtime = fs::metadata(src)
        .and_then(|m| m.modified())
        .map_err(|e| e.to_string())?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cached = cache_dir.join(cache_file_name(src, mtime, size));

    if let Ok(bytes) = fs::read(&cached) {
        return Ok(Rendered {
            bytes,
            mime: "image/jpeg",
            resized: true,
        });
    }

    let rendered = render(src, size)?;
    if rendered.resized && rendered.bytes.len() < MAX_SOURCE_BYTES as usize {
        // Best effort: write to a temp name first so readers never see a partial file.
        if fs::create_dir_all(cache_dir).is_ok() {
            let tmp = cached.with_extension("tmp");
            if fs::write(&tmp, &rendered.bytes).is_ok() {
                let _ = fs::rename(&tmp, &cached);
            }
        }
    }
    Ok(rendered)
}

fn error_response(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .body(Vec::new())
        .unwrap_or_default()
}

/// Thumbnail generation runs on a small dedicated pool: a grid can request
/// dozens of covers at once, and decoding that many multi-megapixel images in
/// parallel would thrash the CPU and balloon memory.
static POOL: std::sync::LazyLock<rayon::ThreadPool> = std::sync::LazyLock::new(|| {
    let threads = std::thread::available_parallelism()
        .map(|n| (n.get() / 2).clamp(2, 4))
        .unwrap_or(2);
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|i| format!("thumbs-{i}"))
        .build()
        .expect("failed to build thumbnail pool")
});

pub fn spawn_job(job: impl FnOnce() + Send + 'static) {
    POOL.spawn(job);
}

/// Entry point for the `thumb` URI scheme handler.
pub fn handle(cache_dir: Option<PathBuf>, uri_path: &str, query: Option<&str>) -> Response<Vec<u8>> {
    let Some((path, size)) = parse_request(uri_path, query) else {
        return error_response(StatusCode::BAD_REQUEST);
    };
    let Some(cache_dir) = cache_dir else {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR);
    };
    match get_or_create(&cache_dir, &path, size) {
        Ok(r) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, r.mime)
            .header(header::CACHE_CONTROL, "public, max-age=600")
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(r.bytes)
            .unwrap_or_default(),
        Err(_) => error_response(StatusCode::NOT_FOUND),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    use std::time::{Duration, UNIX_EPOCH};

    fn encode(path: &Path) -> String {
        utf8_percent_encode(&path.to_string_lossy(), NON_ALPHANUMERIC).to_string()
    }

    fn gradient(w: u32, h: u32) -> image::RgbImage {
        image::RgbImage::from_fn(w, h, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
        })
    }

    fn write_image(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
        let path = dir.join(name);
        gradient(w, h).save(&path).unwrap();
        path
    }

    fn set_mtime(path: &Path, secs: u64) {
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }

    fn file_count(dir: &Path) -> usize {
        fs::read_dir(dir).map(|rd| rd.count()).unwrap_or(0)
    }

    #[test]
    fn bucket_size_rounds_up_and_clamps() {
        assert_eq!(bucket_size(0), 96);
        assert_eq!(bucket_size(64), 96);
        assert_eq!(bucket_size(96), 96);
        assert_eq!(bucket_size(97), 192);
        assert_eq!(bucket_size(200), 384);
        assert_eq!(bucket_size(768), 768);
        assert_eq!(bucket_size(5000), 768);
    }

    #[test]
    fn mime_for_maps_supported_extensions_case_insensitively() {
        assert_eq!(mime_for(Path::new("a.jpg")), Some("image/jpeg"));
        assert_eq!(mime_for(Path::new("a.JPG")), Some("image/jpeg"));
        assert_eq!(mime_for(Path::new("a.JPEG")), Some("image/jpeg"));
        assert_eq!(mime_for(Path::new("a.png")), Some("image/png"));
        assert_eq!(mime_for(Path::new("a.PNG")), Some("image/png"));
        assert_eq!(mime_for(Path::new("a.webp")), Some("image/webp"));
        assert_eq!(mime_for(Path::new("a.WebP")), Some("image/webp"));
        assert_eq!(mime_for(Path::new("a.gif")), Some("image/gif"));
        assert_eq!(mime_for(Path::new("a.bmp")), Some("image/bmp"));
    }

    #[test]
    fn mime_for_rejects_unsupported_or_missing_extensions() {
        assert_eq!(mime_for(Path::new("a.txt")), None);
        assert_eq!(mime_for(Path::new("a.svg")), None);
        assert_eq!(mime_for(Path::new("cover")), None);
    }

    #[cfg(windows)]
    #[test]
    fn parse_request_decodes_windows_path_and_size() {
        let parsed = parse_request("/C%3A%5CUsers%5Cx%5Ccover.jpg", Some("s=200"));
        assert_eq!(parsed, Some((PathBuf::from(r"C:\Users\x\cover.jpg"), 384)));
    }

    #[cfg(unix)]
    #[test]
    fn parse_request_decodes_unix_path_and_size() {
        let parsed = parse_request("/%2Fhome%2Fx%2Fcover.png", Some("s=64"));
        assert_eq!(parsed, Some((PathBuf::from("/home/x/cover.png"), 96)));
    }

    #[test]
    fn parse_request_rejects_relative_path() {
        assert_eq!(parse_request("cover.jpg", None), None);
        assert_eq!(parse_request("/cover.jpg", None), None);
    }

    #[test]
    fn parse_request_rejects_parent_dir_components() {
        let encoded = if cfg!(windows) {
            "C%3A%5Cx%5C..%5Cy%5Ccover.jpg"
        } else {
            "%2Fx%2F..%2Fy%2Fcover.jpg"
        };
        assert_eq!(parse_request(&format!("/{encoded}"), None), None);
    }

    #[test]
    fn parse_request_rejects_unsupported_extension() {
        let encoded = if cfg!(windows) {
            "C%3A%5Cx%5Cnotes.txt"
        } else {
            "%2Fx%2Fnotes.txt"
        };
        assert_eq!(parse_request(&format!("/{encoded}"), Some("s=200")), None);
    }

    #[test]
    fn parse_request_defaults_to_smallest_bucket_without_valid_size() {
        let encoded = if cfg!(windows) {
            "C%3A%5Cx%5Ccover.jpg"
        } else {
            "%2Fx%2Fcover.jpg"
        };
        let uri = format!("/{encoded}");
        let (_, size) = parse_request(&uri, None).unwrap();
        assert_eq!(size, 96);
        let (_, size) = parse_request(&uri, Some("s=abc")).unwrap();
        assert_eq!(size, 96);
    }

    #[test]
    fn cache_file_name_is_stable_for_same_inputs() {
        let src = Path::new("/music/a/cover.jpg");
        assert_eq!(
            cache_file_name(src, 100, 192),
            cache_file_name(src, 100, 192)
        );
    }

    #[test]
    fn cache_file_name_changes_with_size_mtime_and_path() {
        let src = Path::new("/music/a/cover.jpg");
        let base = cache_file_name(src, 100, 192);
        assert_ne!(base, cache_file_name(src, 100, 384));
        assert_ne!(base, cache_file_name(src, 101, 192));
        assert_ne!(
            base,
            cache_file_name(Path::new("/music/b/cover.jpg"), 100, 192)
        );
    }

    #[test]
    fn cache_file_name_ends_with_size_and_jpg() {
        let name = cache_file_name(Path::new("/music/a/cover.png"), 1, 384);
        assert!(name.ends_with("_384.jpg"), "{name}");
    }

    #[test]
    fn render_downscales_large_image_keeping_aspect_ratio() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_image(dir.path(), "big.jpg", 800, 600);
        let rendered = render(&src, 192).unwrap();
        assert_eq!(rendered.mime, "image/jpeg");
        let decoded = image::load_from_memory(&rendered.bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (192, 144));
    }

    #[test]
    fn render_returns_original_bytes_when_image_already_fits() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_image(dir.path(), "small.png", 100, 100);
        let rendered = render(&src, 192).unwrap();
        assert_eq!(rendered.bytes, fs::read(&src).unwrap());
        assert_eq!(rendered.mime, "image/png");
    }

    #[test]
    fn render_rejects_non_image_content_with_image_extension() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("x.jpg");
        fs::write(&src, b"this is not an image").unwrap();
        assert!(render(&src, 192).is_err());
    }

    #[test]
    fn render_rejects_unsupported_extension() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("x.txt");
        fs::write(&src, b"hello").unwrap();
        assert!(render(&src, 192).is_err());
    }

    #[test]
    fn get_or_create_writes_one_cache_file_and_serves_identical_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let src = write_image(dir.path(), "big.jpg", 800, 600);
        set_mtime(&src, 1_000_000_000);

        let first = get_or_create(&cache, &src, 192).unwrap();
        assert_eq!(file_count(&cache), 1);
        let second = get_or_create(&cache, &src, 192).unwrap();
        assert_eq!(first.bytes, second.bytes);
        assert_eq!(second.mime, "image/jpeg");
        assert_eq!(file_count(&cache), 1);
    }

    #[test]
    fn get_or_create_does_not_cache_small_png() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let src = write_image(dir.path(), "small.png", 100, 100);
        let rendered = get_or_create(&cache, &src, 192).unwrap();
        assert_eq!(rendered.bytes, fs::read(&src).unwrap());
        assert_eq!(file_count(&cache), 0);
    }

    #[test]
    fn get_or_create_does_not_cache_small_jpg() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let src = write_image(dir.path(), "small.jpg", 100, 100);
        let rendered = get_or_create(&cache, &src, 192).unwrap();
        assert_eq!(rendered.bytes, fs::read(&src).unwrap());
        assert_eq!(file_count(&cache), 0);
    }

    #[test]
    fn get_or_create_uses_new_cache_entry_after_source_changes() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let src = write_image(dir.path(), "big.jpg", 800, 600);
        set_mtime(&src, 1_000_000_000);
        get_or_create(&cache, &src, 192).unwrap();
        let old_name = cache_file_name(&src, 1_000_000_000, 192);
        assert!(cache.join(&old_name).exists());

        write_image(dir.path(), "big.jpg", 1024, 512);
        set_mtime(&src, 2_000_000_000);
        let rendered = get_or_create(&cache, &src, 192).unwrap();

        let new_name = cache_file_name(&src, 2_000_000_000, 192);
        assert_ne!(old_name, new_name);
        assert!(cache.join(&new_name).exists());
        assert_eq!(file_count(&cache), 2);
        let decoded = image::load_from_memory(&rendered.bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (192, 96));
    }

    #[test]
    fn handle_returns_jpeg_for_large_png() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let src = write_image(dir.path(), "big.png", 800, 600);
        let resp = handle(Some(cache), &format!("/{}", encode(&src)), Some("s=192"));
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/jpeg"
        );
        assert!(!resp.body().is_empty());
    }

    #[test]
    fn handle_returns_400_for_bad_path() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let resp = handle(Some(cache), "/cover.jpg", None);
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn handle_returns_404_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let missing = dir.path().join("missing.jpg");
        let resp = handle(Some(cache), &format!("/{}", encode(&missing)), Some("s=192"));
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn handle_returns_500_without_cache_dir() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_image(dir.path(), "big.png", 800, 600);
        let resp = handle(None, &format!("/{}", encode(&src)), Some("s=192"));
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;

    fn write_png(path: &Path, w: u32, h: u32) {
        image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 90]))
            .save_with_format(path, image::ImageFormat::Png)
            .unwrap();
    }

    #[test]
    fn png_misnamed_as_jpg_is_decoded_by_content_not_extension() {
        // Real-world: Bandcamp/tag covers saved as `cover.jpg` that are actually PNG.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("cover.jpg");
        write_png(&p, 800, 800);

        let r = render(&p, 192).unwrap();
        assert!(r.resized);
        assert_eq!(r.mime, "image/jpeg");
        let out = image::load_from_memory(&r.bytes).unwrap();
        assert_eq!((out.width(), out.height()), (192, 192));
    }

    #[test]
    fn small_misnamed_file_is_passed_through_with_its_real_mime() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("cover.jpg");
        write_png(&p, 64, 64);

        let r = render(&p, 192).unwrap();
        assert!(!r.resized);
        assert_eq!(r.mime, "image/png");
        assert_eq!(r.bytes, std::fs::read(&p).unwrap());
    }

    #[test]
    fn undecodable_known_format_falls_back_to_the_original() {
        // PNG header + garbage: dimensions can't be read or the decode fails, but the
        // browser might still cope, so we serve the original instead of a 404.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("broken.png");
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend_from_slice(&[0u8; 64]);
        std::fs::write(&p, &bytes).unwrap();

        let r = render(&p, 96).unwrap();
        assert!(!r.resized);
        assert_eq!(r.bytes, bytes);
    }

    #[test]
    fn mime_of_format_maps_supported_formats() {
        assert_eq!(mime_of_format(ImageFormat::Jpeg), Some("image/jpeg"));
        assert_eq!(mime_of_format(ImageFormat::Png), Some("image/png"));
        assert_eq!(mime_of_format(ImageFormat::WebP), Some("image/webp"));
        assert_eq!(mime_of_format(ImageFormat::Tiff), None);
    }
}
