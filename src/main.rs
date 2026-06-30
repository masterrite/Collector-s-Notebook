// src/main.rs — Collector's Notebook, iced 0.14 port.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod model;
mod theme;
mod image_util;

use iced::widget::{
    button, column, container, image as image_widget, mouse_area, pane_grid, pick_list, row,
    scrollable, stack, text, text_editor, text_input,
};
use iced::widget::text_editor::{Action as EdAction, Content as EdContent};
use iced::{Color, Element, Fill, Length, Shrink, Task, Theme as IcedTheme};

use model::*;
use theme::{build_palette, color_to_hex, hex, Palette};

// Bundled fonts. Provide these files (see assets/README). Noto Color Emoji gives
// full-color glyphs that never clip because we always pair a font size with a
// matching line_height (see helpers `body`, `emoji`, etc.).
//
// The CJK family ("Noto Sans CJK SC") is provided as two faces — Regular and
// Bold — used for all user-entered input text. Both share the same family name
// and differ only by weight, so `CJK` selects Regular and `CJK_BOLD` selects the
// Bold face of the same family.
const UI_FONT: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");
const CJK_FONT_REGULAR: &[u8] = include_bytes!("../assets/fonts/NotoSansCJK-Regular-subset.otf");
const CJK_FONT_BOLD: &[u8] = include_bytes!("../assets/fonts/NotoSansCJK-Bold-subset.otf");
const EMOJI_FONT: &[u8] = include_bytes!("../assets/fonts/NotoColorEmoji.ttf");

const EMOJI: iced::Font = iced::Font::with_name("Noto Color Emoji");
const CJK: iced::Font = iced::Font::with_name("Noto Sans CJK SC");
const CJK_BOLD: iced::Font = iced::Font {
    weight: iced::font::Weight::Bold,
    ..iced::Font::with_name("Noto Sans CJK SC")
};

// Window/taskbar icon, embedded at compile time. Using include_bytes! means the
// path is resolved relative to THIS source file at build time, so it works no
// matter what the runtime working directory is (the previous runtime from_file
// approach failed because relative paths resolved against the .exe's launch dir).
const APP_ICON: &[u8] = include_bytes!("../assets/icons/Collectors-Notebook.png");
// Logo shown at the bottom of the Settings panel. Embedded so it always loads.
const APP_LOGO: &[u8] = include_bytes!("../assets/logo.png");
// Build the logo image handle ONCE. Rebuilding it every view() (which happens
// on every mouse move) makes the renderer reload the texture and flicker.
static LOGO_HANDLE: std::sync::LazyLock<image_widget::Handle> =
    std::sync::LazyLock::new(|| image_widget::Handle::from_bytes(APP_LOGO.to_vec()));
// Stable scrollable identities so scroll offset survives view rebuilds (e.g.
// when a context-menu overlay opens on right-click). scrollable::Id isn't
// exported in this build, so we use the generic advanced widget Id, which
// scrollable's .id() accepts via Into<widget::Id>.
static COLL_SCROLL: std::sync::LazyLock<iced::advanced::widget::Id> =
    std::sync::LazyLock::new(iced::advanced::widget::Id::unique);
static ITEM_SCROLL: std::sync::LazyLock<iced::advanced::widget::Id> =
    std::sync::LazyLock::new(iced::advanced::widget::Id::unique);

fn main() -> iced::Result {
    // None lets it guess the format from the PNG header — reliable and avoids
    // pinning an `image::ImageFormat` path that can drift between crate versions.
    let icon = iced::window::icon::from_file_data(APP_ICON, None).ok();

    let window = iced::window::Settings {
        size: iced::Size::new(1200.0, 760.0),
        icon,
        ..Default::default()
    };

    iced::application(App::new, App::update, App::view)
        .title("Collector's Notebook")
        .theme(App::theme)
        .subscription(App::subscription)
        .font(UI_FONT)
        .font(CJK_FONT_REGULAR)
        .font(CJK_FONT_BOLD)
        .font(EMOJI_FONT)
        .window(window)
        .run()
}

