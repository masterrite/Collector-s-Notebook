// src/model.rs — data model, persistence, sorting, date helpers.
//
// Carried over from the Slint version, with fixes:
//  * Sort modes use ONE numbering everywhere (see SortMode), removing the
//    Slint/Rust off-by-one mismatch.
//  * Dates round-trip losslessly via Option parts; the sort key is explicit.
//  * Collection item-counts honor the active search where relevant.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

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

impl Item {
    pub fn primary_photo(&self) -> Option<&str> {
        self.photos.first().map(|s| s.as_str()).filter(|s| !s.is_empty())
    }
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

impl AppData {
    /// The next monotonic order value for a newly created collection.
    pub fn next_coll_order(&self) -> u64 {
        self.collections.iter().map(|c| c.order).max().map_or(0, |m| m + 1)
    }
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn default_fields() -> Vec<CustomField> {
    ["CONDITION", "VALUE / PRICE", "TAGS", "NOTES"]
        .iter()
        .map(|l| CustomField { id: new_id(), label: (*l).into(), value: String::new() })
        .collect()
}

// ─── Sort modes — single source of truth ────────────────────────────────────
// Same five variants for both panels; the count/date pair is panel-specific in
// LABEL only, not in numbering. This removes the old Slint(0..4)/Rust scheme
// mismatch entirely.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortMode {
    Added,    // insertion order
    NameAsc,
    NameDesc,
    /// collections: fewest items · items: oldest acquired
    LowOrOld,
    /// collections: most items · items: newest acquired
    HighOrNew,
}

impl Default for SortMode {
    fn default() -> Self { SortMode::Added }
}

impl SortMode {
    pub fn all() -> [SortMode; 5] {
        [Self::Added, Self::NameAsc, Self::NameDesc, Self::LowOrOld, Self::HighOrNew]
    }
    pub fn label(self, is_collection: bool) -> &'static str {
        match self {
            Self::Added => "Date added",
            Self::NameAsc => "Name A–Z",
            Self::NameDesc => "Name Z–A",
            Self::LowOrOld => if is_collection { "Fewest items" } else { "Oldest first" },
            Self::HighOrNew => if is_collection { "Most items" } else { "Newest first" },
        }
    }
}

// ─── Settings ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub dark_mode: bool,
    pub accent_hex: String,
    /// Split ratios for the resizable pane_grid (each in 0..1). `left_ratio` is
    /// the left pane's share of the whole width; `mid_ratio` is the middle
    /// pane's share of the REMAINING width after the left pane (this matches
    /// pane_grid's nested-split model: outer split = left | rest, inner split =
    /// mid | right).
    #[serde(default = "default_left_ratio")]
    pub left_ratio: f32,
    #[serde(default = "default_mid_ratio")]
    pub mid_ratio: f32,
    pub font_size: f32,
    #[serde(default)]
    pub coll_sort: SortMode,
    #[serde(default)]
    pub item_sort: SortMode,
}

pub fn default_left_ratio() -> f32 { 0.25 }
// Mid pane's ratio is its share of the space RIGHT of the left pane. To make
// the middle pane ~25% of the whole window when left is 25%: 0.25 / 0.75 = 0.333.
pub fn default_mid_ratio() -> f32 { 0.333 }

