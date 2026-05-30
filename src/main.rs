// src/main.rs — Collector v5
#![windows_subsystem = "windows"]

slint::include_modules!();
use slint::Model;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::rc::Rc;
use std::cell::RefCell;
use uuid::Uuid;

// ─── Data Model ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Collection {
    id: String,
    name: String,
    icon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Item {
    id: String,
    collection_id: String,
    name: String,
    short_desc: String,
    thumbnail_path: Option<String>,
    // All fields including defaults are stored as custom_fields
    custom_fields: Vec<CustomField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CustomField {
    id: String,
    label: String,
    value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Template {
    id: String,
    name: String,
    field_labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AppData {
    collections: Vec<Collection>,
    items: Vec<Item>,
    #[serde(default)]
    templates: Vec<Template>,
}

// ─── Default fields for new items ─────────────────────────────────────────────

fn default_fields() -> Vec<CustomField> {
    vec![
        CustomField { id: Uuid::new_v4().to_string(), label: "ACQUIRED".into(),    value: "".into() },
        CustomField { id: Uuid::new_v4().to_string(), label: "CONDITION".into(),   value: "".into() },
        CustomField { id: Uuid::new_v4().to_string(), label: "VALUE / PRICE".into(), value: "".into() },
        CustomField { id: Uuid::new_v4().to_string(), label: "TAGS".into(),        value: "".into() },
        CustomField { id: Uuid::new_v4().to_string(), label: "NOTES".into(),       value: "".into() },
    ]
}

// ─── Settings ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Settings {
    dark_mode: bool,
    accent_hex: String,
    left_panel_width: f32,
    mid_panel_width: f32,
    font_size: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            dark_mode: true,
            accent_hex: "#4f8ef7".into(),
            left_panel_width: 220.0,
            mid_panel_width: 268.0,
            font_size: 15.0,
        }
    }
}

// ─── Colour helpers ───────────────────────────────────────────────────────────

fn hex(s: &str) -> slint::Color {
    let s = s.trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).unwrap_or(0);
    slint::Color::from_rgb_u8(((v>>16)&0xff) as u8, ((v>>8)&0xff) as u8, (v&0xff) as u8)
}
fn darken(c: slint::Color, t: f32) -> slint::Color {
    slint::Color::from_rgb_u8(
        (c.red()   as f32*(1.0-t)) as u8,
        (c.green() as f32*(1.0-t)) as u8,
        (c.blue()  as f32*(1.0-t)) as u8)
}
fn lighten(c: slint::Color, t: f32) -> slint::Color {
    let l = |v: u8| (v as f32 + (255.0 - v as f32)*t) as u8;
    slint::Color::from_rgb_u8(l(c.red()), l(c.green()), l(c.blue()))
}
fn with_alpha(c: slint::Color, a: f32) -> slint::Color {
    slint::Color::from_argb_u8((a*255.0) as u8, c.red(), c.green(), c.blue())
}
fn color_to_hex(c: slint::Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.red(), c.green(), c.blue())
}

// ─── Palette ──────────────────────────────────────────────────────────────────

struct Palette {
    bg_base:slint::Color, bg_panel:slint::Color, bg_surface:slint::Color,
    bg_elevated:slint::Color, bg_card:slint::Color, bg_card_hover:slint::Color,
    bg_selected:slint::Color, bg_input:slint::Color,
    text_primary:slint::Color, text_secondary:slint::Color, text_muted:slint::Color,
    border:slint::Color, danger_bg:slint::Color, danger_text:slint::Color,
}

fn dark_palette() -> Palette { Palette {
    bg_base:hex("#0f1117"), bg_panel:hex("#161b24"), bg_surface:hex("#1e2535"),
    bg_elevated:hex("#252d40"), bg_card:hex("#1a2130"), bg_card_hover:hex("#212a3e"),
    bg_selected:hex("#1d3557"), bg_input:hex("#0f1117"),
    text_primary:hex("#e8edf5"), text_secondary:hex("#8896b0"), text_muted:hex("#4a5568"),
    border:hex("#252d40"), danger_bg:hex("#3d1515"), danger_text:hex("#f87171"),
}}

fn light_palette() -> Palette { Palette {
    bg_base:hex("#f4f6fb"), bg_panel:hex("#ffffff"), bg_surface:hex("#edf0f7"),
    bg_elevated:hex("#e2e6f0"), bg_card:hex("#f8f9fd"), bg_card_hover:hex("#eef1f8"),
    bg_selected:hex("#dce8ff"), bg_input:hex("#ffffff"),
    text_primary:hex("#0f1117"), text_secondary:hex("#4a5568"), text_muted:hex("#9aa3b5"),
    border:hex("#d6dcea"), danger_bg:hex("#fff0f0"), danger_text:hex("#e53935"),
}}

fn apply_theme(ui: &AppWindow, dark: bool, accent_hex: &str) {
    let p = if dark { dark_palette() } else { light_palette() };
    let t = ui.global::<Theme>();
    t.set_dark_mode(dark);
    t.set_bg_base(p.bg_base); t.set_bg_panel(p.bg_panel); t.set_bg_surface(p.bg_surface);
    t.set_bg_elevated(p.bg_elevated); t.set_bg_card(p.bg_card); t.set_bg_card_hover(p.bg_card_hover);
    t.set_bg_selected(p.bg_selected); t.set_bg_input(p.bg_input);
    t.set_text_primary(p.text_primary); t.set_text_secondary(p.text_secondary); t.set_text_muted(p.text_muted);
    t.set_border(p.border); t.set_danger_bg(p.danger_bg); t.set_danger_text(p.danger_text);
    let accent = hex(accent_hex);
    t.set_accent(accent); t.set_accent_dim(darken(accent, 0.42));
    t.set_accent_glow(with_alpha(accent, 0.18)); t.set_accent_text(lighten(accent, 0.25));
    t.set_border_accent(with_alpha(accent, 0.32));
}

// ─── Paths ────────────────────────────────────────────────────────────────────

fn app_dir() -> PathBuf {
    let mut p = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("collector");
    std::fs::create_dir_all(&p).ok();
    p
}
fn photos_dir() -> PathBuf {
    let mut p = app_dir(); p.push("photos");
    std::fs::create_dir_all(&p).ok(); p
}
fn data_path()     -> PathBuf { let mut p = app_dir(); p.push("data.json");     p }
fn settings_path() -> PathBuf { let mut p = app_dir(); p.push("settings.json"); p }

// ─── Persistence ──────────────────────────────────────────────────────────────

fn load_data() -> AppData {
    std::fs::read_to_string(data_path()).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| AppData {
            collections: vec![
                Collection { id: Uuid::new_v4().to_string(), name: "Headphones".into(),    icon: "🎧".into() },
                Collection { id: Uuid::new_v4().to_string(), name: "Fountain Pens".into(), icon: "✒".into() },
            ],
            items: vec![],
            templates: vec![],
        })
}

fn save_data(data: &AppData) {
    if let Ok(json) = serde_json::to_string_pretty(data) {
        std::fs::write(data_path(), json).ok();
    }
}

