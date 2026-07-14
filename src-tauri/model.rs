// src/model.rs — data model, persistence, sorting, date helpers.
//
// Carried over from the Slint version, with fixes:
//  * Sort modes use ONE numbering everywhere (see SortMode), removing the
//    Slint/Rust off-by-one mismatch.
//  * Dates round-trip losslessly via Option parts; the sort key is explicit.
//  * Collection item-counts honor the active search where relevant.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ─── Core records ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub icon: String,
    /// Monotonic insertion order, used for the "Date added" sort. Defaults to 0
    /// for older data; normalized on load so existing collections keep order.
    #[serde(default)]
    pub order: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Item {
    pub id: String,
    pub collection_id: String,
    pub name: String,
    pub short_desc: String,
    /// photos[0] is the primary (card thumbnail + main image).
    #[serde(default)]
    pub photos: Vec<String>,
    /// Date acquired, pseudo-ISO "YYYY-MM-DD" with zero-filled unknown parts,
    /// or "" if fully unset. See date helpers below; treat purely as a sort key.
    #[serde(default)]
    pub acquired_date: String,
    pub custom_fields: Vec<CustomField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomField {
    pub id: String,
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub field_labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppData {
    #[serde(default)]
    pub collections: Vec<Collection>,
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub templates: Vec<Template>,
}


// ─── Sort modes — single source of truth ────────────────────────────────────
// Same five variants for both panels; the count/date pair is panel-specific in
// LABEL only, not in numbering. This removes the old Slint(0..4)/Rust scheme
// mismatch entirely.

// Serializes as kebab-case ("added", "name-asc", …). Each variant also accepts
// its legacy PascalCase spelling on read via #[serde(alias)], so existing
// settings.json files keep the user's chosen sort instead of resetting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SortMode {
    #[serde(alias = "Added")]
    #[default]
    Added,    // insertion order
    #[serde(alias = "NameAsc")]
    NameAsc,
    #[serde(alias = "NameDesc")]
    NameDesc,
    /// collections: fewest items · items: oldest acquired
    #[serde(alias = "LowOrOld")]
    LowOrOld,
    /// collections: most items · items: newest acquired
    #[serde(alias = "HighOrNew")]
    HighOrNew,
}


// ─── Settings ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_dark_mode")]
    pub dark_mode: bool,
    #[serde(default = "default_accent_hex")]
    pub accent_hex: String,
    /// Split ratios for the resizable panes (each a fraction of the WHOLE
    /// window width, 0..1). `left_ratio` is the left pane's share of the whole
    /// width; `mid_ratio` is the middle pane's share of the whole width. The
    /// right pane takes the remainder (1 - left - mid). This matches the CSS
    /// flexbox layout in ui/app.js, where each pane is sized as a percentage of
    /// the container. (The earlier pane_grid nested-split meaning is gone.)
    #[serde(default = "default_left_ratio")]
    pub left_ratio: f32,
    #[serde(default = "default_mid_ratio")]
    pub mid_ratio: f32,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default)]
    pub coll_sort: SortMode,
    #[serde(default)]
    pub item_sort: SortMode,
}

pub fn default_dark_mode() -> bool { true }
pub fn default_accent_hex() -> String { "#4f8ef7".into() }
pub fn default_font_size() -> f32 { 15.0 }
pub fn default_left_ratio() -> f32 { 0.25 }
// Mid pane is a fraction of the WHOLE window (not the remainder), matching the
// CSS layout. Default ~25% of the window.
pub fn default_mid_ratio() -> f32 { 0.25 }

impl Default for Settings {
    fn default() -> Self {
        Self {
            dark_mode: default_dark_mode(),
            accent_hex: default_accent_hex(),
            left_ratio: default_left_ratio(),
            mid_ratio: default_mid_ratio(),
            font_size: default_font_size(),
            coll_sort: SortMode::Added,
            item_sort: SortMode::Added,
        }
    }
}