impl Default for Settings {
    fn default() -> Self {
        Self {
            dark_mode: true,
            accent_hex: "#4f8ef7".into(),
            left_ratio: default_left_ratio(),
            mid_ratio: default_mid_ratio(),
            font_size: 15.0,
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

#[allow(dead_code)] // convenience wrapper; the app uses load_data_reporting directly
pub fn load_data() -> AppData {
    load_data_reporting().0
}

/// Like `load_data`, but also reports the path of a corrupt-data backup if one
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
    for item in &mut data.items {
        for p in &mut item.photos {
            if let Some(name) = std::path::Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
            {
                if name != p {
                    *p = name.to_string();
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
    if needs_order || data.collections.iter().all(|c| c.order == 0) {
        for (i, c) in data.collections.iter_mut().enumerate() {
            c.order = i as u64;
        }
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
fn atomic_write(path: &std::path::Path, bytes: &[u8]) {
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
        }
    } else {
        std::fs::remove_file(&tmp).ok();
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
    // Clamp pane ratios into the allowed range. This also self-heals older
    // settings.json files that stored wider ratios before the 0.4 cap existed.
    s.left_ratio = s.left_ratio.clamp(0.08, 0.33);
    s.mid_ratio = s.mid_ratio.clamp(0.12, 0.7);
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
        SortMode::Added => data.collections.sort_by(|a, b| a.order.cmp(&b.order)),
        SortMode::NameAsc => data.collections
            .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        SortMode::NameDesc => data.collections
            .sort_by(|a, b| b.name.to_lowercase().cmp(&a.name.to_lowercase())),
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

/// Order a borrowed item slice. Undated items always sort LAST.
pub fn sort_items(items: &mut Vec<&Item>, mode: SortMode) {
    use std::cmp::Ordering;
    let cmp_dates = |a: &str, b: &str, ascending: bool| -> Ordering {
        match (a.is_empty(), b.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => if ascending { a.cmp(b) } else { b.cmp(a) },
        }
    };
    match mode {
        SortMode::Added => {}
        SortMode::NameAsc => items
            .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        SortMode::NameDesc => items
            .sort_by(|a, b| b.name.to_lowercase().cmp(&a.name.to_lowercase())),
        SortMode::LowOrOld => items
            .sort_by(|a, b| cmp_dates(&a.acquired_date, &b.acquired_date, true)),
        SortMode::HighOrNew => items
            .sort_by(|a, b| cmp_dates(&a.acquired_date, &b.acquired_date, false)),
    }
}

/// Filtered + sorted item view for a collection. Search matches name, desc,
/// and any field label/value (case-insensitive).
pub fn filtered_items<'a>(
    data: &'a AppData,
    coll_id: &str,
    search: &str,
    sort: SortMode,
) -> Vec<&'a Item> {
    let s = search.to_lowercase();
    let mut v: Vec<&Item> = data.items.iter()
        .filter(|i| i.collection_id == coll_id)
        .filter(|i| {
            if s.is_empty() { return true; }
            i.name.to_lowercase().contains(&s)
                || i.short_desc.to_lowercase().contains(&s)
                || i.custom_fields.iter().any(|f|
                    f.value.to_lowercase().contains(&s)
                    || f.label.to_lowercase().contains(&s))
        })
        .collect();
    sort_items(&mut v, sort);
    v
}

// ─── Date helpers — lossless via Option parts ───────────────────────────────

/// Split stored key into (year, month, day) display strings. Zero parts -> "".
pub fn split_date(key: &str) -> (String, String, String) {
    if key.is_empty() { return (String::new(), String::new(), String::new()); }
    let parts: Vec<&str> = key.split('-').collect();
    let blank_if_zero = |s: &str| {
        let n: i64 = s.parse().unwrap_or(0);
        if n == 0 { String::new() } else { n.to_string() }
    };
    (
        blank_if_zero(parts.first().copied().unwrap_or("")),
        blank_if_zero(parts.get(1).copied().unwrap_or("")),
        blank_if_zero(parts.get(2).copied().unwrap_or("")),
    )
}

/// Assemble three boxes into a sortable key. All-blank => "" (undated).
/// Blanks become zeros so partial dates are preserved and still sort sanely
/// (a year-only date sorts after a fully-specified earlier date of same year).
pub fn assemble_date(y: &str, m: &str, d: &str) -> String {
    let (yt, mt, dt) = (y.trim(), m.trim(), d.trim());
    if yt.is_empty() && mt.is_empty() && dt.is_empty() { return String::new(); }
    let year = yt.parse::<i32>().unwrap_or(0).clamp(0, 9999);
    let month = { let v = mt.parse::<i32>().unwrap_or(0); if v == 0 { 0 } else { v.clamp(1, 12) } };
    let day = { let v = dt.parse::<i32>().unwrap_or(0); if v == 0 { 0 } else { v.clamp(1, 31) } };
    format!("{:04}-{:02}-{:02}", year, month, day)
}

/// Human display for view mode: "2023", "2023-03", "2023-03-15", or "—".
pub fn display_date(key: &str) -> String {
    let (y, m, d) = split_date(key);
    if y.is_empty() && m.is_empty() && d.is_empty() { return "—".into(); }
    let mut s = if y.is_empty() { "?".to_string() } else { y };
    if !m.is_empty() { s.push('-'); s.push_str(&m); }
    if !d.is_empty() { s.push('-'); s.push_str(&d); }
    s
}
