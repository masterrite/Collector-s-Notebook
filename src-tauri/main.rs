// src-tauri/src/main.rs — Collector's Notebook, Tauri 2 backend.
//
// The Rust core (model.rs) is UNTOUCHED and reads/writes the same data files
// as the iced build — you can run both side by side. The UI state machine
// lives in the webview (ui/app.js); these commands are persistence, photo
// file management, and native dialogs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod image_util;
mod model;

use model::*;
use serde::Serialize;

#[derive(Serialize)]
struct LoadAll {
    data: AppData,
    settings: Settings,
    corrupt_backup: Option<String>,
}

/// Launch-time safety net: snapshot data.json into app_dir()/backups with a
/// timestamp, keep the newest 5. The file is tiny, so this is instant.
/// Photos are NOT copied (that could be gigabytes every launch) — they are
/// only ever deleted through the delete flows, which now confirm first.
fn backup_data_file() {
    let src = data_path();
    if !src.exists() {
        return;
    }
    let dir = app_dir().join("backups");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    std::fs::copy(&src, dir.join(format!("data-{stamp}.json"))).ok();
    // prune: keep the 5 newest
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("data-") && n.ends_with(".json"))
                    .unwrap_or(false)
            })
            .collect();
        files.sort();
        while files.len() > 5 {
            std::fs::remove_file(files.remove(0)).ok();
        }
    }
}

#[derive(Serialize)]
struct BackupInfo {
    file_name: String,
    stamp_secs: u64,
    size_bytes: u64,
}

fn backups_dir() -> std::path::PathBuf {
    app_dir().join("backups")
}

/// Newest-first list of data.json snapshots.
#[tauri::command]
fn list_backups() -> Vec<BackupInfo> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(backups_dir()) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let stamp = name
                .strip_prefix("data-")
                .and_then(|n| n.strip_suffix(".json"))
                .and_then(|n| n.parse::<u64>().ok());
            if let Some(stamp_secs) = stamp {
                let size_bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
                out.push(BackupInfo { file_name: name, stamp_secs, size_bytes });
            }
        }
    }
    out.sort_by_key(|b| std::cmp::Reverse(b.stamp_secs));
    out
}

#[tauri::command]
fn backup_now() -> bool {
    backup_data_file();
    data_path().exists()
}

/// Restore a snapshot: validate it parses, snapshot the CURRENT data first
/// (so the restore itself is undoable), swap the file in, return the parsed
/// data for the UI to adopt live. None = validation or IO failure; the
/// current data is untouched in that case.
#[tauri::command]
fn restore_backup(file_name: String) -> Option<AppData> {
    // refuse anything that isn't one of our snapshot names (no path tricks)
    if !file_name.starts_with("data-") || !file_name.ends_with(".json") || file_name.contains(['/', '\\']) {
        return None;
    }
    let src = backups_dir().join(&file_name);
    let text = std::fs::read_to_string(&src).ok()?;
    let parsed: AppData = serde_json::from_str(&text).ok()?;
    backup_data_file(); // safety snapshot of what's being replaced
    // Atomic swap: a crash mid-restore leaves the old data.json intact rather
    // than a truncated, unparseable file.
    if !model::atomic_write(&data_path(), text.as_bytes()) {
        return None;
    }
    Some(parsed)
}

#[derive(Serialize)]
struct PhotoArchiveStatus {
    archived_photos: u64,
    archived_thumbs: u64,
    archive_bytes: u64,
    orphaned: u64,          // in the archive but not referenced by any current item
    deleted_pending: u64,   // sitting in _deleted/, awaiting purge
}

fn photo_archive_dir() -> std::path::PathBuf {
    app_dir().join("photo-archive")
}

fn dir_file_count(dir: &std::path::Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter(|e| e.path().is_file()).count() as u64)
        .unwrap_or(0)
}

fn dir_size(dir: &std::path::Path) -> u64 {
    let mut total = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() {
                total += e.metadata().map(|m| m.len()).unwrap_or(0);
            } else if p.is_dir() {
                total += dir_size(&p);
            }
        }
    }
    total
}

/// Incremental ADDITIVE mirror: copy any file in src not already present in
/// dst (same name = same bytes, since photo filenames are content-unique
/// UUIDs). Never deletes from dst. Returns how many new files were copied.
fn mirror_new(src: &std::path::Path, dst: &std::path::Path) -> u64 {
    if std::fs::create_dir_all(dst).is_err() {
        return 0;
    }
    let mut copied = 0;
    if let Ok(rd) = std::fs::read_dir(src) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            if let Some(name) = p.file_name() {
                let target = dst.join(name);
                if !target.exists() && std::fs::copy(&p, &target).is_ok() {
                    copied += 1;
                }
            }
        }
    }
    copied
}

/// Back up photos + thumbnails incrementally (additive; deletions in the app
/// never remove archived copies). Returns how many new files were archived.
#[tauri::command]
fn backup_photos() -> u64 {
    let arch = photo_archive_dir();
    let a = mirror_new(&photos_dir(), &arch.join("photos"));
    let b = mirror_new(&thumbs_dir(), &arch.join("thumbnails"));
    a + b
}