fn load_settings() -> Settings {
    std::fs::read_to_string(settings_path()).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_settings(s: &Settings) {
    if let Ok(json) = serde_json::to_string_pretty(s) {
        std::fs::write(settings_path(), json).ok();
    }
}

// ─── Model helpers ────────────────────────────────────────────────────────────

fn to_slint_collections(data: &AppData) -> slint::ModelRc<CollectionData> {
    collections_model(data, "")
}

fn collections_model(data: &AppData, search: &str) -> slint::ModelRc<CollectionData> {
    let s = search.to_lowercase();
    let v: Vec<CollectionData> = data.collections.iter().map(|c| {
        let count = data.items.iter().filter(|i| i.collection_id == c.id).count();
        let row_match = s.is_empty() || c.name.to_lowercase().contains(&s);
        CollectionData {
            id: c.id.clone().into(), name: c.name.clone().into(),
            icon: c.icon.clone().into(), item_count: count as i32,
            row_match,
        }
    }).collect();
    slint::ModelRc::new(slint::VecModel::from(v))
}

fn load_slint_image(path: &str) -> Option<slint::Image> {
    let img = image::open(path).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(img.as_raw(), w, h);
    Some(slint::Image::from_rgba8(buf))
}

fn thumbnail_image(path: &str) -> slint::Image {
    if let Ok(img) = image::open(path) {
        let thumb = img.thumbnail(200, 200).into_rgba8();
        let (w, h) = thumb.dimensions();
        let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(thumb.as_raw(), w, h);
        return slint::Image::from_rgba8(buf);
    }
    slint::Image::default()
}

fn to_slint_items(data: &AppData, coll_id: &str, search: &str) -> slint::ModelRc<ItemData> {
    let search_lc = search.to_lowercase();
    let v: Vec<ItemData> = data.items.iter()
        .filter(|i| i.collection_id == coll_id)
        .filter(|i| {
            if search_lc.is_empty() { return true; }
            let fields_match = i.custom_fields.iter().any(|f|
                f.value.to_lowercase().contains(&search_lc) ||
                f.label.to_lowercase().contains(&search_lc)
            );
            i.name.to_lowercase().contains(&search_lc)
                || i.short_desc.to_lowercase().contains(&search_lc)
                || fields_match
        })
        .map(|i| {
            let has_photo = i.thumbnail_path.as_deref().map(|p| !p.is_empty()).unwrap_or(false);
            let thumbnail = if has_photo {
                thumbnail_image(i.thumbnail_path.as_deref().unwrap_or(""))
            } else { slint::Image::default() };
            ItemData {
                id: i.id.clone().into(), name: i.name.clone().into(),
                short_desc: i.short_desc.clone().into(),
                has_photo, thumbnail,
                collection_id: i.collection_id.clone().into(),
            }
        }).collect();
    slint::ModelRc::new(slint::VecModel::from(v))
}

fn to_slint_fields(item: &Item) -> slint::ModelRc<FieldData> {
    let v: Vec<FieldData> = item.custom_fields.iter().map(|f| FieldData {
        id: f.id.clone().into(), label: f.label.clone().into(), value: f.value.clone().into(),
    }).collect();
    slint::ModelRc::new(slint::VecModel::from(v))
}

fn to_slint_templates(data: &AppData) -> slint::ModelRc<TemplateData> {
    let v: Vec<TemplateData> = data.templates.iter().map(|t| TemplateData {
        id: t.id.clone().into(), name: t.name.clone().into(),
        field_labels: t.field_labels.join(", ").into(),
    }).collect();
    slint::ModelRc::new(slint::VecModel::from(v))
}

fn bool_model(len: usize) -> slint::ModelRc<CheckedItem> {
    let v: Vec<CheckedItem> = (0..len).map(|_| CheckedItem { checked: false }).collect();
    slint::ModelRc::new(slint::VecModel::from(v))
}

fn clear_detail(ui: &AppWindow) {
    ui.set_detail_name("".into()); ui.set_detail_desc("".into());
    ui.set_detail_has_photo(false);
    ui.set_detail_photo(slint::Image::default());
    ui.set_detail_fields(slint::ModelRc::new(slint::VecModel::from(vec![])));
}

fn load_detail(ui: &AppWindow, item: &Item) {
    ui.set_detail_name(item.name.clone().into());
    ui.set_detail_desc(item.short_desc.clone().into());
    let has_photo = item.thumbnail_path.as_deref().map(|p| !p.is_empty()).unwrap_or(false);
    ui.set_detail_has_photo(has_photo);
    if has_photo {
        if let Some(img) = load_slint_image(item.thumbnail_path.as_deref().unwrap_or("")) {
            ui.set_detail_photo(img);
        }
    } else {
        ui.set_detail_photo(slint::Image::default());
    }
    ui.set_detail_fields(to_slint_fields(item));
}

fn flush_detail(ui: &AppWindow, item: &mut Item) {
    item.name       = ui.get_detail_name().to_string();
    item.short_desc = ui.get_detail_desc().to_string();
    // Fields are saved live via custom-field-value-changed, so no flush needed here
}

fn set_status(ui: &AppWindow, msg: impl Into<slint::SharedString>) {
    ui.set_status_message(msg.into());
}

// ─── Filtered item list (same filter as UI) ───────────────────────────────────

fn filtered_items<'a>(data: &'a AppData, coll_id: &str, search: &str) -> Vec<&'a Item> {
    let search_lc = search.to_lowercase();
    data.items.iter()
        .filter(|i| i.collection_id == coll_id)
        .filter(|i| {
            if search_lc.is_empty() { return true; }
            let fields_match = i.custom_fields.iter().any(|f|
                f.value.to_lowercase().contains(&search_lc) ||
                f.label.to_lowercase().contains(&search_lc)
            );
            i.name.to_lowercase().contains(&search_lc)
                || i.short_desc.to_lowercase().contains(&search_lc)
                || fields_match
        })
        .collect()
}

// ─── Entry Point ──────────────────────────────────────────────────────────────