// Which of the three resizable panes a pane_grid cell represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneId {
    Left,
    Mid,
    Right,
}

/// Build the three-pane layout from the current ratios. Nested vertical splits:
/// [Left | [Mid | Right]]. Outer ratio = left's share of the whole; inner ratio
/// = mid's share of the remaining space. Ratios are clamped to the allowed caps.
fn build_panes(settings: &Settings) -> pane_grid::State<PaneId> {
    pane_grid::State::with_configuration(
        pane_grid::Configuration::Split {
            axis: pane_grid::Axis::Vertical,
            ratio: settings.left_ratio.clamp(0.08, 0.33),
            a: Box::new(pane_grid::Configuration::Pane(PaneId::Left)),
            b: Box::new(pane_grid::Configuration::Split {
                axis: pane_grid::Axis::Vertical,
                ratio: settings.mid_ratio.clamp(0.12, 0.7),
                a: Box::new(pane_grid::Configuration::Pane(PaneId::Mid)),
                b: Box::new(pane_grid::Configuration::Pane(PaneId::Right)),
            }),
        },
    )
}

// ─── Which overlay (if any) is open ─────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Overlay {
    None,
    Settings,
    IconPicker,                 // target collection index stored in `icon_target`
    TemplatePicker,
    Lightbox,
    NameInput,                  // rename coll/item or save-template
    ContextMenu,                // collection or item, see ctx fields
    Enlarge,                    // big editable view of one detail field
    DataRecovered,              // startup notice: prior data.json was corrupt
}

// Which detail editor the Enlarge overlay is editing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum EnlargeTarget {
    Name,
    Desc,
    Field(usize),
}

#[derive(Debug, Clone)]
enum NamePurpose {
    RenameColl(usize),
    RenameItem(usize),          // filtered index
    SaveTemplate,
}

// ─── Per-field editors for the detail pane ──────────────────────────────────
// Every text field is a scrollable multiline editor (text_editor::Content),
// satisfying "all text fields multiline scrollable, no word limit".

struct DetailEditors {
    name: EdContent,
    desc: EdContent,
    year: EdContent,
    month: EdContent,
    day: EdContent,
    fields: Vec<(String, EdContent, EdContent)>, // (field_id, label_editor, value_editor)
}

impl DetailEditors {
    fn empty() -> Self {
        Self {
            name: EdContent::new(),
            desc: EdContent::new(),
            year: EdContent::new(),
            month: EdContent::new(),
            day: EdContent::new(),
            fields: Vec::new(),
        }
    }
    fn from_item(item: &Item) -> Self {
        let (y, m, d) = split_date(&item.acquired_date);
        Self {
            name: EdContent::with_text(&item.name),
            desc: EdContent::with_text(&item.short_desc),
            year: EdContent::with_text(&y),
            month: EdContent::with_text(&m),
            day: EdContent::with_text(&d),
            fields: item.custom_fields.iter()
                .map(|f| (f.id.clone(), EdContent::with_text(&f.label), EdContent::with_text(&f.value)))
                .collect(),
        }
    }
}

// ─── Application state ──────────────────────────────────────────────────────

struct App {
    data: AppData,
    settings: Settings,
    palette: Palette,

    // Resizable three-pane layout (collections | items | detail).
    panes: pane_grid::State<PaneId>,

    sel_coll: Option<String>,
    sel_item: Option<String>,
    item_search: String,
    coll_search: String,

    // multi-select
    coll_checked: Vec<bool>,    // parallel to data.collections after sort
    item_checked: Vec<bool>,    // parallel to current filtered item view
    coll_multi: bool,
    item_multi: bool,
    anchor_coll: Option<usize>,
    anchor_item: Option<usize>,

    is_editing: bool,
    editors: DetailEditors,
    status: String,
    // Set at startup if data.json existed but couldn't be parsed; the recovery
    // overlay shows the user where the salvageable backup copy was written.
    corrupt_backup: Option<std::path::PathBuf>,

    overlay: Overlay,
    icon_target: Option<usize>,

