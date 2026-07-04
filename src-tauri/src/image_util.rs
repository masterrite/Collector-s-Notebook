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

pub fn thumb_path_for(stored: &str) -> PathBuf {
    let stem = Path::new(stored)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("thumb");
    thumbs_dir().join(format!("{stem}.jpg"))
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
    generate_thumbnail(&name);
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

/// data: URL of the cached thumbnail. The webview crops via CSS object-fit,
/// so no square-cropping is needed here — just the small cached image.
pub fn thumb_data_url(stored: &str) -> Option<String> {
    if stored.is_empty() {
        return None;
    }
    let thumb = thumb_path_for(stored);
    if !thumb.exists() {
        generate_thumbnail(stored);
    }
    let img = std::fs::read(&thumb)
        .ok()
        .and_then(|b| image::load_from_memory(&b).ok())
        .or_else(|| image::open(resolve_photo(stored)).ok())?;
    jpeg_data_url(&img)
}

/// data: URL of the photo capped at `max_px` on the longest side (lightbox),
/// plus its delivered pixel dimensions.
pub fn photo_data_url(stored: &str, max_px: u32) -> Option<(String, u32, u32)> {
    if stored.is_empty() {
        return None;
    }
    let img = image::open(resolve_photo(stored)).ok()?;
    let img = if img.width().max(img.height()) > max_px {
        img.resize(max_px, max_px, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let url = jpeg_data_url(&img)?;
    Some((url, img.width(), img.height()))
}
