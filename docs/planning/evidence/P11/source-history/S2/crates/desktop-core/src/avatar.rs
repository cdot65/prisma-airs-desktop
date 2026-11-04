//! The learner's avatar outside the renderer: the photos the learner may
//! choose and how they are prepared, the reviewed destinations an upload may
//! go to, the only Academy images the core reads back, and their hand-off to
//! the presentation as a data URL (the WebView's image policy admits
//! `data:` and nothing remote).
//!
//! Academy's avatar contract (pinned commit; `server/services/avatar.ts`):
//! `settings.avatarUploadUrl({contentType, size})` presigns a PUT of at most
//! 2 MiB of JPEG, PNG or WebP to its object store (Cloudflare R2) under the
//! stable key `avatars/<user>.<ext>`, and `settings.confirmAvatar({ext})`
//! records the public URL `<Academy origin>/api/avatars/<user>.<ext>?v=<n>`,
//! served without a credential. Because the key is stable, a stored PUT can
//! replace the photo Academy already serves before (or without) the
//! confirmation.
//!
//! As v1 did (`src/ui/academy/profile_media.rs`), a chosen photo of up to
//! 16 MiB is decoded within bounds, oriented and re-encoded as a 512-pixel
//! square JPEG within Academy's cap; what Academy serves back is decoded and
//! re-encoded the same way before the presentation sees it.

use url::Url;

/// Academy's `MAX_AVATAR_BYTES`: the prepared upload's cap.
pub const MAX_AVATAR_BYTES: usize = 2 * 1024 * 1024;
/// The largest source photo the learner may choose.
pub const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
/// Decoder bounds: no side above this many pixels ...
pub const MAX_DIMENSION: u32 = 8192;
/// ... and no more decoder memory than this, the decoded pixels included.
pub const MAX_DECODE_ALLOC: u64 = 128 * 1024 * 1024;
/// The prepared avatar is a square of this many pixels.
pub const AVATAR_SIZE: u32 = 512;
/// The prepared avatar's type and Academy extension.
pub const PREPARED_CONTENT_TYPE: &str = "image/jpeg";
pub const PREPARED_EXT: &str = "jpg";

/// The reviewed object-store host suffix for presigned avatar uploads.
pub const UPLOAD_HOST_SUFFIX: &str = ".r2.cloudflarestorage.com";

fn content_type(format: image::ImageFormat) -> Option<&'static str> {
    match format {
        image::ImageFormat::Jpeg => Some("image/jpeg"),
        image::ImageFormat::Png => Some("image/png"),
        image::ImageFormat::WebP => Some("image/webp"),
        _ => None,
    }
}

/// Decodes a JPEG, PNG or WebP of at most `limit` bytes within the decoder
/// bounds (the format is recognised from its bytes; names are not trusted),
/// fills a 512-pixel square, applies its EXIF orientation and re-encodes it
/// as a JPEG within Academy's cap. Returns the detected source type too.
/// Anything that does not decode completely is refused.
///
/// Every buffer is reserved from the memory budget before it is allocated:
/// the decoded pixels first, with the decoder given only what remains
/// (`image` 0.25's `into_decoder` forwards the limits but, unlike
/// `ImageReader::decode`, does not reserve the output buffer), then the
/// resampling of the centred square. Only that square is resampled (in
/// place, never the whole image scaled to cover it, which for an extreme
/// aspect ratio would be enormous), and orientation is applied to the
/// 512-pixel result (a centred square crop commutes with every EXIF
/// rotation and flip), so no full-size copy is made.
fn reencode(bytes: &[u8], limit: usize) -> Option<(&'static str, Vec<u8>)> {
    use image::ImageDecoder;
    if bytes.is_empty() || bytes.len() > limit {
        return None;
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let source = content_type(reader.format()?)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder().ok()?;
    limits.reserve(decoder.total_bytes()).ok()?;
    decoder.set_limits(limits.clone()).ok()?;
    let orientation = decoder.orientation().ok()?;
    let decoded = image::DynamicImage::from_decoder(decoder).ok()?;
    // Only the centred square is resampled, read in place: `resize` makes a
    // side × 512 RGBA f32 intermediate (16 bytes a pixel) and a 512-pixel
    // RGBA output, both reserved from what the budget has left.
    let (width, height) = (decoded.width(), decoded.height());
    let side = width.min(height);
    limits
        .reserve(u64::from(side) * u64::from(AVATAR_SIZE) * 16)
        .ok()?;
    limits
        .reserve(u64::from(AVATAR_SIZE) * u64::from(AVATAR_SIZE) * 4)
        .ok()?;
    let centre = image::imageops::crop_imm(
        &decoded,
        (width - side) / 2,
        (height - side) / 2,
        side,
        side,
    );
    let resampled = image::imageops::resize(
        &*centre,
        AVATAR_SIZE,
        AVATAR_SIZE,
        image::imageops::FilterType::Lanczos3,
    );
    drop(decoded);
    let mut square = image::DynamicImage::ImageRgba8(resampled);
    square.apply_orientation(orientation);
    let square = square.to_rgb8();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85)
        .encode_image(&square)
        .ok()?;
    (jpeg.len() <= MAX_AVATAR_BYTES).then_some((source, jpeg))
}