fn main() {
    let app_data  = load_data();
    let settings  = load_settings();
    let ui        = AppWindow::new().expect("Failed to create window");

    apply_theme(&ui, settings.dark_mode, &settings.accent_hex);
    ui.set_left_width(settings.left_panel_width);
    ui.set_mid_width(settings.mid_panel_width);
    ui.global::<Theme>().set_ui_font_size(settings.font_size);
    ui.set_collections(to_slint_collections(&app_data));
    ui.set_templates(to_slint_templates(&app_data));
    ui.set_coll_checked(bool_model(app_data.collections.len()));
    ui.set_item_checked(bool_model(0));

    let data      = Rc::new(RefCell::new(app_data));
    let cfg       = Rc::new(RefCell::new(settings));
    let sel_coll  = Rc::new(RefCell::new(Option::<String>::None));
    let sel_item  = Rc::new(RefCell::new(Option::<String>::None));
    let search    = Rc::new(RefCell::new(String::new()));
    let coll_search = Rc::new(RefCell::new(String::new()));
    let ctx_coll_idx: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
    let ctx_item_idx: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
    let icon_target: Rc<RefCell<Option<usize>>>  = Rc::new(RefCell::new(None));
    let anchor_coll: Rc<RefCell<Option<usize>>>  = Rc::new(RefCell::new(None));
    let anchor_item: Rc<RefCell<Option<usize>>>  = Rc::new(RefCell::new(None));

    macro_rules! weak { () => { ui.as_weak() }; }

    // ── select-collection (with ctrl/shift multi-select) ──────────────────────
    {
        let (ui_w, data, sel_coll, sel_item, search, coll_search, anchor_coll) =
            (weak!(), data.clone(), sel_coll.clone(), sel_item.clone(), search.clone(), coll_search.clone(), anchor_coll.clone());
        ui.on_select_collection(move |idx, ctrl, shift| {
            let ui = ui_w.unwrap();
            let d = data.borrow();
            let n = d.collections.len();
            if n == 0 { return; }
            let idx_u = idx as usize;

            let checked = ui.get_coll_checked();

            if ctrl {
                // Toggle just this one
                if let Some(row) = checked.row_data(idx_u) {
                    checked.set_row_data(idx_u, CheckedItem { checked: !row.checked });
                }
                *anchor_coll.borrow_mut() = Some(idx_u);
                let cnt = (0..checked.row_count()).filter(|i| checked.row_data(*i).map(|c| c.checked).unwrap_or(false)).count();
                ui.set_coll_multi_mode(cnt >= 1);
                // Clear single-selection highlight when multi-selecting
                ui.set_selected_collection(-1);
                return;
            }
            if shift {
                let anchor = anchor_coll.borrow().or_else(|| {
                    let s = ui.get_selected_collection();
                    if s >= 0 { Some(s as usize) } else { Some(0) }
                }).unwrap_or(0);
                *anchor_coll.borrow_mut() = Some(anchor);
                let (lo, hi) = if anchor <= idx_u { (anchor, idx_u) } else { (idx_u, anchor) };
                let q = coll_search.borrow().to_lowercase();
                for i in 0..checked.row_count() {
                    // Only select within range AND only if the row is visible under the filter
                    let visible = q.is_empty()
                        || d.collections.get(i).map(|c| c.name.to_lowercase().contains(&q)).unwrap_or(false);
                    checked.set_row_data(i, CheckedItem { checked: i >= lo && i <= hi && visible });
                }
                ui.set_coll_multi_mode(true);
                ui.set_selected_collection(-1);
                return;
            }

            // Plain click — clear all checks, single select / deselect
            for i in 0..checked.row_count() {
                checked.set_row_data(i, CheckedItem { checked: false });
            }
            ui.set_coll_multi_mode(false);
            *anchor_coll.borrow_mut() = Some(idx_u);

            if ui.get_selected_collection() == idx {
                *sel_coll.borrow_mut() = None;
                *sel_item.borrow_mut() = None;
                ui.set_selected_collection(-1);
                ui.set_selected_item(-1);
                ui.set_items(slint::ModelRc::new(slint::VecModel::from(vec![])));
                ui.set_item_checked(bool_model(0));
                ui.set_item_multi_mode(false);
                clear_detail(&ui);
                return;
            }
            if let Some(c) = d.collections.get(idx_u) {
                *sel_coll.borrow_mut() = Some(c.id.clone());
                *sel_item.borrow_mut() = None;
                let items = to_slint_items(&d, &c.id, &search.borrow());
                let cnt = items.row_count();
                ui.set_items(items);
                ui.set_item_checked(bool_model(cnt));
                ui.set_item_multi_mode(false);   // clear item multi-select from previous collection
                ui.set_selected_collection(idx);
                ui.set_selected_item(-1);
                ui.set_is_editing(false);
                clear_detail(&ui);
            }
        });
    }

    // ── select-item (with ctrl/shift multi-select) ────────────────────────────
    {
        let (ui_w, data, sel_coll, sel_item, search, anchor_item) =
            (weak!(), data.clone(), sel_coll.clone(), sel_item.clone(), search.clone(), anchor_item.clone());
        ui.on_select_item(move |idx, ctrl, shift| {
            let ui = ui_w.unwrap();
            let idx_u = idx as usize;
            let checked = ui.get_item_checked();

            if ctrl {
                if let Some(row) = checked.row_data(idx_u) {
                    checked.set_row_data(idx_u, CheckedItem { checked: !row.checked });
                }
                *anchor_item.borrow_mut() = Some(idx_u);
                let cnt = (0..checked.row_count()).filter(|i| checked.row_data(*i).map(|c| c.checked).unwrap_or(false)).count();
                ui.set_item_multi_mode(cnt >= 1);
                // Clear single-selection highlight + detail when multi-selecting
                ui.set_selected_item(-1);
                ui.set_is_editing(false);
                clear_detail(&ui);
                return;
            }
            if shift {
                let anchor = anchor_item.borrow().or_else(|| {
                    let s = ui.get_selected_item();
                    if s >= 0 { Some(s as usize) } else { Some(0) }
                }).unwrap_or(0);
                *anchor_item.borrow_mut() = Some(anchor);
                let (lo, hi) = if anchor <= idx_u { (anchor, idx_u) } else { (idx_u, anchor) };
                for i in 0..checked.row_count() {
                    checked.set_row_data(i, CheckedItem { checked: i >= lo && i <= hi });
                }
                ui.set_item_multi_mode(true);
                ui.set_selected_item(-1);
                ui.set_is_editing(false);
                clear_detail(&ui);
                return;
            }

            // Plain click
            for i in 0..checked.row_count() {
                checked.set_row_data(i, CheckedItem { checked: false });
            }
            ui.set_item_multi_mode(false);
            *anchor_item.borrow_mut() = Some(idx_u);

            if ui.get_selected_item() == idx {
                *sel_item.borrow_mut() = None;
                ui.set_selected_item(-1);
                ui.set_is_editing(false);
                clear_detail(&ui);
                return;
            }
            let d = data.borrow();
            let coll_id = sel_coll.borrow().clone().unwrap_or_default();
            let items = filtered_items(&d, &coll_id, &search.borrow());
            if let Some(item) = items.get(idx_u) {
                *sel_item.borrow_mut() = Some(item.id.clone());
                ui.set_selected_item(idx);
                ui.set_is_editing(false);
                load_detail(&ui, item);
            }
        });
    }

    // ── new-collection ────────────────────────────────────────────────────────
    {
        let (ui_w, data) = (weak!(), data.clone());
        ui.on_new_collection(move || {
            let ui = ui_w.unwrap();
            let mut d = data.borrow_mut();
            let icons = ["📁","📂","🎧","✒","📷","🎮","📚","⌚","💍","🎸"];
            let count = d.collections.len();
            let icon  = icons[count % icons.len()].to_string();
            let name  = format!("New Collection {}", count + 1);
            d.collections.push(Collection { id: Uuid::new_v4().to_string(), name, icon });
            save_data(&d);
            ui.set_collections(to_slint_collections(&d));
            ui.set_coll_checked(bool_model(d.collections.len()));
        });
    }

    // ── delete-collection ─────────────────────────────────────────────────────
    {
        let (ui_w, data, sel_coll, sel_item) =
            (weak!(), data.clone(), sel_coll.clone(), sel_item.clone());
        ui.on_delete_collection(move |idx| {
            let ui = ui_w.unwrap();
            let mut d = data.borrow_mut();
            if let Some(c) = d.collections.get(idx as usize) {
                let cid = c.id.clone();
                for item in d.items.iter().filter(|i| i.collection_id == cid) {
                    if let Some(p) = &item.thumbnail_path { std::fs::remove_file(p).ok(); }
                }
                d.items.retain(|i| i.collection_id != cid);
                d.collections.remove(idx as usize);
                *sel_coll.borrow_mut() = None;
                *sel_item.borrow_mut() = None;
                save_data(&d);
            }
            ui.set_collections(to_slint_collections(&d));
            ui.set_coll_checked(bool_model(d.collections.len()));
            ui.set_items(slint::ModelRc::new(slint::VecModel::from(vec![])));
            ui.set_item_checked(bool_model(0));
            ui.set_selected_collection(-1);
            ui.set_selected_item(-1);
            clear_detail(&ui);
        });
    }

    // ── new-item ──────────────────────────────────────────────────────────────
    {
        let (ui_w, data, sel_coll, sel_item, search) =
            (weak!(), data.clone(), sel_coll.clone(), sel_item.clone(), search.clone());
        ui.on_new_item(move || {
            let ui = ui_w.unwrap();
            let coll_id = sel_coll.borrow().clone().unwrap_or_default();
            if coll_id.is_empty() { return; }
            let mut d = data.borrow_mut();
            let new = Item {
                id: Uuid::new_v4().to_string(),
                collection_id: coll_id.clone(),
                name: "New Item".into(),
                custom_fields: default_fields(),
                ..Default::default()
            };
            let nid = new.id.clone();
            d.items.push(new);
            save_data(&d);
            let items = to_slint_items(&d, &coll_id, &search.borrow());
            let n     = items.row_count();
            let idx   = items.iter().position(|i| i.id.as_str() == nid).unwrap_or(0) as i32;
            ui.set_items(items);
            ui.set_item_checked(bool_model(n));
            *sel_item.borrow_mut() = Some(nid.clone());
            ui.set_selected_item(idx);
            ui.set_is_editing(true);
            if let Some(item) = d.items.iter().find(|i| i.id == nid) {
                load_detail(&ui, item);
            }
            ui.set_collections(to_slint_collections(&d));
        });
    }

    // ── delete-item ───────────────────────────────────────────────────────────
    {
        let (ui_w, data, sel_coll, sel_item, search) =
            (weak!(), data.clone(), sel_coll.clone(), sel_item.clone(), search.clone());
        ui.on_delete_item(move |_| {
            let ui = ui_w.unwrap();
            let item_id = sel_item.borrow().clone().unwrap_or_default();
            let coll_id = sel_coll.borrow().clone().unwrap_or_default();
            if item_id.is_empty() { return; }
            {
                let mut d = data.borrow_mut();
                if let Some(item) = d.items.iter().find(|i| i.id == item_id) {
                    if let Some(p) = &item.thumbnail_path { std::fs::remove_file(p).ok(); }
                }
                d.items.retain(|i| i.id != item_id);
                save_data(&d);
            }
            *sel_item.borrow_mut() = None;
            let d = data.borrow();
            let items = to_slint_items(&d, &coll_id, &search.borrow());
            let n = items.row_count();
            ui.set_items(items);
            ui.set_item_checked(bool_model(n));
            ui.set_selected_item(-1);
            ui.set_is_editing(false);
            clear_detail(&ui);
            ui.set_collections(to_slint_collections(&d));
        });
    }

    // ── duplicate-collection ──────────────────────────────────────────────────
    {
        let (ui_w, data) = (weak!(), data.clone());
        ui.on_duplicate_collection(move |idx| {
            let ui = ui_w.unwrap();
            let mut d = data.borrow_mut();
            if let Some(src) = d.collections.get(idx as usize).cloned() {
                let new_id = Uuid::new_v4().to_string();
                let new_coll = Collection {
                    id: new_id.clone(),
                    name: format!("{} (copy)", src.name),
                    icon: src.icon.clone(),
                };
                // Duplicate all items in this collection
                let src_items: Vec<Item> = d.items.iter()
                    .filter(|i| i.collection_id == src.id)
                    .cloned()
                    .collect();
                d.collections.push(new_coll);
                for mut item in src_items {
                    item.id = Uuid::new_v4().to_string();
                    item.collection_id = new_id.clone();
                    item.thumbnail_path = None; // don't copy photo paths
                    item.custom_fields = item.custom_fields.into_iter().map(|mut f| {
                        f.id = Uuid::new_v4().to_string(); f
                    }).collect();
                    d.items.push(item);
                }
                save_data(&d);
                ui.set_collections(to_slint_collections(&d));
                ui.set_coll_checked(bool_model(d.collections.len()));
                ui.set_status_message("".into());
            }
        });
    }

    // ── duplicate-item ────────────────────────────────────────────────────────
    {
        let (ui_w, data, sel_coll, sel_item, search) =
            (weak!(), data.clone(), sel_coll.clone(), sel_item.clone(), search.clone());
        ui.on_duplicate_item(move |_idx| {
            let ui = ui_w.unwrap();
            let item_id = sel_item.borrow().clone().unwrap_or_default();
            let coll_id  = sel_coll.borrow().clone().unwrap_or_default();
            if item_id.is_empty() { return; }
            let insert_at = {
                let d = data.borrow();
                d.items.iter().rposition(|i| i.id == item_id).map(|p| p + 1).unwrap_or_else(|| d.items.len())
            };
            let new_item = {
                let d = data.borrow();
                d.items.iter().find(|i| i.id == item_id).map(|src| {
                    let mut n = src.clone();
                    n.id = Uuid::new_v4().to_string();
                    n.name = format!("{} (copy)", src.name);
                    n.thumbnail_path = None;
                    n.custom_fields = n.custom_fields.into_iter().map(|mut f| {
                        f.id = Uuid::new_v4().to_string(); f
                    }).collect();
                    n
                })
            };
            if let Some(item) = new_item {
                let mut d = data.borrow_mut();
                d.items.insert(insert_at, item);
                save_data(&d);
                let items = to_slint_items(&d, &coll_id, &search.borrow());
                let n = items.row_count();
                ui.set_items(items);
                ui.set_item_checked(bool_model(n));
                ui.set_collections(to_slint_collections(&d));
                ui.set_status_message("".into());
            }
        });
    }

    // ── delete-selected-items ─────────────────────────────────────────────────
    {
        let (ui_w, data, sel_coll, sel_item, search) =
            (weak!(), data.clone(), sel_coll.clone(), sel_item.clone(), search.clone());
        ui.on_delete_selected_items(move || {
            let ui = ui_w.unwrap();
            let coll_id = sel_coll.borrow().clone().unwrap_or_default();
            if coll_id.is_empty() { return; }
            let checked = ui.get_item_checked();
            let mut d = data.borrow_mut();
            let to_delete: Vec<String> = {
                let items = filtered_items(&d, &coll_id, &search.borrow());
                items.iter().enumerate()
                    .filter(|(i, _)| checked.row_data(*i).map(|c| c.checked).unwrap_or(false))
                    .map(|(_, item)| item.id.clone())
                    .collect()
            };
            for id in &to_delete {
                if let Some(item) = d.items.iter().find(|i| &i.id == id) {
                    if let Some(p) = &item.thumbnail_path { std::fs::remove_file(p).ok(); }
                }
            }
            d.items.retain(|i| !to_delete.contains(&i.id));
            save_data(&d);
            *sel_item.borrow_mut() = None;
            let items = to_slint_items(&d, &coll_id, &search.borrow());
            let n = items.row_count();
            ui.set_items(items);
            ui.set_item_checked(bool_model(n));
            ui.set_selected_item(-1);
            clear_detail(&ui);
            ui.set_collections(to_slint_collections(&d));
            ui.set_item_multi_mode(false);
            ui.set_status_message("".into());
        });
    }

    // ── duplicate-selected-items ──────────────────────────────────────────────
    {
        let (ui_w, data, sel_coll, search) =
            (weak!(), data.clone(), sel_coll.clone(), search.clone());
        ui.on_duplicate_selected_items(move || {
            let ui = ui_w.unwrap();
            let coll_id = sel_coll.borrow().clone().unwrap_or_default();
            if coll_id.is_empty() { return; }
            let checked = ui.get_item_checked();
            let mut d = data.borrow_mut();
            let to_dup: Vec<Item> = {
                let items = filtered_items(&d, &coll_id, &search.borrow());
                items.iter().enumerate()
                    .filter(|(i, _)| checked.row_data(*i).map(|c| c.checked).unwrap_or(false))
                    .map(|(_, item)| (*item).clone())
                    .collect()
            };
            for mut item in to_dup {
                item.id = Uuid::new_v4().to_string();
                item.name = format!("{} (copy)", item.name);
                item.thumbnail_path = None;
                item.custom_fields = item.custom_fields.into_iter().map(|mut f| {
                    f.id = Uuid::new_v4().to_string(); f
                }).collect();
                d.items.push(item);
            }
            save_data(&d);
            let items = to_slint_items(&d, &coll_id, &search.borrow());
            let n = items.row_count();
            ui.set_items(items);
            ui.set_item_checked(bool_model(n));
            ui.set_collections(to_slint_collections(&d));
            ui.set_item_multi_mode(false);
            ui.set_status_message("".into());
        });
    }

    // ── toggle-edit / save ────────────────────────────────────────────────────
    {
        let (ui_w, data, sel_coll, sel_item, search) =
            (weak!(), data.clone(), sel_coll.clone(), sel_item.clone(), search.clone());
        ui.on_toggle_edit(move || {
            let ui = ui_w.unwrap();
            if ui.get_is_editing() {
                let item_id = sel_item.borrow().clone().unwrap_or_default();
                let coll_id = sel_coll.borrow().clone().unwrap_or_default();
                if item_id.is_empty() { ui.set_is_editing(false); return; }
                {
                    let mut d = data.borrow_mut();
                    if let Some(item) = d.items.iter_mut().find(|i| i.id == item_id) {
                        flush_detail(&ui, item);
                    }
                    save_data(&d);
                    ui.set_items(to_slint_items(&d, &coll_id, &search.borrow()));
                }
                // Reload detail from saved item so view mode shows current data
                let d = data.borrow();
                if let Some(item) = d.items.iter().find(|i| i.id == item_id) {
                    load_detail(&ui, item);
                }
                ui.set_is_editing(false);
                set_status(&ui, "Saved");
            } else {
                ui.set_is_editing(true);
                ui.set_status_message("".into());
            }
        });
    }

    // ── field-changed ─────────────────────────────────────────────────────────
    // name and desc are handled via two-way bindings; this is a no-op
    ui.on_field_changed(|_, _| {});

    // ── pick-photo ────────────────────────────────────────────────────────────
    {
        let (ui_w, data, sel_item, _sel_coll, search) =
            (weak!(), data.clone(), sel_item.clone(), sel_coll.clone(), search.clone());
        ui.on_pick_photo(move || {
            let ui = ui_w.unwrap();
            let item_id = sel_item.borrow().clone().unwrap_or_default();
            if item_id.is_empty() { return; }
            let picked = rfd::FileDialog::new()
                .set_title("Choose a photo")
                .add_filter("Images", &["png","jpg","jpeg","webp","gif"])
                .pick_file();
            if let Some(src_path) = picked {
                let ext = src_path.extension().and_then(|e| e.to_str()).unwrap_or("jpg");
                let dest = photos_dir().join(format!("{}.{}", Uuid::new_v4(), ext));
                match std::fs::copy(&src_path, &dest) {
                    Ok(_) => {
                        let path_str = dest.to_string_lossy().to_string();
                        let coll_id = {
                            let mut d = data.borrow_mut();
                            let cid = d.items.iter().find(|i| i.id == item_id)
                                .map(|i| i.collection_id.clone()).unwrap_or_default();
                            // Remove old photo
                            if let Some(item) = d.items.iter().find(|i| i.id == item_id) {
                                if let Some(old) = &item.thumbnail_path {
                                    if !old.is_empty() { std::fs::remove_file(old).ok(); }
                                }
                            }
                            if let Some(item) = d.items.iter_mut().find(|i| i.id == item_id) {
                                item.thumbnail_path = Some(path_str.clone());
                            }
                            save_data(&d);
                            cid
                        };
                        ui.set_detail_has_photo(true);
                        if let Some(img) = load_slint_image(&path_str) { ui.set_detail_photo(img); }
                        let d2 = data.borrow();
                        ui.set_items(to_slint_items(&d2, &coll_id, &search.borrow()));
                        ui.set_status_message("".into());
                    }
                    Err(e) => set_status(&ui, format!("Could not copy photo: {e}")),
                }
            }
        });
    }

    // ── add-custom-field ──────────────────────────────────────────────────────
    {
        let (ui_w, data, sel_item) = (weak!(), data.clone(), sel_item.clone());
        ui.on_add_custom_field(move || {
            let ui = ui_w.unwrap();
            let item_id = sel_item.borrow().clone().unwrap_or_default();
            if item_id.is_empty() { return; }
            let mut d = data.borrow_mut();
            if let Some(item) = d.items.iter_mut().find(|i| i.id == item_id) {
                item.custom_fields.push(CustomField {
                    id: Uuid::new_v4().to_string(),
                    label: "NEW FIELD".into(),
                    value: "".into(),
                });
                let fields = to_slint_fields(item);
                save_data(&d);
                ui.set_detail_fields(fields);
            }
        });
    }

    // ── delete-custom-field ───────────────────────────────────────────────────
    {
        let (ui_w, data, sel_item) = (weak!(), data.clone(), sel_item.clone());
        ui.on_delete_custom_field(move |field_id| {
            let ui = ui_w.unwrap();
            let item_id = sel_item.borrow().clone().unwrap_or_default();
            if item_id.is_empty() { return; }
            let mut d = data.borrow_mut();
            if let Some(item) = d.items.iter_mut().find(|i| i.id == item_id) {
                item.custom_fields.retain(|f| f.id != field_id.as_str());
                let fields = to_slint_fields(item);
                save_data(&d);
                ui.set_detail_fields(fields);
            }
        });
    }



    // ── custom-field-label-changed ────────────────────────────────────────────
    // NOTE: We do NOT uppercase here to allow free typing. Uppercasing on every
    // keystroke caused the one-letter-at-a-time bug by triggering a model update
    // that reset focus. The label is stored as typed.
    {
        let (ui_w, data, sel_item) = (weak!(), data.clone(), sel_item.clone());
        ui.on_custom_field_label_changed(move |field_id, new_label| {
            let _ui = ui_w.unwrap();
            let item_id = sel_item.borrow().clone().unwrap_or_default();
            if item_id.is_empty() { return; }
            let mut d = data.borrow_mut();
            if let Some(item) = d.items.iter_mut().find(|i| i.id == item_id) {
                if let Some(field) = item.custom_fields.iter_mut().find(|f| f.id == field_id.as_str()) {
                    field.label = new_label.to_string();
                }
                // Do NOT call set_detail_fields here — that resets TextInput focus
                save_data(&d);
            }
        });
    }

    // ── custom-field-value-changed ────────────────────────────────────────────
    {
        let (ui_w, data, sel_item) = (weak!(), data.clone(), sel_item.clone());
        ui.on_custom_field_value_changed(move |field_id, new_value| {
            let _ui = ui_w.unwrap();
            let item_id = sel_item.borrow().clone().unwrap_or_default();
            if item_id.is_empty() { return; }
            let mut d = data.borrow_mut();
            if let Some(item) = d.items.iter_mut().find(|i| i.id == item_id) {
                if let Some(field) = item.custom_fields.iter_mut().find(|f| f.id == field_id.as_str()) {
                    field.value = new_value.to_string();
                }
                save_data(&d);
            }
        });
    }

    // ── save-template-named ───────────────────────────────────────────────────
    {
        let (ui_w, data, sel_item) = (weak!(), data.clone(), sel_item.clone());
        ui.on_save_template_named(move |name| {
            let ui = ui_w.unwrap();
            let item_id = sel_item.borrow().clone().unwrap_or_default();
            if item_id.is_empty() { return; }
            let mut d = data.borrow_mut();
            let labels: Vec<String> = d.items.iter()
                .find(|i| i.id == item_id)
                .map(|i| i.custom_fields.iter().map(|f| f.label.clone()).collect())
                .unwrap_or_default();
            if labels.is_empty() { return; }
            let tname = if name.trim().is_empty() {
                format!("Template {}", d.templates.len() + 1)
            } else { name.to_string() };
            d.templates.push(Template { id: Uuid::new_v4().to_string(), name: tname, field_labels: labels });
            save_data(&d);
            ui.set_templates(to_slint_templates(&d));
            set_status(&ui, "Saved template");
        });
    }

    // ── rename-collection ─────────────────────────────────────────────────────
    {
        let (ui_w, data) = (weak!(), data.clone());
        ui.on_rename_collection(move |idx, new_name| {
            let ui = ui_w.unwrap();
            if new_name.trim().is_empty() { return; }
            let mut d = data.borrow_mut();
            if let Some(c) = d.collections.get_mut(idx as usize) {
                c.name = new_name.to_string();
            }
            let n = d.collections.len();
            save_data(&d);
            ui.set_collections(to_slint_collections(&d));
            ui.set_coll_checked(bool_model(n));
            ui.set_coll_multi_mode(false);
        });
    }

    // ── rename-item ───────────────────────────────────────────────────────────
    {
        let (ui_w, data, sel_coll, sel_item, search) =
            (weak!(), data.clone(), sel_coll.clone(), sel_item.clone(), search.clone());
        ui.on_rename_item(move |idx, new_name| {
            let ui = ui_w.unwrap();
            if new_name.trim().is_empty() { return; }
            let coll_id = sel_coll.borrow().clone().unwrap_or_default();
            let target_id = {
                let d = data.borrow();
                filtered_items(&d, &coll_id, &search.borrow())
                    .get(idx as usize).map(|i| i.id.clone())
            };
            if let Some(id) = target_id {
                {
                    let mut d = data.borrow_mut();
                    if let Some(item) = d.items.iter_mut().find(|i| i.id == id) {
                        item.name = new_name.to_string();
                    }
                    save_data(&d);
                }
                let d = data.borrow();
                let items = to_slint_items(&d, &coll_id, &search.borrow());
                let n = items.row_count();
                ui.set_items(items);
                ui.set_item_checked(bool_model(n));
                ui.set_item_multi_mode(false);
                if sel_item.borrow().as_deref() == Some(id.as_str()) {
                    ui.set_detail_name(new_name.clone());
                }
            }
        });
    }

    // ── apply-template (replaces current custom fields) ───────────────────────
    {
        let (ui_w, data, sel_item) = (weak!(), data.clone(), sel_item.clone());
        ui.on_apply_template(move |tmpl_id| {
            let ui = ui_w.unwrap();
            let item_id = sel_item.borrow().clone().unwrap_or_default();
            if item_id.is_empty() { return; }
            let mut d = data.borrow_mut();
            let labels: Vec<String> = d.templates.iter()
                .find(|t| t.id == tmpl_id.as_str())
                .map(|t| t.field_labels.clone())
                .unwrap_or_default();
            if let Some(item) = d.items.iter_mut().find(|i| i.id == item_id) {
                // Replace current fields with the template's fields
                item.custom_fields.clear();
                for label in labels {
                    item.custom_fields.push(CustomField {
                        id: Uuid::new_v4().to_string(), label, value: "".into(),
                    });
                }
                let fields = to_slint_fields(item);
                save_data(&d);
                ui.set_detail_fields(fields);
                set_status(&ui, "Loaded template");
            }
        });
    }

    // ── delete-template ───────────────────────────────────────────────────────
    {
        let (ui_w, data) = (weak!(), data.clone());
        ui.on_delete_template(move |tmpl_id| {
            let ui = ui_w.unwrap();
            let mut d = data.borrow_mut();
            d.templates.retain(|t| t.id != tmpl_id.as_str());
            save_data(&d);
            ui.set_templates(to_slint_templates(&d));
        });
    }

    // ── rename-template ───────────────────────────────────────────────────────
    {
        let (ui_w, data) = (weak!(), data.clone());
        ui.on_rename_template(move |tmpl_id, new_name| {
            let ui = ui_w.unwrap();
            let mut d = data.borrow_mut();
            if let Some(t) = d.templates.iter_mut().find(|t| t.id == tmpl_id.as_str()) {
                t.name = new_name.to_string();
            }
            save_data(&d);
            ui.set_templates(to_slint_templates(&d));
        });
    }



    // ── search-changed ────────────────────────────────────────────────────────
    {
        let (ui_w, data, sel_coll, sel_item, search) =
            (weak!(), data.clone(), sel_coll.clone(), sel_item.clone(), search.clone());
        ui.on_search_changed(move |q| {
            let ui = ui_w.unwrap();
            *search.borrow_mut() = q.to_string();
            let coll_id = sel_coll.borrow().clone().unwrap_or_default();
            if coll_id.is_empty() { return; }
            let d = data.borrow();
            let items = to_slint_items(&d, &coll_id, &q);
            let n = items.row_count();
            ui.set_items(items);
            ui.set_item_checked(bool_model(n));
            *sel_item.borrow_mut() = None;
            ui.set_selected_item(-1);
            clear_detail(&ui);
        });
    }

    // ── search-collections (filter left panel by name; indices stay stable) ───
    {
        let (ui_w, data, coll_search) = (weak!(), data.clone(), coll_search.clone());
        ui.on_search_collections(move |q| {
            let ui = ui_w.unwrap();
            *coll_search.borrow_mut() = q.to_string();
            let d = data.borrow();
            ui.set_collections(collections_model(&d, &q));
        });
    }

    // ── collection-icon-clicked ───────────────────────────────────────────────
    {
        let (ui_w, icon_target) = (weak!(), icon_target.clone());
        ui.on_collection_icon_clicked(move |idx| {
            let ui = ui_w.unwrap();
            *icon_target.borrow_mut() = Some(idx as usize);
            ui.set_show_icon_picker(true);
        });
    }

    // ── icon-picked ───────────────────────────────────────────────────────────
    {
        let (ui_w, data, icon_target) = (weak!(), data.clone(), icon_target.clone());
        ui.on_icon_picked(move |ico| {
            let ui = ui_w.unwrap();
            if let Some(idx) = *icon_target.borrow() {
                let mut d = data.borrow_mut();
                if let Some(c) = d.collections.get_mut(idx) {
                    c.icon = ico.to_string();
                }
                save_data(&d);
                ui.set_collections(to_slint_collections(&d));
            }
        });
    }


    // ── coll-right-clicked ────────────────────────────────────────────────────
    {
        let (ui_w, ctx_coll_idx) = (weak!(), ctx_coll_idx.clone());
        ui.on_coll_right_clicked(move |idx, mx, my| {
            let ui = ui_w.unwrap();
            *ctx_coll_idx.borrow_mut() = Some(idx as usize);
            ui.set_ctx_target(idx);
            ui.set_ctx_is_collection(true);
            let cnt = ui.get_coll_checked().iter().filter(|c| c.checked).count();
            ui.set_ctx_multi(cnt >= 2);
            ui.set_ctx_x(mx);
            ui.set_ctx_y(my);
            ui.set_show_context_menu(true);
        });
    }

    // ── item-right-clicked ────────────────────────────────────────────────────
    {
        let (ui_w, ctx_item_idx) = (weak!(), ctx_item_idx.clone());
        ui.on_item_right_clicked(move |idx, mx, my| {
            let ui = ui_w.unwrap();
            *ctx_item_idx.borrow_mut() = Some(idx as usize);
            ui.set_ctx_target(idx);
            ui.set_ctx_is_collection(false);
            let cnt = ui.get_item_checked().iter().filter(|c| c.checked).count();
            ui.set_ctx_multi(cnt >= 2);
            ui.set_ctx_x(mx);
            ui.set_ctx_y(my);
            ui.set_show_context_menu(true);
        });
    }

    // ── delete-selected-collections ───────────────────────────────────────────
    {
        let (ui_w, data, sel_coll, sel_item) =
            (weak!(), data.clone(), sel_coll.clone(), sel_item.clone());
        ui.on_delete_selected_collections(move || {
            let ui = ui_w.unwrap();
            let checked = ui.get_coll_checked();
            let mut d = data.borrow_mut();
            let to_delete: Vec<String> = (0..checked.row_count())
                .filter(|i| checked.row_data(*i).map(|c| c.checked).unwrap_or(false))
                .filter_map(|i| d.collections.get(i).map(|c| c.id.clone()))
                .collect();
            // Delete photos of items in those collections
            for cid in &to_delete {
                for item in d.items.iter().filter(|i| &i.collection_id == cid) {
                    if let Some(p) = &item.thumbnail_path { std::fs::remove_file(p).ok(); }
                }
            }
            d.items.retain(|i| !to_delete.contains(&i.collection_id));
            d.collections.retain(|c| !to_delete.contains(&c.id));
            save_data(&d);
            *sel_coll.borrow_mut() = None;
            *sel_item.borrow_mut() = None;
            ui.set_collections(to_slint_collections(&d));
            ui.set_coll_checked(bool_model(d.collections.len()));
            ui.set_items(slint::ModelRc::new(slint::VecModel::from(vec![])));
            ui.set_item_checked(bool_model(0));
            ui.set_selected_collection(-1);
            ui.set_selected_item(-1);
            ui.set_coll_multi_mode(false);
            clear_detail(&ui);
            ui.set_status_message("".into());
        });
    }

    // ── duplicate-selected-collections ────────────────────────────────────────
    {
        let (ui_w, data) = (weak!(), data.clone());
        ui.on_duplicate_selected_collections(move || {
            let ui = ui_w.unwrap();
            let checked = ui.get_coll_checked();
            let mut d = data.borrow_mut();
            let sel_ids: Vec<String> = (0..checked.row_count())
                .filter(|i| checked.row_data(*i).map(|c| c.checked).unwrap_or(false))
                .filter_map(|i| d.collections.get(i).map(|c| c.id.clone()))
                .collect();
            for cid in sel_ids {
                if let Some(src) = d.collections.iter().find(|c| c.id == cid).cloned() {
                    let new_id = Uuid::new_v4().to_string();
                    let pos = d.collections.iter().position(|c| c.id == cid).unwrap_or(d.collections.len()-1);
                    d.collections.insert(pos + 1, Collection {
                        id: new_id.clone(),
                        name: format!("{} (copy)", src.name),
                        icon: src.icon.clone(),
                    });
                    let src_items: Vec<Item> = d.items.iter().filter(|i| i.collection_id == cid).cloned().collect();
                    for mut it in src_items {
                        it.id = Uuid::new_v4().to_string();
                        it.collection_id = new_id.clone();
                        it.thumbnail_path = None;
                        it.custom_fields = it.custom_fields.into_iter().map(|mut f| { f.id = Uuid::new_v4().to_string(); f }).collect();
                        d.items.push(it);
                    }
                }
            }
            save_data(&d);
            ui.set_collections(to_slint_collections(&d));
            ui.set_coll_checked(bool_model(d.collections.len()));
            ui.set_coll_multi_mode(false);
            ui.set_status_message("".into());
        });
    }

    // ── clear-selection (button + Esc) ────────────────────────────────────────
    {
        let (ui_w, data, sel_coll) = (weak!(), data.clone(), sel_coll.clone());
        ui.on_clear_selection(move || {
            let ui = ui_w.unwrap();
            let d = data.borrow();
            ui.set_coll_checked(bool_model(d.collections.len()));
            let coll_id = sel_coll.borrow().clone().unwrap_or_default();
            let n = d.items.iter().filter(|i| i.collection_id == coll_id).count();
            ui.set_item_checked(bool_model(n));
            ui.set_coll_multi_mode(false);
            ui.set_item_multi_mode(false);
        });
    }

    // ── toggle-coll-check ─────────────────────────────────────────────────────
    {
        let ui_w = weak!();
        ui.on_toggle_coll_check(move |idx| {
            let ui = ui_w.unwrap();
            let checked = ui.get_coll_checked();
            if let Some(row) = checked.row_data(idx as usize) {
                checked.set_row_data(idx as usize, CheckedItem { checked: !row.checked });
            }
        });
    }

    // ── toggle-item-check ─────────────────────────────────────────────────────
    {
        let ui_w = weak!();
        ui.on_toggle_item_check(move |idx| {
            let ui = ui_w.unwrap();
            let checked = ui.get_item_checked();
            if let Some(row) = checked.row_data(idx as usize) {
                checked.set_row_data(idx as usize, CheckedItem { checked: !row.checked });
            }
        });
    }

    // ── Panel resize ──────────────────────────────────────────────────────────
    {
        let cfg = cfg.clone();
        ui.on_resize_left(move |w| {
            cfg.borrow_mut().left_panel_width = w;
            let s = cfg.borrow().clone();
            save_settings(&s);
        });
    }
    {
        let cfg = cfg.clone();
        ui.on_resize_mid(move |w| {
            cfg.borrow_mut().mid_panel_width = w;
            let s = cfg.borrow().clone();
            save_settings(&s);
        });
    }

    // ── Toggle dark/light ─────────────────────────────────────────────────────
    {
        let (ui_w, cfg) = (weak!(), cfg.clone());
        ui.on_toggle_dark_mode(move || {
            let ui = ui_w.unwrap();
            let mut s = cfg.borrow_mut();
            s.dark_mode = !s.dark_mode;
            let (dark, accent) = (s.dark_mode, s.accent_hex.clone());
            apply_theme(&ui, dark, &accent);
            save_settings(&s);
        });
    }

    // ── Set accent ────────────────────────────────────────────────────────────
    {
        let (ui_w, cfg) = (weak!(), cfg.clone());
        ui.on_set_accent(move |c| {
            let ui = ui_w.unwrap();
            let mut s = cfg.borrow_mut();
            s.accent_hex = color_to_hex(c);
            let (dark, accent) = (s.dark_mode, s.accent_hex.clone());
            apply_theme(&ui, dark, &accent);
            save_settings(&s);
        });
    }

    // ── Set font size ─────────────────────────────────────────────────────────
    {
        let (ui_w, cfg) = (weak!(), cfg.clone());
        ui.on_set_font_size(move |size| {
            let ui = ui_w.unwrap();
            ui.global::<Theme>().set_ui_font_size(size);
            let mut s = cfg.borrow_mut();
            s.font_size = size;
            save_settings(&s);
        });
    }

    // ── Export ────────────────────────────────────────────────────────────────
    {
        let (ui_w, data) = (weak!(), data.clone());
        ui.on_export_data(move || {
            let ui = ui_w.unwrap();
            let default = dirs::document_dir()
                .or_else(|| dirs::home_dir())
                .unwrap_or_else(|| PathBuf::from("."))
                .join("collector-export.json");
            let path = rfd::FileDialog::new()
                .set_title("Export collection data")
                .set_file_name("collector-export.json")
                .add_filter("JSON", &["json"])
                .save_file()
                .unwrap_or(default);
            let d = data.borrow();
            match serde_json::to_string_pretty(&*d) {
                Ok(json) => match std::fs::write(&path, json) {
                    Ok(_)  => ui.set_status_message("".into()),
                    Err(e) => set_status(&ui, format!("Export failed: {e}")),
                },
                Err(e) => set_status(&ui, format!("Serialise error: {e}")),
            }
        });
    }

    // ── Import ────────────────────────────────────────────────────────────────
    {
        let (ui_w, data, sel_coll, sel_item) =
            (weak!(), data.clone(), sel_coll.clone(), sel_item.clone());
        ui.on_import_data(move || {
            let ui = ui_w.unwrap();
            let picked = rfd::FileDialog::new()
                .set_title("Import collection data")
                .add_filter("JSON", &["json"])
                .pick_file();
            let path = match picked {
                Some(p) => p,
                None => { ui.set_status_message("".into()); return; }
            };
            match std::fs::read_to_string(&path) {
                Ok(contents) => match serde_json::from_str::<AppData>(&contents) {
                    Ok(imported) => {
                        let mut d = data.borrow_mut();
                        let ex_colls: std::collections::HashSet<_> =
                            d.collections.iter().map(|c| c.id.clone()).collect();
                        let ex_items: std::collections::HashSet<_> =
                            d.items.iter().map(|i| i.id.clone()).collect();
                        // (counts not shown; status bar removed)
                        for c in imported.collections { if !ex_colls.contains(&c.id) { d.collections.push(c); } }
                        for i in imported.items       { if !ex_items.contains(&i.id) { d.items.push(i); } }
                        save_data(&d);
                        *sel_coll.borrow_mut() = None;
                        *sel_item.borrow_mut() = None;
                        ui.set_collections(to_slint_collections(&d));
                        ui.set_coll_checked(bool_model(d.collections.len()));
                        ui.set_items(slint::ModelRc::new(slint::VecModel::from(vec![])));
                        ui.set_item_checked(bool_model(0));
                        ui.set_selected_collection(-1);
                        ui.set_selected_item(-1);
                        clear_detail(&ui);
                        ui.set_status_message("".into());
                    }
                    Err(e) => set_status(&ui, format!("Parse error: {e}")),
                },
                Err(e) => set_status(&ui, format!("Could not read: {e}")),
            }
        });
    }

    // ── Open data folder ──────────────────────────────────────────────────────
    {
        let ui_w = weak!();
        ui.on_open_data_folder(move || {
            let ui = ui_w.unwrap();
            let dir = app_dir();
            // Use Windows Explorer / macOS Finder / xdg-open
            #[cfg(target_os = "windows")]
            std::process::Command::new("explorer").arg(&dir).spawn().ok();
            #[cfg(target_os = "macos")]
            std::process::Command::new("open").arg(&dir).spawn().ok();
            #[cfg(target_os = "linux")]
            std::process::Command::new("xdg-open").arg(&dir).spawn().ok();
            ui.set_status_message("".into());
        });
    }

    ui.run().expect("Event loop failed");
}