// ─── Paths ──────────────────────────────────────────────────────────────────

pub fn app_dir() -> PathBuf {
    let mut p = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("Collectors-Notebook");
    std::fs::create_dir_all(&p).ok();
    p
}
pub fn photos_dir() -> PathBuf {
    let mut p = app_dir(); p.push("photos");
    std::fs::create_dir_all(&p).ok(); p
}
pub fn thumbs_dir() -> PathBuf {
    let mut p = photos_dir(); p.push("thumbnails");
    std::fs::create_dir_all(&p).ok(); p
}
pub fn data_path() -> PathBuf { let mut p = app_dir(); p.push("data.json"); p }
pub fn settings_path() -> PathBuf { let mut p = app_dir(); p.push("settings.json"); p }

// ─── Persistence ────────────────────────────────────────────────────────────

/// Loads app data, and also reports the path of a corrupt-data backup if one
/// was created on this load (so the UI can tell the user their previous data
/// couldn't be read and where the salvageable copy lives). `None` means the
/// data loaded cleanly or there was simply no file yet.
pub fn load_data_reporting() -> (AppData, Option<PathBuf>) {
    // Distinguish "no file yet" (legitimately empty first run) from "file
    // exists but won't parse" (corruption). In the corruption case we must NOT
    // silently fall back to an empty dataset, because the next save_data would
    // then overwrite the user's real (recoverable) file with nothing. Instead,
    // preserve the bad file under a timestamped .corrupt name first.
    let mut corrupt_backup: Option<PathBuf> = None;
    let mut data: AppData = match std::fs::read_to_string(data_path()) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(parsed) => parsed,
            Err(_) => {
                corrupt_backup = backup_corrupt_file(&data_path());
                AppData::default()
            }
        },
        // File missing or unreadable: normal first-run / empty state.
        Err(_) => AppData::default(),
    };
    // Migrate any legacy absolute photo paths to bare filenames so the data and
    // photos/ folder are portable across machines and user accounts.
    let mut migrated = false;
    for item in &mut data.items {
        for p in &mut item.photos {
            if let Some(name) = std::path::Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
            {
                if name != p {
                    *p = name.to_string();
                    migrated = true;
                }
            }
        }
    }
    // Normalize collection insertion order. Old data has order=0 everywhere; if
    // we detect duplicate/zero orders, reassign by current position so the
    // "Date added" sort is stable and reflects existing order.
    let needs_order = {
        let mut seen = std::collections::HashSet::new();
        data.collections.iter().any(|c| !seen.insert(c.order))
            && data.collections.len() > 1
    };
    if needs_order || (data.collections.iter().all(|c| c.order == 0) && !data.collections.is_empty()) {
        for (i, c) in data.collections.iter_mut().enumerate() {
            let ord = i as u64;
            if c.order != ord {
                c.order = ord;
                migrated = true;
            }
        }
    }
    // Persist the migration once, so it doesn't re-run every launch. Only when
    // the data actually changed AND it loaded cleanly (never overwrite on the
    // corruption path — we must preserve the salvageable original).
    if migrated && corrupt_backup.is_none() {
        save_data(&data);
    }
    (data, corrupt_backup)
}
pub fn save_data(data: &AppData) {
    if let Ok(json) = serde_json::to_string_pretty(data) {
        atomic_write(&data_path(), json.as_bytes());
    }
}

/// Write `bytes` to `path` atomically: write to a sibling temp file, flush, then
/// rename over the destination. A crash or power loss mid-write leaves either
/// the old file or the new file intact — never a truncated, unparseable one.
/// (rename is atomic when source and destination are on the same filesystem,
/// which they are here since the temp file sits in the same directory.)
/// Returns true only if the destination now holds the new bytes.
pub fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> bool {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    // Scope the file handle so it's closed (flushed) before the rename.
    let wrote = {
        match std::fs::File::create(&tmp) {
            Ok(mut f) => f.write_all(bytes).and_then(|_| f.sync_all()).is_ok(),
            Err(_) => false,
        }
    };
    if wrote {
        // If the rename fails, drop the temp file rather than leaving litter.
        if std::fs::rename(&tmp, path).is_err() {
            std::fs::remove_file(&tmp).ok();
            return false;
        }
        true
    } else {
        std::fs::remove_file(&tmp).ok();
        false
    }
}