/// The avatar to upload for a chosen photo (a prepared 512-pixel JPEG), or
/// None when it is not a complete JPEG, PNG or WebP within the bounds.
pub fn prepare(bytes: &[u8]) -> Option<Vec<u8>> {
    reencode(bytes, MAX_SOURCE_BYTES).map(|(_, jpeg)| jpeg)
}

fn origin(url: &Url) -> String {
    url.origin().ascii_serialization()
}

/// A presigned upload destination is acceptable only when it is HTTPS with
/// no credentials or fragment, carries its signature in the query, and is
/// on the reviewed object store or on Academy's own origin (the synthetic
/// fixture endpoint).
pub fn upload_destination(text: &str, academy_origin: &str) -> Option<Url> {
    let url = Url::parse(text).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    let reviewed_store = host.ends_with(UPLOAD_HOST_SUFFIX)
        && host.len() > UPLOAD_HOST_SUFFIX.len()
        && url.port().is_none();
    (url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && url.query().is_some_and(|q| !q.is_empty())
        && (reviewed_store || origin(&url) == academy_origin))
        .then_some(url)
}

/// The only Academy image the core reads: an avatar on Academy's origin
/// (`/api/avatars/<id>.<ext>`, optionally `?v=<digits>`), never another
/// path, host, credential or fragment.
pub fn avatar_address(text: &str, academy_origin: &str) -> Option<Url> {
    let url = Url::parse(text).ok()?;
    let file = url.path().strip_prefix("/api/avatars/")?;
    let (stem, ext) = file.rsplit_once('.')?;
    let file_ok = !stem.is_empty()
        && stem.len() <= 128
        && stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && matches!(ext, "jpg" | "png" | "webp");
    let query_ok = url.query().is_none_or(|q| {
        q.strip_prefix("v=").is_some_and(|v| {
            !v.is_empty() && v.len() <= 20 && v.bytes().all(|b| b.is_ascii_digit())
        })
    });
    (url.scheme() == "https"
        && origin(&url) == academy_origin
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && file_ok
        && query_ok)
        .then_some(url)
}

/// An avatar Academy served, re-encoded as a `data:` JPEG, when its
/// declared type matches the image its bytes decode to within Academy's cap.
pub fn data_url(declared: &str, bytes: &[u8]) -> Option<String> {
    let (source, jpeg) = reencode(bytes, MAX_AVATAR_BYTES)?;
    (source == declared).then(|| format!("data:{PREPARED_CONTENT_TYPE};base64,{}", base64(&jpeg)))
}

/// Standard base64 with padding (RFC 4648 section 4).
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Synthetic encoded photos for tests (fixtures only).
#[cfg(any(test, feature = "test-fixtures"))]
pub mod samples {
    use image::{ImageEncoder, RgbImage};

