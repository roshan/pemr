//! Thumbnail generation for non-DICOM image uploads (JPEG/PNG/WebP).
//!
//! For DICOMs the `dicom_import` module renders a thumbnail directly from
//! the decoded pixel data; this module is for anything that lands via
//! `POST /records` with `Content-Type: image/*`.

use std::io::Cursor;

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("decode: {0}")]
    Decode(String),
    #[error("encode: {0}")]
    Encode(String),
}

pub fn thumbnail_webp(bytes: &[u8], max_dim: u32) -> Result<Vec<u8>, ImageError> {
    let img = image::load_from_memory(bytes).map_err(|e| ImageError::Decode(e.to_string()))?;
    let thumb = img.thumbnail(max_dim, max_dim);
    let mut out = Vec::new();
    thumb
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::WebP)
        .map_err(|e| ImageError::Encode(e.to_string()))?;
    Ok(out)
}

/// True for content types we know image::load_from_memory can handle with
/// the features enabled in Cargo.toml (png + jpeg + webp).
pub fn can_thumbnail(content_type: &str) -> bool {
    matches!(
        content_type.split(';').next().unwrap_or("").trim(),
        "image/png" | "image/jpeg" | "image/jpg" | "image/webp"
    )
}

/// Sniff the real image format from magic bytes and return its canonical MIME
/// type. Used where a *stored* file must be known-good image bytes rather than
/// whatever content-type a client claimed — insurance cards, whose whole point
/// is that a caller can render them without inspecting them first.
pub fn sniff_mime(bytes: &[u8]) -> Option<&'static str> {
    image::guess_format(bytes).ok().map(|f| f.to_mime_type())
}

/// Canonical file extension for sniffed image bytes, for content-addressed
/// storage. Falls back to `None` when the bytes aren't a recognised image.
pub fn sniff_extension(bytes: &[u8]) -> Option<&'static str> {
    match image::guess_format(bytes).ok()? {
        image::ImageFormat::Png => Some("png"),
        image::ImageFormat::Jpeg => Some("jpg"),
        image::ImageFormat::WebP => Some("webp"),
        image::ImageFormat::Gif => Some("gif"),
        _ => None,
    }
}
