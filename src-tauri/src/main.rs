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
    out.sort_by(|a, b| b.stamp_secs.cmp(&a.stamp_secs));
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
    std::fs::write(data_path(), text).ok()?;
    Some(parsed)
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

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            load_all,
            save_data_cmd,
            save_settings_cmd,
            sort_collections_cmd,
            thumb_b64,
            photo_b64,
            pick_photos,
            copy_photo,
            delete_photo,
            export_data_cmd,
            import_data_cmd,
            open_data_folder,
            list_backups,
            backup_now,
            restore_backup
        ])
        .run(tauri::generate_context!())
        .expect("error while running application");
}
