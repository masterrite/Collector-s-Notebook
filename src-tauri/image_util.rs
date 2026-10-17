// src-tauri/src/image_util.rs — photo files, thumbnails, base64 delivery.
//
// Same portable storage model as always: photos are bare filenames resolved
// against photos_dir() at access time; 250px square jpg thumbnails cached on
// disk. NEW for the webview: images travel to the UI as data: URLs, so no
// asset-protocol scope configuration is needed and <img> tags just work.

use crate::model::{photos_dir, thumbs_dir};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub fn resolve_photo(stored: &str) -> PathBuf {
    if stored.is_empty() {
        return PathBuf::new();
    }
    let p = Path::new(stored);
    if p.is_absolute() && p.exists() {
        return p.to_path_buf();
    }
    let name = p.file_name().unwrap_or(p.as_os_str());
    photos_dir().join(name)
}

/// Thumbnail filename is derived from the FULL stored filename (not just the
/// stem) so two photos that share a stem but differ by extension — e.g.
/// "abc.png" and "abc.webp" arriving via an imported dataset — can't collide to
/// the same cached thumbnail. The thumbnail itself is always JPEG.
pub fn thumb_path_for(stored: &str) -> PathBuf {
    let name = Path::new(stored)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("thumb");
    thumbs_dir().join(format!("{name}.jpg"))
}

/// One-time cleanup for thumbnails written by older builds, which named them
/// "<stem>.jpg" instead of the current "<full name>.jpg". Without this, a
/// legacy "abc.jpg" sits next to a freshly regenerated "abc.jpg.jpg" forever,
/// and the photo backup mirrors both. Legacy files are renamed to the current
/// scheme when unambiguous, otherwise deleted (they'll be regenerated on
/// demand). `photo_dirs` are the folders whose filenames these thumbs belong to.
pub fn migrate_legacy_thumbs(thumbs: &Path, photo_dirs: &[PathBuf]) {
    use std::collections::{HashMap, HashSet};
    let mut names: HashSet<String> = HashSet::new();
    for d in photo_dirs {
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                if e.path().is_file() {
                    names.insert(e.file_name().to_string_lossy().to_string());
                }
            }
        }
    }
    let mut stem_count: HashMap<String, usize> = HashMap::new();
    for n in &names {
        if let Some(s) = Path::new(n).file_stem().and_then(|s| s.to_str()) {
            *stem_count.entry(s.to_string()).or_default() += 1;
        }
    }
    for n in &names {
        let Some(stem) = Path::new(n).file_stem().and_then(|s| s.to_str()) else { continue };
        let legacy = thumbs.join(format!("{stem}.jpg"));
        let current = thumbs.join(format!("{n}.jpg"));
        // Same path (photo has no extension) or nothing to migrate.
        if legacy == current || !legacy.exists() {
            continue;
        }
        // "<stem>.jpg" is the *current* thumb of a photo literally named "<stem>".
        if names.contains(stem) {
            continue;
        }
        let ambiguous = stem_count.get(stem).copied().unwrap_or(0) > 1;
        if current.exists() || ambiguous {
            std::fs::remove_file(&legacy).ok();
        } else {
            std::fs::rename(&legacy, &current).ok();
        }
    }
}

pub fn generate_thumbnail(stored: &str) {
    if stored.is_empty() {
        return;
    }
    let src = resolve_photo(stored);
    if let Ok(img) = image::open(&src) {
        let thumb = img.resize(250, 250, image::imageops::FilterType::Lanczos3);
        let dest = thumb_path_for(stored);
        thumb
            .to_rgb8()
            .save_with_format(&dest, image::ImageFormat::Jpeg)
            .ok();
    }
}

pub fn delete_photo_files(stored: &str) {
    if stored.is_empty() {
        return;
    }
    std::fs::remove_file(resolve_photo(stored)).ok();
    std::fs::remove_file(thumb_path_for(stored)).ok();
}

