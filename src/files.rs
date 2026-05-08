use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::AsyncWriteExt;

pub struct StoredFile {
    pub relative_path: String,
    pub sha256_hex: String,
    pub byte_size: i64,
}

/// Write `bytes` to a content-addressed location under `root`.
/// The relative path is `<sha[0:2]>/<sha[2:4]>/<sha>.<ext>`. If a file with
/// the target path already exists, we leave it alone (deduplication).
pub async fn store_bytes(
    root: &Path,
    bytes: &[u8],
    extension: Option<&str>,
) -> std::io::Result<StoredFile> {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let sha256_hex = hex::encode(hasher.finalize());

    let ext = extension.unwrap_or("bin");
    let dir1 = &sha256_hex[0..2];
    let dir2 = &sha256_hex[2..4];
    let relative = format!("{dir1}/{dir2}/{sha256_hex}.{ext}");

    let dir = root.join(dir1).join(dir2);
    fs::create_dir_all(&dir).await?;
    let target: PathBuf = root.join(&relative);

    if !target.exists() {
        // write to temp + atomic rename
        let tmp = target.with_extension(format!("{ext}.tmp.{}", uuid::Uuid::now_v7()));
        let mut f = fs::File::create(&tmp).await?;
        f.write_all(bytes).await?;
        f.sync_all().await?;
        drop(f);
        fs::rename(&tmp, &target).await?;
    }

    Ok(StoredFile {
        relative_path: relative,
        sha256_hex,
        byte_size: bytes.len() as i64,
    })
}

/// Resolve a relative file_path under root. Refuses to escape via "..".
pub fn resolve(root: &Path, relative: &str) -> Option<PathBuf> {
    let p = Path::new(relative);
    if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return None;
    }
    Some(root.join(p))
}

pub fn extension_from_filename(name: &str) -> Option<&str> {
    Path::new(name).extension().and_then(|s| s.to_str())
}

pub fn extension_from_content_type(ct: &str) -> Option<&'static str> {
    match ct.split(';').next().unwrap_or("").trim() {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/heic" | "image/heif" => Some("heic"),
        "image/tiff" => Some("tiff"),
        "application/pdf" => Some("pdf"),
        "application/dicom" => Some("dcm"),
        "text/plain" => Some("txt"),
        "text/markdown" => Some("md"),
        _ => None,
    }
}