    /// A solid-colour image encoded as `format` (PNG, JPEG or WebP).
    pub fn photo(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
        let pixels = RgbImage::from_pixel(width, height, image::Rgb([43, 108, 176]));
        let mut out = Vec::new();
        match format {
            image::ImageFormat::Png => image::codecs::png::PngEncoder::new(&mut out).write_image(
                pixels.as_raw(),
                width,
                height,
                image::ExtendedColorType::Rgb8,
            ),
            image::ImageFormat::Jpeg => {
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 95).write_image(
                    pixels.as_raw(),
                    width,
                    height,
                    image::ExtendedColorType::Rgb8,
                )
            }
            image::ImageFormat::WebP => image::codecs::webp::WebPEncoder::new_lossless(&mut out)
                .write_image(
                    pixels.as_raw(),
                    width,
                    height,
                    image::ExtendedColorType::Rgb8,
                ),
            _ => panic!("unsupported sample format"),
        }
        .expect("encode sample");
        out
    }

    /// A 7000×7000 PNG (about 730 KB): within the dimension bound, but its
    /// decoded pixels (147,000,000 bytes) exceed the decoder memory budget.
    pub fn over_budget_png() -> &'static [u8] {
        static PNG: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        PNG.get_or_init(|| photo(7000, 7000, image::ImageFormat::Png))
    }

    /// A JPEG's pixel dimensions.
    pub fn dimensions(jpeg: &[u8]) -> (u32, u32) {
        let decoded =
            image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg).expect("jpeg");
        (decoded.width(), decoded.height())
    }

    /// A 200×100 JPEG, red on the left and blue on the right, whose EXIF
    /// orientation says "rotate 90° clockwise" (6): shown upright it is
    /// 100×200 with red on top and blue below.
    pub fn rotated_jpeg() -> Vec<u8> {
        let pixels = RgbImage::from_fn(200, 100, |x, _| {
            if x < 100 {
                image::Rgb([220, 20, 20])
            } else {
                image::Rgb([20, 20, 220])
            }
        });
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
            .write_image(pixels.as_raw(), 200, 100, image::ExtendedColorType::Rgb8)
            .expect("encode sample");
        let mut out = jpeg[..2].to_vec();
        out.extend_from_slice(&exif_rotate_90());
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    /// An APP1 Exif segment: big-endian TIFF with one IFD0 entry,
    /// Orientation = 6 (rotate 90° clockwise).
    pub fn exif_rotate_90() -> Vec<u8> {
        let tiff: [u8; 26] = [
            b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0, 0, 0, 0,
            0,
        ];
        let mut app1 = vec![0xFF, 0xE1];
        let len = (2 + 6 + tiff.len()) as u16;
        app1.extend_from_slice(&len.to_be_bytes());
        app1.extend_from_slice(b"Exif\0\0");
        app1.extend_from_slice(&tiff);
        app1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageFormat;

    const ORIGIN: &str = "https://academy.example";

    fn dimensions(jpeg: &[u8]) -> (u32, u32) {
        let decoded = image::load_from_memory_with_format(jpeg, ImageFormat::Jpeg).expect("jpeg");
        (decoded.width(), decoded.height())
    }

    #[test]
    fn base64_matches_rfc_4648_vectors() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), encoded);
        }
    }

    #[test]
    fn chosen_photos_become_512_pixel_jpegs_within_academys_cap() {
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
            let prepared = prepare(&samples::photo(1200, 800, format)).expect("prepared");
            assert_eq!(&prepared[..3], &[0xFF, 0xD8, 0xFF], "{format:?} -> JPEG");
            assert!(prepared.len() <= MAX_AVATAR_BYTES);
            assert_eq!(dimensions(&prepared), (AVATAR_SIZE, AVATAR_SIZE));
        }
    }

    #[test]
    fn a_photo_larger_than_academys_upload_cap_is_still_accepted_as_a_source() {
        // A noisy PNG well above 2 MiB (as camera photos are) but below 16 MiB.
        let mut pixels = image::RgbImage::new(1400, 1400);
        let mut seed: u32 = 7;
        for p in pixels.pixels_mut() {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            *p = image::Rgb([(seed >> 8) as u8, (seed >> 16) as u8, (seed >> 24) as u8]);
        }
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(pixels)
            .write_to(&mut std::io::Cursor::new(&mut png), ImageFormat::Png)
            .expect("png");
        assert!(
            png.len() > MAX_AVATAR_BYTES && png.len() <= MAX_SOURCE_BYTES,
            "{}",
            png.len()
        );
        let prepared = prepare(&png).expect("prepared");
        assert!(prepared.len() <= MAX_AVATAR_BYTES);
        assert_eq!(dimensions(&prepared), (AVATAR_SIZE, AVATAR_SIZE));
    }

    #[test]
    fn exif_orientation_is_applied_to_the_prepared_square() {
        // Upright, the photo is red above blue; unrotated it would be red
        // beside blue.
        let prepared = prepare(&samples::rotated_jpeg()).expect("prepared");
        let square = image::load_from_memory_with_format(&prepared, ImageFormat::Jpeg)
            .expect("jpeg")
            .to_rgb8();
        assert_eq!(square.dimensions(), (AVATAR_SIZE, AVATAR_SIZE));
        let red = |p: &image::Rgb<u8>| p[0] > 150 && p[2] < 100;
        let blue = |p: &image::Rgb<u8>| p[2] > 150 && p[0] < 100;
        assert!(
            red(square.get_pixel(256, 64)),
            "top {:?}",
            square.get_pixel(256, 64)
        );
        assert!(
            blue(square.get_pixel(256, 448)),
            "bottom {:?}",
            square.get_pixel(256, 448)
        );
    }

    #[test]
    fn decoded_pixels_beyond_the_memory_budget_are_refused_before_decoding() {
        let png = samples::over_budget_png();
        assert!(
            png.len() <= MAX_AVATAR_BYTES,
            "small enough to be served back too"
        );
        let decoder = image::ImageReader::new(std::io::Cursor::new(png))
            .with_guessed_format()
            .expect("reader")
            .into_decoder()
            .expect("decoder");
        assert!(image::ImageDecoder::total_bytes(&decoder) > MAX_DECODE_ALLOC);
        assert_eq!(prepare(png), None);
        assert_eq!(data_url("image/png", png), None);
    }

    #[test]
    fn extreme_landscape_and_portrait_photos_are_prepared_from_their_centre() {
        // 8192x1 and 1x8192 decode to 24 KB; scaled to cover a 512 square
        // whole they would need gigabytes. Their centred 1-pixel square is
        // resampled instead, for a chosen photo and for readback.
        for (width, height) in [(MAX_DIMENSION, 1), (1, MAX_DIMENSION)] {
            let png = samples::photo(width, height, ImageFormat::Png);
            let prepared = prepare(&png).expect("prepared");
            assert_eq!(
                dimensions(&prepared),
                (AVATAR_SIZE, AVATAR_SIZE),
                "{width}x{height}"
            );
            assert!(
                data_url("image/png", &png).is_some(),
                "{width}x{height} read back"
            );
        }
    }

    #[test]
    fn decoded_pixels_and_their_resampling_together_stay_within_the_budget() {
        // 6400x6400: 122,880,000 decoded bytes fit the budget alone, but with
        // the 6400 x 512 resampling intermediate (52,428,800 bytes) they do not.
        let png = samples::photo(6400, 6400, ImageFormat::Png);
        assert_eq!(prepare(&png), None);
    }

    #[test]
    fn a_24_megapixel_rotated_photo_within_the_budget_is_prepared() {
        // 6000x4000 RGB (72 MB decoded) with orientation 6: a full-size
        // rotated copy would exceed the budget; orienting the square does not.
        let photo = samples::photo(6000, 4000, ImageFormat::Jpeg);
        let mut rotated = photo[..2].to_vec();
        rotated.extend_from_slice(&samples::exif_rotate_90());
        rotated.extend_from_slice(&photo[2..]);
        assert_eq!(
            dimensions(&prepare(&rotated).expect("prepared")),
            (AVATAR_SIZE, AVATAR_SIZE)
        );
    }

    #[test]
    fn malformed_unsupported_oversize_and_huge_photos_are_refused() {
        let png = samples::photo(64, 64, ImageFormat::Png);
        let jpeg = samples::photo(64, 64, ImageFormat::Jpeg);
        // Valid signatures followed by too little or wrong data.
        assert_eq!(
            prepare(b"\x89PNG\r\n\x1a\nsynthetic text, not pixels"),
            None
        );
        assert_eq!(prepare(&png[..png.len() / 2]), None);
        assert_eq!(prepare(&jpeg[..jpeg.len() / 3]), None);
        assert_eq!(prepare(b"RIFF\0\0\0\0WEBPVP8 broken"), None);
        // Other formats, empty and beyond the source cap.
        assert_eq!(prepare(b"GIF89a\x01\0\x01\0"), None);
        assert_eq!(prepare(b"<svg xmlns='http://www.w3.org/2000/svg'/>"), None);
        assert_eq!(prepare(b""), None);
        let mut big = png.clone();
        big.resize(MAX_SOURCE_BYTES + 1, 0);
        assert_eq!(prepare(&big), None);
        // Beyond the decoder's dimension bound (a cheap, solid image).
        let huge = samples::photo(MAX_DIMENSION + 1, 1, ImageFormat::Png);
        assert_eq!(prepare(&huge), None);
    }

    #[test]
    fn uploads_go_only_to_reviewed_destinations() {
        let ok = "https://acct.r2.cloudflarestorage.com/bucket/avatars/u.jpg?X-Amz-Signature=s";
        assert!(upload_destination(ok, ORIGIN).is_some());
        assert!(upload_destination("https://academy.example/upload?sig=1", ORIGIN).is_some());
        for bad in [
            "http://acct.r2.cloudflarestorage.com/b/a.jpg?sig=1",
            "https://acct.r2.cloudflarestorage.com/b/a.jpg",
            "https://evil.example/r2.cloudflarestorage.com?sig=1",
            "https://r2.cloudflarestorage.com.evil.example/a?sig=1",
            "https://.r2.cloudflarestorage.com/a?sig=1",
            "https://user:pw@acct.r2.cloudflarestorage.com/a?sig=1",
            "https://acct.r2.cloudflarestorage.com:8443/a?sig=1",
            "https://acct.r2.cloudflarestorage.com/a?sig=1#f",
            "https://academy.example:444/upload?sig=1",
            "not a url",
        ] {
            assert!(upload_destination(bad, ORIGIN).is_none(), "{bad}");
        }
    }

    #[test]
    fn only_avatars_on_academys_origin_are_read() {
        assert!(
            avatar_address(
                "https://academy.example/api/avatars/user_2abc.jpg?v=171",
                ORIGIN
            )
            .is_some()
        );
        assert!(
            avatar_address("https://academy.example/api/avatars/user-2.webp", ORIGIN).is_some()
        );
        for bad in [
            "https://cdn.example/api/avatars/u.jpg",
            "http://academy.example/api/avatars/u.jpg",
            "https://academy.example/api/trpc/settings.get",
            "https://academy.example/api/avatars/../trpc/x.jpg",
            "https://academy.example/api/avatars/u.svg",
            "https://academy.example/api/avatars/u.jpg?v=1&x=2",
            "https://academy.example/api/avatars/u.jpg#f",
            "https://u:p@academy.example/api/avatars/u.jpg",
        ] {
            assert!(avatar_address(bad, ORIGIN).is_none(), "{bad}");
        }
    }

    #[test]
    fn served_avatars_are_re_encoded_only_when_type_and_image_agree() {
        let png = samples::photo(32, 32, ImageFormat::Png);
        let src = data_url("image/png", &png).expect("data url");
        assert!(src.starts_with("data:image/jpeg;base64,/9j/"), "{src}");
        assert_eq!(
            data_url("image/jpeg", &png),
            None,
            "declared type must match"
        );
        assert_eq!(data_url("image/png", b"\x89PNG\r\n\x1a\nnot pixels"), None);
        assert_eq!(data_url("text/html", b"<script>"), None);
    }
}