/// Preserve an unparseable data/settings file before it can be overwritten, by
/// copying it to "<name>.corrupt-<unix_secs>". Returns the backup path on
/// success. Best-effort: failures yield `None` because this runs on a path
/// that's already degraded.
fn backup_corrupt_file(path: &std::path::Path) -> Option<PathBuf> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut backup = path.as_os_str().to_owned();
    backup.push(format!(".corrupt-{secs}"));
    let backup = PathBuf::from(backup);
    std::fs::copy(path, &backup).ok().map(|_| backup)
}

pub fn load_settings() -> Settings {
    // Settings corruption is far less costly than data corruption (the file is
    // small and regenerable), but we still preserve a bad one rather than
    // silently discarding it, for symmetry and easier debugging.
    let mut s: Settings = match std::fs::read_to_string(settings_path()) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(parsed) => parsed,
            Err(_) => {
                backup_corrupt_file(&settings_path());
                Settings::default()
            }
        },
        Err(_) => Settings::default(),
    };
    // Clamp pane ratios into the allowed range. Both are now fractions of the
    // whole window, so we additionally guarantee the right pane keeps at least
    // ~15% by capping left+mid. This also self-heals older settings.json files
    // that stored the previous "share of the remainder" mid_ratio (e.g. 0.333),
    // which would otherwise render the middle pane too wide.
    s.left_ratio = s.left_ratio.clamp(0.08, 0.33);
    s.mid_ratio = s.mid_ratio.clamp(0.12, 0.6);
    // Guard NaN/inf coming from a bad payload before the sum check.
    if !s.left_ratio.is_finite() { s.left_ratio = default_left_ratio(); }
    if !s.mid_ratio.is_finite() { s.mid_ratio = default_mid_ratio(); }
    if s.left_ratio + s.mid_ratio > 0.85 {
        s.mid_ratio = (0.85 - s.left_ratio).max(0.12);
    }
    if !s.font_size.is_finite() {
        s.font_size = default_font_size();
    }
    s.font_size = s.font_size.clamp(10.0, 28.0);
    s
}
pub fn save_settings(s: &Settings) {
    if let Ok(json) = serde_json::to_string_pretty(s) {
        atomic_write(&settings_path(), json.as_bytes());
    }
}

// ─── Sorting ────────────────────────────────────────────────────────────────

pub fn item_count(data: &AppData, coll_id: &str) -> usize {
    data.items.iter().filter(|i| i.collection_id == coll_id).count()
}

/// Sort the underlying collections vec so stable indices stay valid.
pub fn sort_collections(data: &mut AppData, mode: SortMode) {
    match mode {
        SortMode::Added => data.collections.sort_by_key(|a| a.order),
        SortMode::NameAsc => data.collections
            .sort_by_key(|a| a.name.to_lowercase()),
        SortMode::NameDesc => data.collections
            .sort_by_key(|b| std::cmp::Reverse(b.name.to_lowercase())),
        SortMode::LowOrOld | SortMode::HighOrNew => {
            let counts: std::collections::HashMap<String, usize> = data
                .collections.iter()
                .map(|c| (c.id.clone(), item_count(data, &c.id)))
                .collect();
            if mode == SortMode::LowOrOld {
                data.collections.sort_by(|a, b| counts[&a.id].cmp(&counts[&b.id]));
            } else {
                data.collections.sort_by(|a, b| counts[&b.id].cmp(&counts[&a.id]));
            }
        }
    }
}