/// Restore photos referenced by current data that are MISSING from the live
/// photos folder, pulling them back from the archive; regenerate any absent
/// thumbnails. `referenced` is the set of photo filenames the UI knows about.
/// Returns how many photos were restored.
#[tauri::command]
fn restore_missing_photos(referenced: Vec<String>) -> u64 {
    let arch_photos = photo_archive_dir().join("photos");
    let live = photos_dir();
    std::fs::create_dir_all(&live).ok();
    let mut restored = 0;
    for name in referenced {
        if name.trim().is_empty() {
            continue;
        }
        let live_path = live.join(&name);
        if live_path.exists() {
            // present, but make sure a thumbnail exists too
            if !image_util::thumb_path_for(&name).exists() {
                image_util::generate_thumbnail(&name);
            }
            continue;
        }
        let arch_path = arch_photos.join(&name);
        if arch_path.exists() && std::fs::copy(&arch_path, &live_path).is_ok() {
            image_util::generate_thumbnail(&name);
            restored += 1;
        }
    }
    restored
}

/// Archive files not referenced by any current item get moved to
/// photo-archive/_deleted/ (not erased) if they're not already there. Called
/// before reporting status so `orphaned`/`deleted_pending` are meaningful.
fn shunt_orphans(referenced: &std::collections::HashSet<String>) -> u64 {
    let arch = photo_archive_dir();
    let ap = arch.join("photos");
    let del = arch.join("_deleted");
    std::fs::create_dir_all(&del).ok();
    let mut moved = 0;
    if let Ok(rd) = std::fs::read_dir(&ap) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if !referenced.contains(&name)
                && std::fs::rename(&p, del.join(&name)).is_ok()
            {
                moved += 1;
            }
        }
    }
    moved
}

/// Count archived photos not referenced by any current item WITHOUT moving
/// anything — a pure read used by the status query.
fn count_orphans(referenced: &std::collections::HashSet<String>) -> u64 {
    let ap = photo_archive_dir().join("photos");
    let mut orphaned = 0;
    if let Ok(rd) = std::fs::read_dir(&ap) {
        for e in rd.flatten() {
            if !e.path().is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if !referenced.contains(&name) {
                orphaned += 1;
            }
        }
    }
    orphaned
}

/// Explicitly move archived photos not referenced by any current item into
/// _deleted/ (reversible — they aren't erased until purge). Returns how many
/// were moved. This is the mutating counterpart to `photo_archive_status`,
/// which is now a pure read.
#[tauri::command]
fn shunt_orphan_photos(referenced: Vec<String>) -> u64 {
    let refset: std::collections::HashSet<String> = referenced.into_iter().collect();
    shunt_orphans(&refset)
}

/// Pure read: never moves or deletes files. `orphaned` reports how many
/// archived photos are no longer referenced; call `shunt_orphan_photos` to act
/// on them.
#[tauri::command]
fn photo_archive_status(referenced: Vec<String>) -> PhotoArchiveStatus {
    let refset: std::collections::HashSet<String> = referenced.into_iter().collect();
    let arch = photo_archive_dir();
    let del = arch.join("_deleted");
    PhotoArchiveStatus {
        archived_photos: dir_file_count(&arch.join("photos")),
        archived_thumbs: dir_file_count(&arch.join("thumbnails")),
        archive_bytes: dir_size(&arch),
        orphaned: count_orphans(&refset),
        deleted_pending: dir_file_count(&del),
    }
}

/// Permanently delete archived photos in _deleted/ older than `days` days.
/// Returns how many were purged. This is the ONLY destructive archive op and
/// is always user-initiated.
#[tauri::command]
fn purge_deleted_photos(days: u64) -> u64 {
    let del = photo_archive_dir().join("_deleted");
    // Clamp to a sane range and use saturating math so a huge `days` can't
    // overflow the multiply (which panics in debug) or silently wrap. A large
    // value simply yields a very old cutoff (UNIX_EPOCH), purging nothing,
    // which is the safe direction.
    let days = days.min(36_500); // ~100 years is plenty
    let secs = days.saturating_mul(86_400);
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(secs))
        .unwrap_or(std::time::UNIX_EPOCH);
    let mut purged = 0;
    if let Ok(rd) = std::fs::read_dir(&del) {
        for e in rd.flatten() {
            let modified = e.metadata().and_then(|m| m.modified()).ok();
            if let Some(mt) = modified {
                if mt < cutoff {
                    // remove the photo and any matching thumbnail copy
                    let name = e.file_name().to_string_lossy().to_string();
                    if std::fs::remove_file(e.path()).is_ok() {
                        purged += 1;
                    }
                    let thumb = photo_archive_dir()
                        .join("thumbnails")
                        .join(std::path::Path::new(&name)
                            .with_extension("jpg")
                            .file_name()
                            .unwrap_or_default());
                    std::fs::remove_file(thumb).ok();
                }
            }
        }
    }
    purged
}