    // name-input modal
    name_value: EdContent,
    name_title: String,
    name_purpose: Option<NamePurpose>,

    // context menu
    ctx_is_collection: bool,
    ctx_multi: bool,
    ctx_target: usize,
    // Stable id of the right-clicked row, captured at menu-open time. The
    // positional `ctx_target` can go stale if the list reorders between the
    // right-click and the chosen action; the Ctx* handlers re-resolve this id
    // to a fresh index just before dispatching.
    ctx_target_id: Option<String>,
    ctx_pos: iced::Point,   // where the menu should anchor (cursor at right-click)

    // lightbox
    lightbox_handle: Option<image_widget::Handle>,
    lightbox_index: usize,
    lightbox_count: usize,

    // template rename inline state
    template_rename: Option<(String, EdContent)>, // (template_id, editor)

    // live keyboard modifier state (for ctrl/shift-click multi-select)
    modifiers: iced::keyboard::Modifiers,
    // live cursor position, tracked so the context menu can open at the cursor
    cursor: iced::Point,
    // which detail field the Enlarge overlay is editing (if any)
    enlarge_target: Option<EnlargeTarget>,
    // Hovered row indices for hover highlighting (mouse_area can't style itself).
    hover_coll: Option<usize>,
    hover_item: Option<usize>,
    // True while the cursor is over the main detail photo, so a hover prompt can
    // be overlaid ("add more photos" in edit mode, "click to enlarge" otherwise).
    hover_main_photo: bool,
    // Live window width/height, tracked so panel-relative truncation budgets
    // and overlay positioning are accurate at any window size (not a fixed
    // nominal).
    window_w: f32,
    window_h: f32,
    // Cache of decoded thumbnail handles, keyed by stored filename, so each
    // thumbnail is decoded once instead of every view() frame. RefCell because
    // view(&self) is immutable but needs to populate the cache lazily.
    thumb_cache: std::cell::RefCell<std::collections::HashMap<String, image_widget::Handle>>,
}

// ─── Messages ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Message {
    // selection
    SelectCollection(usize),
    SelectItem(usize),
    ToggleCollCheck(usize),
    ToggleItemCheck(usize),
    ClearSelection,
    ModifiersChanged(iced::keyboard::Modifiers),
    EscapePressed,
    CursorMoved(iced::Point),
    WindowResized(f32, f32),

    // collection CRUD
    NewCollection,
    DeleteCollection(usize),
    DuplicateCollection(usize),
    DeleteSelectedCollections,
    DuplicateSelectedCollections,

    // item CRUD
    NewItem,
    DeleteItem,
    DuplicateItem,
    DeleteSelectedItems,
    DuplicateSelectedItems,

    // editing
    ToggleEdit,
    NameEdited(EdAction),
    DescEdited(EdAction),
    YearEdited(EdAction),
    MonthEdited(EdAction),
    DayEdited(EdAction),
    FieldLabelEdited(usize, EdAction),
    FieldValueEdited(usize, EdAction),
    AddCustomField,
    DeleteCustomField(String),
    FocusDetail(usize),  // Tab/Shift+Tab: focus a detail editor by tab-order index
    SaveShortcut,        // Ctrl+S / Cmd+S: save edited fields, stay in edit mode

    // search
    ItemSearchChanged(String),
    CollSearchChanged(String),
    ClearItemSearch,
    ClearCollSearch,

    // sort
    SetCollSort(SortMode),
    SetItemSort(SortMode),

    // photos
    PickPhotos,
    PhotosPicked(Vec<String>),  // already copied into store
    RemovePhoto(usize),
    SetMainPhoto(usize),
    OpenLightbox(usize),
    LightboxPrev,
    LightboxNext,
    LightboxArrowPrev,
    LightboxArrowNext,

    // templates
    OpenTemplatePicker,
    ApplyTemplate(String),
    DeleteTemplate(String),
    StartTemplateRename(String),
    TemplateRenameEdited(EdAction),
    CommitTemplateRename,

    // icon
    OpenIconPicker(usize),
    IconPicked(String),

    // name modal
    OpenRenameCollection(usize),
    OpenRenameItem(usize),
    OpenSaveTemplate,
    OpenEnlarge(EnlargeTarget),
    HoverColl(Option<usize>),
    HoverItem(Option<usize>),
    HoverMainPhoto(bool),
    NameValueEdited(EdAction),
    NameAccepted,

    // context menu
    CollRightClicked(usize),
    ItemRightClicked(usize),
    CtxRename,
    CtxPrimary,
    CtxDanger,

    // settings
    OpenSettings,
    SetDarkMode(bool),
    SetAccent(Color),
    FontInc,
    FontDec,
    ExportData,
    ImportData,
    ImportLoaded(Option<AppData>),
    OpenDataFolder,
    ResetPanels,

    CloseOverlay,
    Noop,
    PaneResized(pane_grid::ResizeEvent),
}