pub fn copy_photo_file(stored: &str) -> Option<String> {
    if stored.is_empty() {
        return None;
    }
    let src = resolve_photo(stored);
    if !src.exists() {
        return None;
    }
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("jpg");
    let name = format!("{}.{}", Uuid::new_v4(), ext);
    let dest = photos_dir().join(&name);
    std::fs::copy(&src, &dest).ok()?;
    // Reuse the source's cached thumbnail if it exists (cheap file copy) instead
    // of re-decoding and re-encoding the full image; fall back to generation.
    let src_thumb = thumb_path_for(stored);
    let dst_thumb = thumb_path_for(&name);
    if !src_thumb.exists() || std::fs::copy(&src_thumb, &dst_thumb).is_err() {
        generate_thumbnail(&name);
    }
    Some(name)
}

pub fn import_picked_photo(src: &Path) -> Option<String> {
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("jpg");
    let name = format!("{}.{}", Uuid::new_v4(), ext);
    let dest = photos_dir().join(&name);
    std::fs::copy(src, &dest).ok()?;
    generate_thumbnail(&name);
    Some(name)
}

fn jpeg_data_url(img: &image::DynamicImage) -> Option<String> {
    let mut buf = Vec::new();
    img.to_rgb8()
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Jpeg)
        .ok()?;
    Some(format!("data:image/jpeg;base64,{}", B64.encode(buf)))
}

/// data: URL of the cached thumbnail. Fast path: the on-disk thumbnail is
/// already a small JPEG, so read its bytes and base64 them directly — no image
/// decode/re-encode. Only the fallback (thumbnail somehow missing after a
/// generate attempt, or reading the source instead) touches the `image` crate.
pub fn thumb_data_url(stored: &str) -> Option<String> {
    if stored.is_empty() {
        return None;
    }
    let thumb = thumb_path_for(stored);
    if !thumb.exists() {
        generate_thumbnail(stored);
    }
    // Fast path: raw JPEG bytes straight to base64.
    if let Ok(bytes) = std::fs::read(&thumb) {
        if !bytes.is_empty() {
            return Some(format!("data:image/jpeg;base64,{}", B64.encode(bytes)));
        }
    }
    // Fallback: thumbnail unreadable — decode the source and encode a data URL.
    let img = image::open(resolve_photo(stored)).ok()?;
    jpeg_data_url(&img)
}

/// data: URL of the photo capped at `max_px` on the longest side (lightbox),
/// plus its delivered pixel dimensions.
pub fn photo_data_url(stored: &str, max_px: u32) -> Option<(String, u32, u32)> {
    if stored.is_empty() {
        return None;
    }
    // No artificial resolution cap: the lightbox zooms for detail inspection,
    // so the full image must be delivered. Memory is managed on the UI side by
    // revoking the image when the lightbox closes (see ui/app.js), rather than
    // by shrinking the picture here. `max_px` still lets a caller opt into a
    // smaller delivery (e.g. a preview), but 0 means "no downscaling".
    let img = image::open(resolve_photo(stored)).ok()?;
    let img = if max_px > 0 && img.width().max(img.height()) > max_px {
        img.resize(max_px, max_px, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let url = jpeg_data_url(&img)?;
    Some((url, img.width(), img.height()))
}

/// Raw JPEG bytes of the photo (optionally downscaled to `max_px` on the
/// longest side; 0 = full resolution), plus its delivered pixel dimensions.
/// Unlike `photo_data_url`, this returns the bytes themselves rather than a
/// base64 data: URL. The UI wraps them in a Blob + object URL, which it can
/// revoke to free memory deterministically instead of leaving a large,
/// non-revocable data URL string live in the DOM/heap for the whole session.
pub fn photo_bytes(stored: &str, max_px: u32) -> Option<(Vec<u8>, u32, u32)> {
    if stored.is_empty() {
        return None;
    }
    let img = image::open(resolve_photo(stored)).ok()?;
    let img = if max_px > 0 && img.width().max(img.height()) > max_px {
        img.resize(max_px, max_px, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut buf = Vec::new();
    img.to_rgb8()
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Jpeg)
        .ok()?;
    Some((buf, img.width(), img.height()))
}