fn open_archive_folder() {
    let dir = photo_archive_dir();
    std::fs::create_dir_all(&dir).ok();
    #[cfg(target_os = "windows")]
    { std::process::Command::new("explorer").arg(&dir).spawn().ok(); }
    #[cfg(target_os = "macos")]
    { std::process::Command::new("open").arg(&dir).spawn().ok(); }
    #[cfg(target_os = "linux")]
    { std::process::Command::new("xdg-open").arg(&dir).spawn().ok(); }
}

#[tauri::command]
fn open_photo_archive() {
    open_archive_folder();
}

#[tauri::command]
fn load_all() -> LoadAll {
    backup_data_file();
    let settings = load_settings();
    let (mut data, corrupt) = load_data_reporting();
    sort_collections(&mut data, settings.coll_sort);
    LoadAll {
        data,
        settings,
        corrupt_backup: corrupt.map(|p| p.display().to_string()),
    }
}

#[tauri::command]
fn save_data_cmd(data: AppData) {
    save_data(&data);
}

#[tauri::command]
fn save_settings_cmd(settings: Settings) {
    save_settings(&settings);
}

#[tauri::command]
fn sort_collections_cmd(mut data: AppData, mode: SortMode) -> AppData {
    sort_collections(&mut data, mode);
    data
}

#[tauri::command]
fn thumb_b64(name: String) -> Option<String> {
    image_util::thumb_data_url(&name)
}

#[tauri::command]
fn photo_b64(name: String, max_px: u32) -> Option<(String, u32, u32)> {
    image_util::photo_data_url(&name, max_px)
}

/// Raw JPEG bytes + dimensions for the lightbox. The UI turns the bytes into a
/// Blob/object URL it can revoke, so photo memory is freed deterministically
/// (see the LRU cache in ui/app.js) rather than accumulating as data URLs.
#[tauri::command]
fn photo_bytes(name: String, max_px: u32) -> Option<(Vec<u8>, u32, u32)> {
    image_util::photo_bytes(&name, max_px)
}

/// Native multi-file picker → copies into photos_dir + thumbnails → returns
/// the new bare filenames. (Windows: rfd off the main thread is fine.)
#[tauri::command]
fn pick_photos() -> Vec<String> {
    let picked = rfd::FileDialog::new()
        .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"])
        .pick_files();
    picked
        .unwrap_or_default()
        .iter()
        .filter_map(|p| image_util::import_picked_photo(p))
        .collect()
}

#[tauri::command]
fn copy_photo(name: String) -> Option<String> {
    image_util::copy_photo_file(&name)
}

#[tauri::command]
fn delete_photo(name: String) {
    image_util::delete_photo_files(&name);
}

#[tauri::command]
fn export_data_cmd(data: AppData) -> bool {
    let picked = rfd::FileDialog::new()
        .set_file_name("collection-data.json")
        .add_filter("JSON", &["json"])
        .save_file();
    let Some(path) = picked else { return false };
    match serde_json::to_string_pretty(&data) {
        Ok(json) => std::fs::write(path, json).is_ok(),
        Err(_) => false,
    }
}

#[tauri::command]
fn import_data_cmd() -> Option<AppData> {
    let path = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file()?;
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<AppData>(&text).ok()
}

#[tauri::command]
fn open_data_folder() {
    let dir = app_dir();
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(&dir).spawn().ok();
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(&dir).spawn().ok();
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(&dir).spawn().ok();
    }
}

/// The app version, baked in at compile time from Cargo.toml's `version` field.
/// The UI shows this in Settings, so the footer always matches the crate version
/// with no second place to keep in sync.
#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Read plain text from the system clipboard. Used by the custom text-field
/// context menu's Paste. Going through Rust avoids the webview's
/// navigator.clipboard permission prompt. Returns "" on any failure.
#[tauri::command]
fn clipboard_read() -> String {
    arboard::Clipboard::new()
        .and_then(|mut c| c.get_text())
        .unwrap_or_default()
}

/// Write plain text to the system clipboard (custom menu's Copy).
#[tauri::command]
fn clipboard_write(text: String) {
    if let Ok(mut c) = arboard::Clipboard::new() {
        c.set_text(text).ok();
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            load_all,
            save_data_cmd,
            save_settings_cmd,
            sort_collections_cmd,
            thumb_b64,
            photo_b64,
            photo_bytes,
            pick_photos,
            copy_photo,
            delete_photo,
            export_data_cmd,
            import_data_cmd,
            open_data_folder,
            app_version,
            clipboard_read,
            clipboard_write,
            list_backups,
            backup_now,
            restore_backup,
            backup_photos,
            restore_missing_photos,
            photo_archive_status,
            shunt_orphan_photos,
            purge_deleted_photos,
            open_photo_archive
        ])
        .run(tauri::generate_context!())
        .expect("error while running application");
}