// ─── Construction ───────────────────────────────────────────────────────────

impl App {
    fn new() -> (Self, Task<Message>) {
        let (mut data, corrupt_backup) = load_data_reporting();
        let settings = load_settings();
        sort_collections(&mut data, settings.coll_sort);
        let palette = build_palette(settings.dark_mode, &settings.accent_hex);
        let coll_checked = vec![false; data.collections.len()];

        let panes = build_panes(&settings);

        // If the previous data file was corrupt, greet the user with a notice
        // rather than letting them assume their collection silently vanished.
        let overlay = if corrupt_backup.is_some() {
            Overlay::DataRecovered
        } else {
            Overlay::None
        };

        let app = Self {
            coll_checked,
            item_checked: Vec::new(),
            data,
            settings,
            palette,
            panes,
            sel_coll: None,
            sel_item: None,
            item_search: String::new(),
            coll_search: String::new(),
            coll_multi: false,
            item_multi: false,
            anchor_coll: None,
            anchor_item: None,
            is_editing: false,
            editors: DetailEditors::empty(),
            status: String::new(),
            corrupt_backup,
            overlay,
            icon_target: None,
            name_value: EdContent::new(),
            name_title: String::new(),
            name_purpose: None,
            ctx_is_collection: false,
            ctx_multi: false,
            ctx_target: 0,
            ctx_target_id: None,
            ctx_pos: iced::Point::ORIGIN,
            lightbox_handle: None,
            lightbox_index: 0,
            lightbox_count: 0,
            template_rename: None,
            modifiers: iced::keyboard::Modifiers::default(),
            cursor: iced::Point::ORIGIN,
            enlarge_target: None,
            hover_coll: None,
            hover_item: None,
            hover_main_photo: false,
            window_w: 1200.0,
            window_h: 760.0,
            thumb_cache: std::cell::RefCell::new(std::collections::HashMap::new()),
        };
        (app, Task::none())
    }

    fn theme(&self) -> IcedTheme {
        // We render colors manually from `self.palette`; use a neutral base theme.
        if self.settings.dark_mode { IcedTheme::Dark } else { IcedTheme::Light }
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        use iced::event::{self, Event};
        use iced::keyboard::{self, key::Named, Key};
        use iced::mouse;
        // In iced 0.14 the per-key subscriptions were unified; we listen to all
        // events and pick out modifier changes (for ctrl/shift-click), Escape,
        // and cursor moves (so the context menu can open at the cursor).
        event::listen_with(|evt, _status, _window| match evt {
            Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                Some(Message::ModifiersChanged(m))
            }
            // Fire on Escape regardless of capture status. A focused search
            // text_input captures the first Escape (to unfocus), which used to
            // swallow it; handling both Ignored and Captured makes a single
            // Escape clear the search.
            Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(Named::Escape), .. }) => {
                Some(Message::EscapePressed)
            }
            // Left/Right arrows navigate the lightbox (gated in update so they
            // do nothing when the lightbox isn't open).
            Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(Named::ArrowLeft), .. }) => {
                Some(Message::LightboxArrowPrev)
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(Named::ArrowRight), .. }) => {
                Some(Message::LightboxArrowNext)
            }
            // Ctrl+S / Cmd+S saves the in-progress edits (handled in update,
            // which no-ops when not editing). `command()` is Ctrl on
            // Windows/Linux and Cmd on macOS. Seen even while a text editor is
            // focused because we listen regardless of capture status.
            Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Character(c), modifiers, .. })
                if modifiers.command() && c.as_str().eq_ignore_ascii_case("s") =>
            {
                Some(Message::SaveShortcut)
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                Some(Message::CursorMoved(position))
            }
            Event::Window(iced::window::Event::Resized(size)) => {
                Some(Message::WindowResized(size.width, size.height))
            }
            _ => None,
        })
    }

    // ── view-model helpers ──────────────────────────────────────────────────

    fn current_items(&self) -> Vec<&Item> {
        let coll_id = self.sel_coll.clone().unwrap_or_default();
        filtered_items(&self.data, &coll_id, &self.item_search, self.settings.item_sort)
    }

    fn selected_item(&self) -> Option<&Item> {
        let id = self.sel_item.as_ref()?;
        self.data.items.iter().find(|i| &i.id == id)
    }

    // Approximate pixel widths for truncation budgets. pane_grid is ratio-based
    // and the window is resizable, so these use a nominal window width; exact
    // precision isn't needed since truncation is itself approximate.
    fn nominal_window_w(&self) -> f32 { self.window_w.max(400.0) }
    fn nominal_window_h(&self) -> f32 { self.window_h.max(300.0) }

    /// Thumbnail handle for `stored`, decoded once and cached. Returns None if
    /// the image can't be loaded.
    fn thumb(&self, stored: &str) -> Option<image_widget::Handle> {
        if stored.is_empty() { return None; }
        if let Some(h) = self.thumb_cache.borrow().get(stored) {
            return Some(h.clone());
        }
        let handle = image_util::thumbnail_handle(stored)?;
        {
            let mut cache = self.thumb_cache.borrow_mut();
            // Soft cap so a long session with many distinct photos can't grow the
            // cache without bound. Far more than fit on screen, so this only
            // trims long-cold entries. (Photos are UUID-named, so a re-inserted
            // key is always the same image — no stale-content risk.)
            const MAX_THUMB_CACHE: usize = 512;
            if cache.len() >= MAX_THUMB_CACHE {
                cache.clear();
            }
            cache.insert(stored.to_string(), handle.clone());
        }
        Some(handle)
    }

    /// Drop a single cached thumbnail handle (e.g. when its photo is removed),
    /// so the cache doesn't retain handles for files that no longer exist.
    fn evict_thumb(&self, stored: &str) {
        if stored.is_empty() { return; }
        self.thumb_cache.borrow_mut().remove(stored);
    }
    fn left_px(&self) -> f32 {
        self.nominal_window_w() * self.settings.left_ratio
    }
    fn mid_px(&self) -> f32 {
        // Mid pane's share of the width remaining after the left pane.
        self.nominal_window_w() * (1.0 - self.settings.left_ratio) * self.settings.mid_ratio
    }

    fn rebuild_coll_checked(&mut self) {
        self.coll_checked = vec![false; self.data.collections.len()];
        self.coll_multi = false;
    }

    fn rebuild_item_checked(&mut self) {
        self.item_checked = vec![false; self.current_items().len()];
        self.item_multi = false;
    }

    fn reload_editors(&mut self) {
        self.editors = match self.selected_item() {
            Some(it) => DetailEditors::from_item(it),
            None => DetailEditors::empty(),
        };
        // Clear any lingering main-photo hover so the "add more photos" prompt
        // doesn't stick when the selected item or edit state changes.
        self.hover_main_photo = false;
    }

    fn persist(&self) {
        save_data(&self.data);
    }
    fn persist_settings(&self) {
        save_settings(&self.settings);
    }
    fn reapply_theme(&mut self) {
        self.palette = build_palette(self.settings.dark_mode, &self.settings.accent_hex);
    }
}

include!("update.rs");
include!("view.rs");
