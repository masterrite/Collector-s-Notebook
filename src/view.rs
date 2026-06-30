// src/view.rs — included into main.rs. The `view` method and widget builders.
//
// Emoji handling: every glyph that may be emoji is rendered with `emoji_text`,
// which sets the color-emoji font AND a line_height of 1.3 relative to the font
// size. Because the line box scales with the text, emoji never clip at any font
// size (the original audit issue). Body text uses the CJK-capable font so user
// content in any script renders.

use iced::widget::container::Style as ContainerStyle;
use iced::Border;

// iced 0.14's `Space` has only `Space::new()` (no args); size is set via
// `.width()`/`.height()`. These helpers restore the old ergonomics.
fn hspace(w: impl Into<Length>) -> iced::widget::Space {
    iced::widget::Space::new().width(w)
}
fn vspace(h: impl Into<Length>) -> iced::widget::Space {
    iced::widget::Space::new().height(h)
}
fn boxspace(w: impl Into<Length>, h: impl Into<Length>) -> iced::widget::Space {
    iced::widget::Space::new().width(w).height(h)
}

// ── text_editor key bindings (word ops + Tab field nav) ─────────────────────
//
// iced 0.14's text_editor lets us override key handling with `.key_binding()`,
// a closure `Fn(KeyPress) -> Option<Binding<Message>>`. Returning `Some(b)`
// runs binding `b`; returning `None` makes the key do nothing. Because a custom
// closure *replaces* the defaults entirely, anything we don't handle explicitly
// is forwarded to `Binding::from_key_press(kp)` so normal typing, Enter, plain
// Backspace/Delete, arrows, copy/paste, etc. keep working.
//
// The "jump" modifier (`Modifiers::jump()`) is Ctrl on Windows/Linux and Option
// on macOS — the native word-wise modifier on each platform.

/// Word-wise operations shared by every editor. Returns `Some(..)` only when the
/// jump modifier is held together with Backspace/Delete/Arrow; otherwise `None`
/// so the caller can fall through to default handling.
fn editor_word_binding(kp: &text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    use iced::keyboard::key::Named;
    use iced::keyboard::Key;
    use text_editor::{Binding, Motion};

    let word = kp.modifiers.jump();
    if !word {
        return None;
    }
    let shift = kp.modifiers.shift();
    let Key::Named(named) = &kp.key else {
        return None;
    };
    match named {
        // Ctrl/Option+Backspace — delete the word to the left: select it, then delete.
        Named::Backspace => Some(Binding::Sequence(vec![
            Binding::Select(Motion::WordLeft),
            Binding::Backspace,
        ])),
        // Ctrl/Option+Delete — delete the word to the right.
        Named::Delete => Some(Binding::Sequence(vec![
            Binding::Select(Motion::WordRight),
            Binding::Delete,
        ])),
        // Ctrl/Option+Left/Right — jump by word (extend the selection if Shift is held).
        Named::ArrowLeft => Some(if shift {
            Binding::Select(Motion::WordLeft)
        } else {
            Binding::Move(Motion::WordLeft)
        }),
        Named::ArrowRight => Some(if shift {
            Binding::Select(Motion::WordRight)
        } else {
            Binding::Move(Motion::WordRight)
        }),
        _ => None,
    }
}

/// Builds the key-binding closure for the detail editor at position `idx`.
/// Word ops are shared; Tab/Shift+Tab emit a message carrying this editor's
/// index and the direction. `update` then moves focus positionally with
/// focus_next/focus_previous, clamping at the ends so it stays in the panel.
/// Index order: 0 name, 1 description, 2 year, 3 month, 4 day, then for each
/// custom field i: 5+2i label, 6+2i value.
fn detail_keys(
    idx: usize,
) -> impl Fn(text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    move |kp| {
        use iced::keyboard::key::Named;
        use iced::keyboard::Key;
        use text_editor::Binding;

        if let Some(b) = editor_word_binding(&kp) {
            return Some(b);
        }
        // Tab → next detail field, Shift+Tab → previous. Overrides text_editor's
        // default Tab-indents-line behavior here.
        if let Key::Named(Named::Tab) = &kp.key {
            return Some(Binding::Custom(Message::TabField(idx, !kp.modifiers.shift())));
        }
        Binding::from_key_press(kp)
    }
}

/// Key bindings for modal / overlay editors: word ops only. Tab is left to the
/// editor's default so focus can't escape the modal into hidden base widgets.
fn editor_keys_basic(kp: text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    if let Some(b) = editor_word_binding(&kp) {
        return Some(b);
    }
    text_editor::Binding::from_key_press(kp)
}

// ── small style/element helpers ─────────────────────────────────────────────

/// Truncate a string to a single display line of roughly `max` characters,
/// appending an ellipsis. iced's text has no native ellipsis, so we clamp the
/// source string and render with no-wrap.
fn truncate_one_line(s: &str, max: usize) -> String {
    let one = s.replace('\n', " ");
    if one.chars().count() > max {
        // Use three literal dots rather than the single "…" glyph: a partially
        // clipped "…" shows as one or two dots, whereas trailing ASCII dots
        // degrade cleanly.
        let mut t: String = one.chars().take(max.saturating_sub(3)).collect();
        t.push_str("...");
        t
    } else {
        one
    }
}

/// Approximate the rendered width of a single character, in em units.
/// Latin/ASCII and most narrow scripts are ~0.58em; CJK ideographs, kana,
/// Hangul, fullwidth forms, and emoji render roughly full-width (~1em), so the
/// flat 0.58 estimate badly under-counts them and lets the text overflow its
/// container. Treating those ranges as ~1em keeps the truncation honest for the
/// CJK content this app is built around.
fn char_em_width(c: char) -> f32 {
    let u = c as u32;
    let wide = matches!(u,
        0x1100..=0x115F |   // Hangul Jamo
        0x2E80..=0x303E |   // CJK radicals, Kangxi, CJK symbols/punct
        0x3041..=0x33FF |   // Hiragana, Katakana, CJK symbols, compat
        0x3400..=0x4DBF |   // CJK Ext A
        0x4E00..=0x9FFF |   // CJK Unified
        0xA000..=0xA4CF |   // Yi
        0xAC00..=0xD7A3 |   // Hangul syllables
        0xF900..=0xFAFF |   // CJK compat ideographs
        0xFE30..=0xFE4F |   // CJK compat forms
        0xFF00..=0xFF60 |   // Fullwidth forms
        0xFFE0..=0xFFE6 |   // Fullwidth signs
        0x1F000..=0x1FAFF | // emoji & symbols
        0x20000..=0x3FFFD   // CJK Ext B+ (supplementary ideographic planes)
    );
    if wide { 1.0 } else { 0.58 }
}

/// Truncate to fit a pixel width given the font size, appending an ellipsis.
/// Walks the string accumulating per-character em widths (so CJK/emoji count as
/// full-width) and stops once the budget is exhausted, reserving slack so the
/// trailing dots stay fully visible.
fn truncate_to_width(s: &str, width_px: f32, font_size: f32) -> String {
    let em = font_size.max(1.0);
    let one = s.replace('\n', " ");
    // Reserve ~1.5 average glyphs of slack so the trailing dots aren't clipped.
    let usable = (width_px - em * 0.58 * 1.5).max(em * 0.58);

    // First pass: does the whole string fit?
    let total: f32 = one.chars().map(|c| char_em_width(c) * em).sum();
    if total <= usable {
        return one;
    }

    // Doesn't fit: take as many leading chars as fit, leaving room for "...".
    let dots = em * 0.58 * 3.0;
    let budget = (usable - dots).max(0.0);
    let mut used = 0.0;
    let mut t = String::new();
    for c in one.chars() {
        let w = char_em_width(c) * em;
        if used + w > budget { break; }
        used += w;
        t.push(c);
    }
    t.push_str("...");
    t
}

impl App {
    fn fs(&self) -> f32 { self.settings.font_size }

    /// Vertical scroll area with a thin scrollbar sitting in a right-side gutter
    /// so it doesn't overlap the content (cards/rows).
    fn vscroll<'a>(&self, content: Element<'a, Message>, id: iced::advanced::widget::Id) -> Element<'a, Message> {
        scrollable(container(content).padding(iced::Padding { top: 0.0, right: 10.0, bottom: 0.0, left: 0.0 }))
            .id(id)
            .height(Fill)
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(6).scroller_width(6).margin(2)))
            .into()
    }

    fn body<'a>(&self, s: impl iced::widget::text::IntoFragment<'a>) -> iced::widget::Text<'a> {
        text(s).font(CJK).size(self.fs()).line_height(1.3)
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
            .color(self.palette.text_primary)
    }
    /// Bold heading/title text, using the bold face of the CJK family.
    fn heading<'a>(&self, s: impl iced::widget::text::IntoFragment<'a>, size: f32) -> iced::widget::Text<'a> {
        text(s).font(CJK_BOLD).size(size).line_height(1.3)
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
            .color(self.palette.text_primary)
    }
    fn muted<'a>(&self, s: impl iced::widget::text::IntoFragment<'a>) -> iced::widget::Text<'a> {
        text(s).font(CJK).size(self.fs() - 2.0).line_height(1.3)
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
            .color(self.palette.text_muted)
    }
    fn label<'a>(&self, s: impl iced::widget::text::IntoFragment<'a>) -> iced::widget::Text<'a> {
        text(s).font(CJK).size((self.fs() - 4.0).max(9.0)).line_height(1.4)
            .color(self.palette.text_muted)
    }
    /// Centered, bold sub-header for the detail panel sections.
    fn subheader<'a>(&self, s: impl iced::widget::text::IntoFragment<'a>) -> Element<'a, Message> {
        container(
            text(s).font(CJK_BOLD).size((self.fs() - 1.0).max(11.0)).line_height(1.4)
                .color(self.palette.text_secondary)
        ).center_x(Fill).into()
    }
    /// A thin horizontal separation line.
    fn separator(&self) -> Element<'_, Message> {
        container(boxspace(Fill, Length::Fixed(1.0)))
            .style(self.card_style(self.palette.border, Color::TRANSPARENT, 0.0, 0.0))
            .into()
    }
    /// Color-emoji text with a generous line height so glyphs never clip.
    fn emoji_text<'a>(&self, s: impl iced::widget::text::IntoFragment<'a>, size: f32) -> iced::widget::Text<'a> {
        text(s).font(EMOJI).size(size).line_height(1.3)
    }

    fn panel_style(&self, bg: Color) -> impl Fn(&IcedTheme) -> ContainerStyle {
        move |_| ContainerStyle {
            background: Some(bg.into()),
            ..ContainerStyle::default()
        }
    }

    fn card_style(&self, bg: Color, border: Color, bw: f32, radius: f32)
        -> impl Fn(&IcedTheme) -> ContainerStyle
    {
        move |_| ContainerStyle {
            background: Some(bg.into()),
            border: Border { color: border, width: bw, radius: radius.into() },
            ..ContainerStyle::default()
        }
    }

    fn editor_style(&self, editing: bool) -> impl Fn(&IcedTheme, text_editor::Status) -> text_editor::Style {
        let p = self.palette;
        move |_theme, _status| text_editor::Style {
            background: p.bg_input.into(),
            border: Border {
                color: if editing { p.accent_dim } else { p.border },
                width: if editing { 1.0 } else { 0.0 },
                radius: 6.0.into(),
            },
            placeholder: p.text_muted,
            value: p.text_primary,
            selection: p.border_accent,
        }
    }

    /// A more visible bordered editor for pop-up panels.
    fn editor_style_strong(&self) -> impl Fn(&IcedTheme, text_editor::Status) -> text_editor::Style {
        let p = self.palette;
        move |_theme, _status| {
            text_editor::Style {
                background: p.bg_input.into(),
                border: Border {
                    color: p.border_accent,
                    width: 1.5,
                    radius: 6.0.into(),
                },
                placeholder: p.text_muted,
                value: p.text_primary,
                selection: p.border_accent,
            }
        }
    }

    // ── top-level view ───────────────────────────────────────────────────

    fn view(&self) -> Element<'_, Message> {
        // Three resizable panes (collections | items | detail) via pane_grid.
        // Dragging the dividers resizes; panes can't be reordered (no on_drag).
        // spacing(0) + no .style() keeps the grid invisible so the panels' own
        // borders/backgrounds define the look exactly as before.
        let grid = pane_grid(&self.panes, |_pane, id, _maximized| {
            pane_grid::Content::new(match id {
                PaneId::Left => self.left_panel(),
                PaneId::Mid => self.mid_panel(),
                PaneId::Right => self.right_panel(),
            })
        })
        .width(Fill)
        .height(Fill)
        .spacing(0)
        .on_resize(8, Message::PaneResized);

        let base = container(grid)
            .style(self.panel_style(self.palette.bg_base))
            .width(Fill).height(Fill);

        // Overlays render on top via stack. Wrapping each overlay in `opaque`
        // makes it capture all mouse interaction (including cursor shape), so
        // hovering the overlay never shows the I-beam from a text box behind it.
        // The root must keep an identical widget-tree shape whether or not an
        // overlay is open. iced preserves per-widget state (like a scrollable's
        // offset, keyed by its stable .id()) only when the surrounding tree
        // structure is stable. Previously `Overlay::None` returned `base` while
        // every other arm returned `stack![base, …]`, so opening an overlay
        // (e.g. the right-click context menu) pushed `base` one level deeper and
        // iced rebuilt the scrollables from scratch, resetting the scroll. By
        // ALWAYS rendering a two-layer stack — with a zero-size transparent
        // placeholder when there's no overlay — `base` stays at the same depth
        // and the scroll offset is retained.
        use iced::widget::opaque;
        let layer: Element<'_, Message> = match self.overlay {
            // A Shrink-sized Space has zero bounds, so even wrapped in opaque it
            // captures no input — clicks pass through to `base` as before.
            Overlay::None => {
                iced::widget::Space::new().width(Shrink).height(Shrink).into()
            }
            Overlay::Settings => opaque(self.settings_overlay()),
            Overlay::IconPicker => opaque(self.icon_picker_overlay()),
            Overlay::TemplatePicker => opaque(self.template_picker_overlay()),
            Overlay::Lightbox => opaque(self.lightbox_overlay()),
            Overlay::NameInput => opaque(self.name_input_overlay()),
            Overlay::ContextMenu => self.context_menu_overlay(),
            Overlay::Enlarge => opaque(self.enlarge_overlay()),
            Overlay::DataRecovered => opaque(self.data_recovered_overlay()),
        };
        stack![base, layer].into()
    }

    // ── LEFT: collections ─────────────────────────────────────────────────

    fn left_panel(&self) -> Element<'_, Message> {
        let p = self.palette;

        let header = container(row![
            self.heading("Collections", self.fs() + 1.0),
            hspace(Fill),
            self.icon_btn("⚙", Message::OpenSettings),
            self.icon_btn("➕", Message::NewCollection),
        ]
        .spacing(2)
        .align_y(iced::Alignment::Center))
        .height(Length::Fixed(36.0)).center_y(Length::Fixed(36.0));

        let search = self.search_box(
            &self.coll_search, "Filter collections...",
            Message::CollSearchChanged, Message::ClearCollSearch,
        );

        let sort = self.sort_bar(false, self.settings.coll_sort, Message::SetCollSort);

        let mut list = column![].spacing(2);
        if self.data.collections.is_empty() {
            list = list.push(
                container(
                    column![
                        self.emoji_text("🗂", 36.0),
                        self.muted("Press + to create a collection"),
                    ]
                    .spacing(8)
                    .align_x(iced::Alignment::Center)
                )
                .center_x(Fill).padding(20)
            );
        } else {
            let q = self.coll_search.to_lowercase();
            for (i, c) in self.data.collections.iter().enumerate() {
                if !q.is_empty() && !c.name.to_lowercase().contains(&q) { continue; }
                list = list.push(self.collection_row(i, c));
            }
        }

        let bottom = self.multi_hint(self.coll_multi || self.item_multi);

        let inner = column![
            header,
            search,
            sort,
            self.vscroll(list.into(), COLL_SCROLL.clone()),
            bottom,
        ]
        .spacing(6)
        .padding(14);

        container(inner)
            .width(Fill)
            .height(Fill)
            .style(self.panel_style(p.bg_panel))
            .into()
    }

    fn collection_row<'a>(&'a self, idx: usize, c: &'a Collection) -> Element<'a, Message> {
        let p = self.palette;
        let selected = self.sel_coll.as_deref() == Some(&c.id);
        let checked = self.coll_checked.get(idx).copied().unwrap_or(false);
        let count = item_count(&self.data, &c.id);

        // Icon badge — its own interactive button so it highlights separately
        // and clicking it opens the icon picker (not the row selection).
        let icon_box = button(
            container(self.emoji_text(c.icon.clone(), self.fs() + 2.0))
                .center_x(Length::Fixed(30.0)).center_y(Length::Fixed(30.0))
        )
        .padding(0)
        .on_press(Message::OpenIconPicker(idx))
        .style(move |_t, status| {
            let bg = if selected { p.accent_dim } else { p.bg_elevated };
            let bw = if matches!(status, button::Status::Hovered | button::Status::Pressed) { 1.0 } else { 0.0 };
            button::Style {
                background: Some(bg.into()),
                text_color: p.text_primary,
                border: Border { color: p.accent, width: bw, radius: 8.0.into() },
                ..button::Style::default()
            }
        });

        // Name: bold when selected, single line, clipped to the column width.
        let name_w = (self.left_px() - 88.0).max(40.0);
        let name = container(
            text(truncate_to_width(&c.name, name_w, self.fs()))
                .font(if selected { CJK_BOLD } else { CJK })
                .size(self.fs()).line_height(1.2)
                .wrapping(iced::widget::text::Wrapping::None)
                .color(if selected { p.text_primary } else { p.text_secondary })
        ).width(Fill).clip(true);

        let mut left = row![].spacing(8).align_y(iced::Alignment::Center);
        if self.coll_multi {
            left = left.push(self.checkbox(checked, Message::ToggleCollCheck(idx)));
        }
        left = left.push(icon_box);
        left = left.push(
            column![
                name,
                text(format!("{count} items")).font(CJK)
                    .size((self.fs() - 3.0).max(10.0)).color(p.text_muted),
            ].spacing(2).width(Fill)
        );

        // Whole-row container styled from hover/selection state. Selection and
        // gestures (single/double/right click) are handled by the OUTER
        // mouse_area — a button would swallow the double-click in iced 0.14.
        let hovered = self.hover_coll == Some(idx);
        let row_bg = if selected { p.bg_selected }
            else if hovered { p.bg_surface }
            else { Color::TRANSPARENT };
        let inner = container(left).padding([6, 10]).width(Fill)
            .style(self.card_style(row_bg, p.border_accent,
                if selected { 1.0 } else { 0.0 }, 8.0));

        // Left accent ribbon when selected, overlaid at the left edge.
        let row_el: Element<Message> = if selected {
            let ribbon = container(boxspace(Length::Fixed(3.0), Length::Fixed(32.0)))
                .style(self.card_style(p.accent, Color::TRANSPARENT, 0.0, 2.0));
            stack![inner, container(ribbon).center_y(Fill)].into()
        } else {
            inner.into()
        };

        mouse_area(row_el)
            .on_press(Message::SelectCollection(idx))
            .on_double_click(Message::OpenRenameCollection(idx))
            .on_right_press(Message::CollRightClicked(idx))
            .on_enter(Message::HoverColl(Some(idx)))
            .on_exit(Message::HoverColl(None))
            .into()
    }

    // ── MIDDLE: items ──────────────────────────────────────────────────────

    fn mid_panel(&self) -> Element<'_, Message> {
        let p = self.palette;
        let has_coll = self.sel_coll.is_some();

        let title = if let Some(id) = &self.sel_coll {
            self.data.collections.iter().find(|c| &c.id == id)
                .map(|c| c.name.clone()).unwrap_or_else(|| "Items".into())
        } else { "Items".into() };

        // Title is width-bounded and single-line so long collection names never
        // push the +/Delete buttons out of the panel.
        let title_el = container(
            text(truncate_to_width(&title, (self.mid_px() - 70.0).max(40.0), self.fs() + 1.0))
                .font(CJK_BOLD).size(self.fs() + 1.0).line_height(1.2)
                .wrapping(iced::widget::text::Wrapping::None)
                .color(p.text_primary)
        ).width(Fill).clip(true);

        let mut header = row![title_el].align_y(iced::Alignment::Center).spacing(6);
        if has_coll && self.item_multi {
            header = header.push(self.danger_btn("× Delete", Message::DeleteSelectedItems));
        }
        if has_coll {
            header = header.push(self.icon_btn("➕", Message::NewItem));
        }
        let header = container(header)
            .height(Length::Fixed(36.0)).center_y(Length::Fixed(36.0));

        let mut col = column![header].spacing(6).padding(14);

        if has_coll {
            col = col.push(self.search_box(
                &self.item_search, "Search...",
                Message::ItemSearchChanged, Message::ClearItemSearch,
            ));
            col = col.push(self.sort_bar(false, self.settings.item_sort, Message::SetItemSort));

            let items = self.current_items();
            let mut list = column![].spacing(5);
            if items.is_empty() {
                list = list.push(
                    container(column![
                        self.emoji_text("📦", 36.0),
                        self.muted("Press + to add an item"),
                    ].spacing(8).align_x(iced::Alignment::Center))
                    .center_x(Fill).padding(20)
                );
            } else {
                for (i, it) in items.iter().enumerate() {
                    list = list.push(self.item_card(i, it));
                }
            }
            col = col.push(self.vscroll(list.into(), ITEM_SCROLL.clone()));
        } else {
            col = col.push(
                container(column![
                    self.emoji_text("←", self.fs() + 8.0),
                    self.muted("Select a collection"),
                ].spacing(8).align_x(iced::Alignment::Center))
                .center_x(Fill).center_y(Fill)
            );
        }

        container(col)
            .width(Fill)
            .height(Fill)
            .style(self.panel_style(p.bg_base))
            .into()
    }

    fn item_card<'a>(&'a self, idx: usize, it: &'a Item) -> Element<'a, Message> {
        let p = self.palette;
        let selected = self.sel_item.as_deref() == Some(&it.id);
        let checked = self.item_checked.get(idx).copied().unwrap_or(false);

        let mut rowc = row![].spacing(10).align_y(iced::Alignment::Center);
        if self.item_multi {
            rowc = rowc.push(self.checkbox(checked, Message::ToggleItemCheck(idx)));
        }

        // thumbnail
        let thumb: Element<Message> = match it.primary_photo().and_then(|s| self.thumb(s)) {
            Some(h) => image_widget(h)
                .width(Length::Fixed(56.0)).height(Length::Fixed(56.0))
                .content_fit(iced::ContentFit::Cover).into(),
            None => container(self.emoji_text("📷", 22.0))
                .center_x(Length::Fixed(56.0)).center_y(Length::Fixed(56.0))
                .style(self.card_style(p.bg_elevated, p.border, 0.0, 8.0)).into(),
        };
        rowc = rowc.push(thumb);
        // Description: ~2 wrapped lines, height-capped and pre-truncated.
        let line_px = (self.fs() - 2.0) * 1.3;
        let desc = truncate_one_line(&it.short_desc, 140);
        // Reserve space for padding(28) + card padding(20) + thumb(56) +
        // spacing(10) and the checkbox when multi-selecting.
        let reserve = 114.0 + if self.item_multi { 22.0 } else { 0.0 };
        let name_w = (self.mid_px() - reserve).max(40.0);
        rowc = rowc.push(
            column![
                container(
                    text(truncate_to_width(&it.name, name_w, self.fs()))
                        .font(CJK).size(self.fs())
                        .line_height(1.2)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .color(p.text_primary)
                ).width(Fill).clip(true),
                container(self.muted(desc))
                    .width(Fill)
                    .max_height(line_px * 2.0 + 2.0)
                    .clip(true),
            ].spacing(2).width(Fill)
        );

        let hovered = self.hover_item == Some(idx);
        let card_bg = if selected { p.bg_selected }
            else if hovered { p.bg_hover }
            else { p.bg_card };
        let card = container(rowc).padding(10).width(Fill)
            .style(self.card_style(card_bg,
                if selected { p.border_accent } else { p.border }, 1.0, 10.0));

        mouse_area(card)
            .on_press(Message::SelectItem(idx))
            .on_double_click(Message::OpenRenameItem(idx))
            .on_right_press(Message::ItemRightClicked(idx))
            .on_enter(Message::HoverItem(Some(idx)))
            .on_exit(Message::HoverItem(None))
            .into()
    }

    // ── RIGHT: detail ────────────────────────────────────────────────────

    fn right_panel(&self) -> Element<'_, Message> {
        let p = self.palette;

        let content: Element<Message> = if self.sel_item.is_none() {
            container(column![
                self.emoji_text("🔍", 36.0),
                self.muted("Select an item to view details"),
            ].spacing(10).align_x(iced::Alignment::Center))
            .center_x(Fill).center_y(Fill).into()
        } else {
            scrollable(self.detail_body())
                .height(Fill)
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new().width(8).scroller_width(8)))
                .into()
        };

        container(content)
            .width(Fill).height(Fill)
            .style(self.panel_style(p.bg_panel))
            .into()
    }

    fn detail_body(&self) -> Element<'_, Message> {
        let p = self.palette;
        let editing = self.is_editing;

        // top bar
        let edit_label = if editing { "Save" } else { "Edit" };
        let edit_emoji = if editing { "💾" } else { "✏" };
        let title_text = if self.editors.name.text().trim().is_empty() {
            "New Item".to_string()
        } else {
            self.editors.name.text().trim_end_matches('\n').to_string()
        };
        let top = container(row![
            // One-line, clipped title so it never reflows the panel content and
            // matches the other panel headers' size (fs+1).
            container(
                text(truncate_to_width(
                    &title_text,
                    (self.nominal_window_w() - self.left_px() - self.mid_px() - 240.0).max(80.0),
                    self.fs() + 1.0))
                    .font(CJK_BOLD).size(self.fs() + 1.0).line_height(1.2)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .color(p.text_primary)
            ).width(Fill).clip(true),
            self.icon_label_btn(edit_emoji, edit_label, p.accent_text, false, Message::ToggleEdit),
            self.icon_label_btn("🗑", "Delete", p.danger_text, true, Message::DeleteItem),
        ].spacing(8).align_y(iced::Alignment::Center))
        .height(Length::Fixed(36.0)).center_y(Length::Fixed(36.0));

        // photo + name/desc
        let photo: Element<Message> = match self.selected_item()
            .and_then(|i| i.primary_photo()).and_then(|s| self.thumb(s)) {
            Some(h) => {
                let img = image_widget(h)
                    .width(Length::Fixed(140.0)).height(Length::Fixed(140.0))
                    .content_fit(iced::ContentFit::Cover);
                // Hovering the main photo reveals a prompt over a translucent
                // scrim. In edit mode the prompt is "Add more photos" and a click
                // opens the file picker; otherwise it's "Click to enlarge" and a
                // click opens the lightbox.
                let (prompt_text, click_msg) = if editing {
                    ("Add more photos", Message::PickPhotos)
                } else {
                    ("Click to enlarge", Message::OpenLightbox(0))
                };
                let layered: Element<Message> = if self.hover_main_photo {
                    let prompt = container(
                        text(prompt_text).font(CJK).size(self.fs() - 1.0)
                            .line_height(1.2)
                            .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                            .color(Color::WHITE)
                    )
                    .center_x(Length::Fixed(140.0)).center_y(Length::Fixed(140.0))
                    .padding(8)
                    .style(self.card_style(
                        Color { a: 0.55, ..Color::BLACK }, Color::TRANSPARENT, 0.0, 11.0));
                    stack![img, prompt].into()
                } else {
                    img.into()
                };
                mouse_area(layered)
                    .on_press(click_msg)
                    .on_enter(Message::HoverMainPhoto(true))
                    .on_exit(Message::HoverMainPhoto(false))
                    .into()
            }
            None => {
                let ph = container(column![
                    self.emoji_text("📷", 32.0),
                    if editing { self.muted("Click to add") } else { self.muted("") },
                ].spacing(6).align_x(iced::Alignment::Center))
                .center_x(Length::Fixed(140.0)).center_y(Length::Fixed(140.0))
                .style(self.card_style(p.bg_surface, p.border, 1.0, 11.0));
                if editing { mouse_area(ph).on_press(Message::PickPhotos).into() } else { ph.into() }
            }
        };

        let name_block = column![
            stack![
                self.subheader("Name"),
                container(self.enlarge_btn(EnlargeTarget::Name)).align_right(Fill),
            ],
            text_editor(&self.editors.name)
                .on_action(Message::NameEdited)
                .key_binding(detail_keys(0))
                .font(CJK).size(self.fs() + 1.0)
                .padding(8)
                // Name is a single line; it scrolls horizontally past that.
                .min_height((self.fs() + 1.0) * 1.3 + 16.0)
                .max_height((self.fs() + 1.0) * 1.3 + 16.0)
                .wrapping(iced::widget::text::Wrapping::None)
                .style(self.editor_style(editing)),
        ].spacing(4);

        let desc_block = column![
            stack![
                self.subheader("Description"),
                container(self.enlarge_btn(EnlargeTarget::Desc)).align_right(Fill),
            ],
            text_editor(&self.editors.desc)
                .on_action(Message::DescEdited)
                .key_binding(detail_keys(1))
                .font(CJK).size(self.fs())
                // Fixed three-line box. The editor renders at ~1.0x line height
                // (not the 1.3 the text widget uses), so size off ~1.15x per line
                // for a little breathing room; min == max keeps the height stable
                // while scrolling. Content beyond 3 lines scrolls.
                .min_height(self.fs() * 1.15 * 3.0 + 16.0)
                .max_height(self.fs() * 1.15 * 3.0 + 16.0)
                .padding(8)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                .style(self.editor_style(editing)),
        ].spacing(4);

        let head = row![
            photo,
            column![name_block, desc_block].spacing(10).width(Fill),
        ].spacing(18).align_y(iced::Alignment::Center);

        let mut body = column![top, head].spacing(12)
            .padding(iced::Padding { top: 14.0, right: 22.0, bottom: 22.0, left: 22.0 });

        // photo strip
        if let Some(it) = self.selected_item() {
            if it.photos.len() > 1 || (editing && !it.photos.is_empty()) {
                body = body.push(self.subheader("Photos"));
                let mut strip = row![].spacing(6);
                for (i, _p) in it.photos.iter().enumerate() {
                    let handle = it.photos.get(i).and_then(|s| self.thumb(s));
                    let thumb: Element<Message> = match handle {
                        Some(h) => image_widget(h)
                            .width(Length::Fixed(56.0)).height(Length::Fixed(56.0))
                            .content_fit(iced::ContentFit::Cover).into(),
                        None => hspace(Length::Fixed(56.0)).into(),
                    };
                    let main_badge = i == 0;
                    let bordered = container(thumb)
                        .style(self.card_style(p.bg_surface,
                            if main_badge { p.accent } else { p.border },
                            if main_badge { 2.0 } else { 1.0 }, 8.0));
                    let tile = mouse_area(bordered).on_press(
                        if editing { Message::SetMainPhoto(i) } else { Message::OpenLightbox(i) });
                    // Delete badge overlaid at the TOP-RIGHT of the thumbnail, the
                    // ✖ centered within its circle.
                    let cell: Element<Message> = if editing {
                        let badge = container(
                            mouse_area(
                                container(
                                    text("×").font(CJK_BOLD).size(13.0).line_height(1.0)
                                        .color(p.danger_text)
                                )
                                    .center_x(Length::Fixed(18.0)).center_y(Length::Fixed(18.0))
                                    .style(self.card_style(p.danger_bg, p.danger_text, 1.0, 9.0))
                            ).on_press(Message::RemovePhoto(i))
                        ).width(Length::Fixed(56.0)).height(Length::Fixed(56.0))
                            .align_right(Length::Fixed(56.0)).align_top(Length::Fixed(56.0));
                        stack![tile, badge].into()
                    } else {
                        tile.into()
                    };
                    strip = strip.push(cell);
                }
                // Scrollbar sits BELOW the thumbnails (bottom padding gives it a
                // clear lane that doesn't overlap the tiles). The padding +
                // margin set how far below the thumbnails the drag bar sits.
                body = body.push(
                    container(
                        scrollable(container(strip).padding(iced::Padding { top: 0.0, right: 0.0, bottom: 16.0, left: 0.0 }))
                            .direction(scrollable::Direction::Horizontal(
                                scrollable::Scrollbar::new().width(6).scroller_width(6).margin(4)))
                    )
                );
                body = body.push(self.muted(if editing {
                    "Click to set as main · × to remove"
                } else {
                    "Click a photo to enlarge"
                }));
            }
        }

        // date
        body = body.push(self.subheader("Date Acquired"));
        if editing {
            body = body.push(container(row![
                self.tiny_editor(&self.editors.year, Message::YearEdited, 70.0, "YYYY", 2),
                text("-").color(p.text_muted),
                self.tiny_editor(&self.editors.month, Message::MonthEdited, 48.0, "MM", 3),
                text("-").color(p.text_muted),
                self.tiny_editor(&self.editors.day, Message::DayEdited, 48.0, "DD", 4),
            ].spacing(6).align_y(iced::Alignment::Center)).center_x(Fill));
        } else if let Some(it) = self.selected_item() {
            body = body.push(container(self.body(display_date(&it.acquired_date))).center_x(Fill));
        }

        // Separation line between Date and Details.
        body = body.push(self.separator());

        // details header + template/add buttons
        body = body.push(self.subheader("Details"));
        if editing {
            body = body.push(row![
                self.small_action("Load template", Message::OpenTemplatePicker),
                self.small_action("Save as template", Message::OpenSaveTemplate),
                self.small_action("+ Field", Message::AddCustomField),
            ].spacing(4));
        }

        // custom fields — each label + value is a scrollable multiline editor
        for (i, (fid, _lbl, _val)) in self.editors.fields.iter().enumerate() {
            let label_el: Element<Message> = if editing {
                text_editor(&self.editors.fields[i].1)
                    .on_action(move |a| Message::FieldLabelEdited(i, a))
                    .key_binding(detail_keys(5 + i * 2))
                    .font(CJK).size((self.fs() - 4.0).max(9.0))
                    .padding(4)
                    .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                    .style(self.editor_style(true)).into()
            } else {
                self.label(self.editors.fields[i].1.text()).into()
            };

            let mut field_row = column![
                row![
                    container(label_el).width(Length::Fixed(200.0)),
                    hspace(Fill),
                    self.enlarge_btn(EnlargeTarget::Field(i)),
                    if editing {
                        Element::from(mouse_area(self.emoji_text("✖", 13.0))
                            .on_press(Message::DeleteCustomField(fid.clone())))
                    } else {
                        Element::from(hspace(Shrink))
                    },
                ].spacing(6).align_y(iced::Alignment::Center),
            ].spacing(3);

            field_row = field_row.push(
                text_editor(&self.editors.fields[i].2)
                    .on_action(move |a| Message::FieldValueEdited(i, a))
                    .key_binding(detail_keys(6 + i * 2))
                    .font(CJK).size(self.fs())
                    .min_height(self.fs() * 1.3 + 16.0)
                    .max_height(self.fs() * 1.3 * 3.0 + 16.0)
                    .padding(8)
                    .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                    .style(self.editor_style(editing))
            );
            body = body.push(field_row);
        }

        body.into()
    }

    // ── reusable widgets ──────────────────────────────────────────────────

    fn tiny_editor<'a>(
        &'a self, content: &'a text_editor::Content,
        on_action: impl Fn(text_editor::Action) -> Message + 'a,
        width: f32, _placeholder: &str, idx: usize,
    ) -> Element<'a, Message> {
        container(
            text_editor(content)
                .on_action(on_action)
                .key_binding(detail_keys(idx))
                .font(CJK).size(self.fs() - 1.0)
                .padding(6)
                .style(self.editor_style(true))
        ).width(Length::Fixed(width)).into()
    }

    /// Small "enlarge" button shown beside each detail field; opens the field in
    /// a large editable overlay.
    fn enlarge_btn(&self, target: EnlargeTarget) -> Element<'_, Message> {
        let p = self.palette;
        button(text("Enlarge").font(CJK).size((self.fs() - 4.0).max(9.0)))
            .padding([2, 6])
            .on_press(Message::OpenEnlarge(target))
            .style(move |_t, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => p.accent_dim,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(bg.into()),
                    text_color: p.accent_text,
                    // No border around Enlarge.
                    border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 5.0.into() },
                    ..button::Style::default()
                }
            }).into()
    }

    /// Emoji + label button with hover/press feedback (e.g. Edit, Delete).
    fn icon_label_btn<'a>(
        &'a self, glyph: &'static str, label: &'a str, fg: Color, danger: bool, msg: Message,
    ) -> Element<'a, Message> {
        let p = self.palette;
        button(row![
            self.emoji_text(glyph, self.fs()),
            text(label).font(CJK).size(self.fs()),
        ].spacing(4).align_y(iced::Alignment::Center))
            .padding([4, 8])
            .on_press(msg)
            .style(move |_t, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed =>
                        if danger {
                            // Visible tint in both themes (danger_bg is near-white
                            // in light mode, so derive from the danger text color).
                            Color { a: 0.16, ..p.danger_text }
                        } else { p.accent_dim },
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(bg.into()),
                    text_color: fg,
                    border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 6.0.into() },
                    ..button::Style::default()
                }
            }).into()
    }

    fn icon_btn(&self, glyph: &'static str, msg: Message) -> Element<'_, Message> {
        let p = self.palette;
        button(
            container(self.emoji_text(glyph, self.fs()))
                .center_x(Length::Fixed(30.0)).center_y(Length::Fixed(30.0))
        )
        .padding(0)
        .on_press(msg)
        .style(move |_t, status| {
            // Transparent normally; accent-tinted on hover, stronger on press.
            let bg = match status {
                button::Status::Hovered => p.accent_dim,
                button::Status::Pressed => p.accent,
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: p.text_secondary,
                border: Border { color: p.border, width: 0.0, radius: 6.0.into() },
                ..button::Style::default()
            }
        }).into()
    }

    fn danger_btn<'a>(&'a self, label: &'a str, msg: Message) -> Element<'a, Message> {
        let p = self.palette;
        button(text(label).font(CJK).size(self.fs() - 3.0))
            .padding([4, 8])
            .on_press(msg)
            .style(move |_t, status| {
                let (bg, fg) = match status {
                    button::Status::Hovered => (p.danger_text, Color::WHITE),
                    button::Status::Pressed => (p.danger_text, Color::WHITE),
                    _ => (p.danger_bg, p.danger_text),
                };
                button::Style {
                    background: Some(bg.into()),
                    text_color: fg,
                    border: Border { color: p.danger_text, width: 1.0, radius: 6.0.into() },
                    ..button::Style::default()
                }
            }).into()
    }

    fn small_action<'a>(&'a self, label: &'a str, msg: Message) -> Element<'a, Message> {
        let p = self.palette;
        button(text(label).font(CJK).size((self.fs() - 4.0).max(9.0)))
            .padding([4, 8])
            .on_press(msg)
            .style(move |_t, status| Self::btn_style(status, p, false))
            .into()
    }

    /// Shared status-aware style for text action buttons. `wide`/full-width
    /// styling is left to the caller; this just colors per interaction state.
    fn btn_style(status: button::Status, p: Palette, _wide: bool) -> button::Style {
        let (bg, fg, bw) = match status {
            button::Status::Hovered => (p.accent_dim, p.accent_text, 1.0),
            button::Status::Pressed => (p.accent, Color::WHITE, 1.0),
            _ => (p.bg_surface, p.text_secondary, 1.0),
        };
        button::Style {
            background: Some(bg.into()),
            text_color: fg,
            border: Border {
                color: if matches!(status, button::Status::Active) { p.border } else { p.border_accent },
                width: bw, radius: 6.0.into(),
            },
            ..button::Style::default()
        }
    }

    /// Larger action button (body-sized text, full-width) used for the settings
    /// DATA actions so they match the size of the Theme/Accent/Font-size rows.
    fn action_btn<'a>(&'a self, label: &'a str, msg: Message) -> Element<'a, Message> {
        let p = self.palette;
        button(text(label).font(CJK).size(self.fs()))
            .padding([10, 12]).width(Fill)
            .on_press(msg)
            .style(move |_t, status| Self::btn_style(status, p, true))
            .into()
    }

    /// Compact dialog button: shrinks to fit its centered text. `primary` makes
    /// it accent-colored to stand out (e.g. OK / Done).
    fn dialog_btn<'a>(&'a self, label: &'a str, primary: bool, msg: Message) -> Element<'a, Message> {
        let p = self.palette;
        button(
            container(text(label).font(CJK).size(self.fs())).center_x(Shrink)
        )
            .padding([6, 14])
            .on_press(msg)
            .style(move |_t, status| {
                let hov = matches!(status, button::Status::Hovered | button::Status::Pressed);
                let (bg, fg, bc) = if primary {
                    (if hov { p.accent } else { p.accent_dim }, if hov { Color::WHITE } else { p.accent_text }, p.accent)
                } else {
                    (if hov { p.bg_surface } else { Color::TRANSPARENT }, p.text_secondary, p.border)
                };
                button::Style {
                    background: Some(bg.into()),
                    text_color: fg,
                    border: Border { color: bc, width: 1.0, radius: 6.0.into() },
                    ..button::Style::default()
                }
            }).into()
    }

    fn checkbox(&self, checked: bool, msg: Message) -> Element<'_, Message> {
        let p = self.palette;
        // Checked state shows a small centered dark dot; box is smaller (14px).
        let inner: Element<Message> = if checked {
            container(boxspace(Length::Fixed(6.0), Length::Fixed(6.0)))
                .style(self.card_style(p.text_primary, Color::TRANSPARENT, 0.0, 3.0))
                .into()
        } else {
            boxspace(Length::Fixed(6.0), Length::Fixed(6.0)).into()
        };
        mouse_area(
            container(inner)
                .center_x(Length::Fixed(14.0)).center_y(Length::Fixed(14.0))
                .style(self.card_style(
                    Color::TRANSPARENT,
                    if checked { p.accent } else { p.border }, 1.0, 4.0))
        ).on_press(msg).into()
    }

    fn search_box<'a>(
        &'a self, value: &'a str, placeholder: &'a str,
        on_change: impl Fn(String) -> Message + 'a, on_clear: Message,
    ) -> Element<'a, Message> {
        let p = self.palette;
        let input = text_input(placeholder, value)
            .on_input(on_change)
            .font(CJK).size(self.fs() - 2.0)
            .padding(8)
            .width(Fill)
            .style(move |_t, _s| iced::widget::text_input::Style {
                background: p.bg_surface.into(),
                border: Border { color: p.border, width: 1.0, radius: 8.0.into() },
                icon: p.text_muted,
                placeholder: p.text_muted,
                value: p.text_primary,
                selection: p.border_accent,
            });
        // Keep a CONSTANT tree shape (always a row with a trailing slot) so the
        // text_input keeps a stable widget identity and never loses focus while
        // typing. When there's nothing to clear, the slot is an empty spacer.
        let clear: Element<Message> = if value.is_empty() {
            hspace(Length::Fixed(0.0)).into()
        } else {
            mouse_area(
                container(text("×").font(CJK).size(self.fs()).line_height(1.0).color(p.text_muted))
                    .center_x(Length::Fixed(20.0))
            ).on_press(on_clear).into()
        };
        row![input, clear]
            .spacing(4).align_y(iced::Alignment::Center).into()
    }

    fn sort_bar<'a>(
        &'a self, is_collection: bool, current: SortMode,
        on_pick: impl Fn(SortMode) -> Message + 'a,
    ) -> Element<'a, Message> {
        let p = self.palette;
        let _ = current; // default vs not no longer changes the color
        let options: Vec<SortLabel> = SortMode::all().iter()
            .map(|m| SortLabel { mode: *m, is_collection }).collect();
        let selected = SortLabel { mode: current, is_collection };
        let menu = pick_list(options, Some(selected), move |sl| on_pick(sl.mode))
            .text_size(self.fs() - 3.0)
            .font(CJK)
            .padding([4, 6])
            .style(move |_t, status| iced::widget::pick_list::Style {
                // Rule text always accent-colored; borderless, subtle hover bg.
                text_color: p.accent_text,
                placeholder_color: p.text_muted,
                handle_color: p.accent_text,
                background: match status {
                    iced::widget::pick_list::Status::Hovered
                    | iced::widget::pick_list::Status::Opened { .. } => p.bg_surface.into(),
                    _ => Color::TRANSPARENT.into(),
                },
                border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 6.0.into() },
            })
            .menu_style(move |_t| iced::widget::overlay::menu::Style {
                background: p.bg_panel.into(),          // white in light mode
                border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 10.0.into() },
                text_color: p.text_primary,
                selected_text_color: p.accent_text,
                selected_background: p.accent_dim.into(),
                shadow: iced::Shadow {
                    color: Color { a: 0.18, ..Color::BLACK },
                    offset: iced::Vector::new(0.0, 2.0),
                    blur_radius: 10.0,
                },
            });
        // "Sort:" stays put (black); only the rule dropdown opens.
        row![
            text("Sort:").font(CJK).size(self.fs() - 3.0).color(p.text_primary),
            menu,
        ].spacing(4).align_y(iced::Alignment::Center).into()
    }

    fn multi_hint(&self, multi: bool) -> Element<'_, Message> {
        let p = self.palette;
        if multi {
            let clear = mouse_area(
                container(text("Clear").font(CJK).size(self.fs() - 3.0)
                    .color(p.accent_text))
                    .center_x(Fill).padding(8)
                    .style(self.card_style(p.accent_dim, p.border_accent, 1.0, 6.0))
            ).on_press(Message::ClearSelection);
            // When collections are the multi-selected set, offer a Delete action
            // right here at the bottom (in addition to Clear).
            if self.coll_multi {
                row![
                    clear,
                    mouse_area(
                        container(text("Delete").font(CJK).size(self.fs() - 3.0)
                            .color(p.danger_text))
                            .center_x(Fill).padding(8)
                            .style(self.card_style(p.danger_bg, p.danger_text, 1.0, 6.0))
                    ).on_press(Message::DeleteSelectedCollections),
                ].spacing(6).into()
            } else {
                clear.into()
            }
        } else {
            container(column![
                text("Ctrl/Shift-click to multi-select").font(CJK)
                    .size((self.fs() - 3.0).max(11.0)).color(self.palette.text_muted),
                text("Right-click for options").font(CJK)
                    .size((self.fs() - 3.0).max(11.0)).color(self.palette.text_muted),
            ].spacing(2).align_x(iced::Alignment::Center)).center_x(Fill).padding(6).into()
        }
    }

    // ── overlays ──────────────────────────────────────────────────────────

    fn scrim<'a>(&self, modal: Element<'a, Message>) -> Element<'a, Message> {
        // Click on the dim backdrop (outside the modal) closes it. Clicks that
        // land on the modal card itself are swallowed by wrapping the card in a
        // mouse_area with a no-op press, so empty space inside the panel does
        // NOT close it.
        stack![
            mouse_area(container(boxspace(Fill, Fill))
                .style(|_| ContainerStyle {
                    background: Some(Color { a: 0.5, ..Color::BLACK }.into()),
                    ..ContainerStyle::default()
                }))
                .on_press(Message::CloseOverlay),
            container(mouse_area(modal).on_press(Message::Noop))
                .center_x(Fill).center_y(Fill),
        ].into()
    }

    fn modal_card<'a>(&self, content: Element<'a, Message>, w: f32, h: f32) -> Element<'a, Message> {
        let p = self.palette;
        container(content)
            .width(Length::Fixed(w)).height(Length::Fixed(h)).padding(18)
            .style(self.card_style(p.bg_panel, p.border, 1.0, 12.0))
            .into()
    }

    /// Modal card that fits its content height (fixed width, shrink height).
    fn modal_card_fit<'a>(&self, content: Element<'a, Message>, w: f32) -> Element<'a, Message> {
        let p = self.palette;
        container(content)
            .width(Length::Fixed(w)).height(Shrink).padding(18)
            .style(self.card_style(p.bg_panel, p.border, 1.0, 12.0))
            .into()
    }

    fn settings_overlay(&self) -> Element<'_, Message> {
        let p = self.palette;
        let mut accents = row![].spacing(5);
        for hexs in ["#4f8ef7","#7c5cbf","#2ecc71","#e67e22","#e74c3c","#1abc9c","#e91e8c","#f0c040"] {
            let c = hex(hexs);
            accents = accents.push(
                mouse_area(container(boxspace(Length::Fixed(24.0), Length::Fixed(24.0)))
                    .style(self.card_style(c, p.text_primary,
                        if self.settings.accent_hex.eq_ignore_ascii_case(hexs) { 2.0 } else { 0.0 }, 6.0)))
                    .on_press(Message::SetAccent(c))
            );
        }
        // Light/Dark segmented slider: two halves in a rounded pill; the active
        // side is filled with the accent, the inactive side is muted. Clicking
        // either side switches the theme.
        let dark = self.settings.dark_mode;
        let seg = |this: &Self, label: &'static str, active: bool, msg: Message| {
            let (bg, fg) = if active {
                (this.palette.accent, Color::WHITE)
            } else {
                (Color::TRANSPARENT, this.palette.text_muted)
            };
            mouse_area(
                container(text(label).font(CJK).size(this.fs() - 2.0).color(fg))
                    .center_x(Length::Fixed(78.0)).center_y(Length::Fixed(28.0))
                    .style(this.card_style(bg, Color::TRANSPARENT, 0.0, 14.0))
            ).on_press(msg)
        };
        let theme_slider = container(
            row![
                seg(self, "Light", !dark, Message::SetDarkMode(false)),
                seg(self, "Dark", dark, Message::SetDarkMode(true)),
            ].spacing(0)
        )
        .padding(2)
        .style(self.card_style(p.bg_input, p.border, 1.0, 16.0));

        // Logo sized off the font size (clamped), using the cached handle.
        let logo_px = (self.fs() * 1.6).clamp(20.0, 40.0);
        let content = column![
            row![self.heading("Settings", self.fs() + 1.0),
                 hspace(Fill),
                 self.icon_btn("✖", Message::CloseOverlay)]
                .align_y(iced::Alignment::Center),
            vspace(Length::Fixed(8.0)),
            container(self.heading("Appearance", self.fs())).center_x(Fill),
            row![self.body("Theme"),
                 hspace(Fill),
                 theme_slider]
                .align_y(iced::Alignment::Center),
            row![self.body("Accent color"), hspace(Fill), accents]
                .align_y(iced::Alignment::Center),
            row![self.body("Font size"), hspace(Fill),
                 self.small_action("-", Message::FontDec),
                 self.body(format!("{}", self.settings.font_size as i32)),
                 self.small_action("+", Message::FontInc)]
                .spacing(6).align_y(iced::Alignment::Center),
            vspace(Length::Fixed(8.0)),
            container(self.heading("Data", self.fs())).center_x(Fill),
            self.action_btn("Export collection data", Message::ExportData),
            self.action_btn("Import collection data", Message::ImportData),
            self.action_btn("Open data folder", Message::OpenDataFolder),
            self.action_btn("Reset panel sizes", Message::ResetPanels),
            vspace(Fill),
            // Footer: logo + version, centered. 24-ish px, scales with font size.
            container(column![
                image_widget(LOGO_HANDLE.clone())
                    .height(Length::Fixed(logo_px))
                    .content_fit(iced::ContentFit::Contain),
                self.muted(concat!("Version ", env!("CARGO_PKG_VERSION"))),
            ].spacing(6).align_x(iced::Alignment::Center))
                .center_x(Fill),
        ].spacing(10);
        self.scrim(self.modal_card(content.into(), 420.0, 600.0))
    }

    fn icon_picker_overlay(&self) -> Element<'_, Message> {
        // Curated: one representative emoji per category. Car added to row one.
        let icons = [
            "🚗","📁","🎧","🖊","📷","🎮","📚","⌚",
            "💍","🎸","🎨","🏆","🎯","🔬","🚀","🌿",
            "🍷","⚽","🎲","💎","🖥","📻","🎺","🎻",
            "🏺","💰","🔑","🔧","🔭","🎁","🚲","🌱",
            "🐾","🦋","🌊","🏠","🎭","🍵","🌍","🐶",
            "📦",
        ];
        let mut grid = column![].spacing(4);
        let p = self.palette;
        for chunk in icons.chunks(8) {
            let mut r = row![].spacing(4);
            for ico in chunk {
                r = r.push(
                    button(
                        container(self.emoji_text(*ico, 22.0))
                            .center_x(Length::Fixed(40.0)).center_y(Length::Fixed(40.0))
                    )
                    .padding(0)
                    .on_press(Message::IconPicked((*ico).to_string()))
                    .style(move |_t, status| {
                        let hov = matches!(status, button::Status::Hovered | button::Status::Pressed);
                        button::Style {
                            background: Some(if hov { p.accent_dim } else { p.bg_surface }.into()),
                            text_color: p.text_primary,
                            border: Border {
                                color: if hov { p.accent } else { p.border },
                                width: 1.0, radius: 6.0.into(),
                            },
                            ..button::Style::default()
                        }
                    })
                );
            }
            grid = grid.push(r);
        }
        let content = column![
            row![self.heading("Choose icon", self.fs()),
                 hspace(Fill), self.icon_btn("✖", Message::CloseOverlay)]
                .align_y(iced::Alignment::Center),
            scrollable(grid)
                .height(Length::Fixed(300.0))
                .width(Fill)
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new().width(8).scroller_width(8).margin(2))),
        ].spacing(10);
        self.scrim(self.modal_card(content.into(), 412.0, 380.0))
    }

    fn template_picker_overlay(&self) -> Element<'_, Message> {
        let p = self.palette;
        let mut list = column![].spacing(6);
        if self.data.templates.is_empty() {
            list = list.push(self.muted("No templates yet. Use 'Save as template' on an item."));
        } else {
            for t in &self.data.templates {
                let renaming = self.template_rename.as_ref().map(|(id, _)| id == &t.id).unwrap_or(false);
                let rename_el: Element<Message> = if renaming {
                    let ed = &self.template_rename.as_ref().unwrap().1;
                    row![
                        text_editor(ed).on_action(Message::TemplateRenameEdited)
                            .key_binding(editor_keys_basic)
                            .font(CJK).size(self.fs() - 2.0).padding(4)
                            .style(self.editor_style(true)),
                        self.small_action("OK", Message::CommitTemplateRename),
                    ].spacing(4).into()
                } else {
                    mouse_area(self.muted("Rename..."))
                        .on_press(Message::StartTemplateRename(t.id.clone())).into()
                };
                list = list.push(column![
                    row![
                        mouse_area(container(column![
                            self.body(t.name.clone()),
                            self.muted(t.field_labels.join(", ")),
                        ].spacing(2)).padding(8).width(Fill)
                            .style(self.card_style(p.bg_surface, p.border, 1.0, 8.0)))
                            .on_press(Message::ApplyTemplate(t.id.clone())),
                        self.danger_btn("×", Message::DeleteTemplate(t.id.clone())),
                    ].spacing(6),
                    rename_el,
                ].spacing(4));
            }
        }
        let content = column![
            row![self.heading("Field Templates", self.fs()),
                 hspace(Fill), self.icon_btn("✖", Message::CloseOverlay)]
                .align_y(iced::Alignment::Center),
            scrollable(list).height(Fill),
        ].spacing(10);
        self.scrim(self.modal_card(content.into(), 360.0, 380.0))
    }

    fn lightbox_overlay(&self) -> Element<'_, Message> {
        // The built-in image Viewer handles scroll-to-zoom (toward the cursor),
        // drag-to-pan, and clamps both scale and pan internally. Defaults give
        // min 0.25x / max 10x; we keep max 10x and set min 1.0 so the image
        // can't shrink below fit. Because the Viewer captures mouse events,
        // clicking the image no longer closes the lightbox.
        let img: Element<Message> = match &self.lightbox_handle {
            Some(h) => iced::widget::image::viewer(h.clone())
                .min_scale(1.0)
                .max_scale(10.0)
                .width(Fill)
                .height(Fill)
                .into(),
            None => boxspace(Fill, Fill).into(),
        };

        let controls: Element<Message> = if self.lightbox_count > 1 {
            row![
                self.small_action("‹", Message::LightboxPrev),
                hspace(Fill),
                self.body(format!("{} / {}", self.lightbox_index + 1, self.lightbox_count)),
                hspace(Fill),
                self.small_action("›", Message::LightboxNext),
            ].align_y(iced::Alignment::Center).into()
        } else {
            vspace(Shrink).into()
        };

        let hint = container(
            self.muted("Scroll to zoom · drag to pan · Esc to close")
        ).center_x(Fill);

        // Red ✕ close button (distinct from the neutral icon_btn used
        // elsewhere). Uses a CJK-font glyph rather than the color-emoji ✖, since
        // color-emoji glyphs ignore text_color and would stay black.
        let p = self.palette;
        let close = button(
            container(text("✕").font(CJK_BOLD).size(self.fs() + 2.0).color(p.danger_text))
                .center_x(Length::Fixed(30.0)).center_y(Length::Fixed(30.0))
        )
            .padding(0)
            .on_press(Message::CloseOverlay)
            .style(move |_t, status| {
                let hov = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: Some(if hov { p.danger_bg } else { Color::TRANSPARENT }.into()),
                    text_color: p.danger_text,
                    border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 8.0.into() },
                    ..button::Style::default()
                }
            });

        let body = column![
            row![hspace(Fill), close],
            img,
            hint,
            controls,
        ].spacing(8).padding(16);

        stack![
            mouse_area(container(boxspace(Fill, Fill))
                .style(|_| ContainerStyle {
                    background: Some(Color { a: 0.8, ..Color::BLACK }.into()),
                    ..ContainerStyle::default()
                }))
                .on_press(Message::CloseOverlay),
            container(body).width(Fill).height(Fill),
        ].into()
    }

    fn data_recovered_overlay(&self) -> Element<'_, Message> {
        // Shown once at startup when the previous data.json couldn't be parsed.
        // The point is to prevent the user concluding their collection vanished:
        // explain what happened and where the salvageable copy is.
        let path_line: Element<'_, Message> = match &self.corrupt_backup {
            Some(p) => self.muted(p.display().to_string()).into(),
            None => self.muted("a backup copy was saved next to your data file").into(),
        };
        let content = column![
            self.heading("Couldn't read your saved data", self.fs() + 2.0),
            self.body(
                "Your previous data file existed but couldn't be opened, so it may \
                 have been corrupted. To avoid overwriting it, a copy was preserved \
                 and the app started with an empty collection."
            ),
            self.body("The preserved copy is here:"),
            path_line,
            self.muted(
                "If you can fix or restore that file, replace your data file with it \
                 and restart. Until then, new changes will be saved fresh."
            ),
            row![hspace(Fill), self.dialog_btn("Got it", true, Message::CloseOverlay)],
        ].spacing(12);
        self.scrim(self.modal_card_fit(content.into(), 460.0))
    }

    fn name_input_overlay(&self) -> Element<'_, Message> {
        let content = column![
            self.body(self.name_title.clone()),
            text_editor(&self.name_value)
                .on_action(Message::NameValueEdited)
                .key_binding(editor_keys_basic)
                .font(CJK).size(self.fs()).padding(8)
                .min_height(self.fs() * 1.3 + 16.0)
                .max_height(self.fs() * 1.3 * 3.0 + 16.0)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                .style(self.editor_style_strong()),
            row![hspace(Fill),
                 self.dialog_btn("Cancel", false, Message::CloseOverlay),
                 self.dialog_btn("OK", true, Message::NameAccepted)]
                .spacing(8),
        ].spacing(12);
        // Card height fits the content (title + up-to-3-line editor + buttons).
        self.scrim(self.modal_card_fit(content.into(), 340.0))
    }

    fn enlarge_overlay(&self) -> Element<'_, Message> {
        // Big editable view of a single detail field. Bound to the same editor
        // content as the panel, so edits flow back and are saved on close.
        let (title, content, on_action): (&str, &EdContent, fn(text_editor::Action) -> Message) =
            match self.enlarge_target {
                Some(EnlargeTarget::Name) =>
                    ("Name", &self.editors.name, Message::NameEdited as fn(_) -> _),
                Some(EnlargeTarget::Desc) =>
                    ("Description", &self.editors.desc, Message::DescEdited as fn(_) -> _),
                Some(EnlargeTarget::Field(i)) => {
                    if let Some((_, _, val)) = self.editors.fields.get(i) {
                        // Per-index closures can't be fn pointers; handle below.
                        return self.enlarge_field_overlay(i, val);
                    }
                    ("Field", &self.editors.name, Message::NameEdited as fn(_) -> _)
                }
                None => ("Field", &self.editors.name, Message::NameEdited as fn(_) -> _),
            };

        let body = column![
            row![self.heading(title, self.fs() + 1.0), hspace(Fill),
                 self.icon_btn("✖", Message::CloseOverlay)]
                .align_y(iced::Alignment::Center),
            text_editor(content)
                .on_action(on_action)
                .key_binding(editor_keys_basic)
                .font(CJK).size(self.fs())
                .height(Fill)
                .padding(10)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                .style(self.editor_style_strong()),
            row![hspace(Fill), self.dialog_btn("Done", true, Message::CloseOverlay)],
        ].spacing(12);
        self.scrim(self.modal_card(body.into(), 560.0, 460.0))
    }

    fn enlarge_field_overlay<'a>(&'a self, i: usize, content: &'a EdContent) -> Element<'a, Message> {
        let body = column![
            row![self.heading("Field", self.fs() + 1.0), hspace(Fill),
                 self.icon_btn("✖", Message::CloseOverlay)]
                .align_y(iced::Alignment::Center),
            text_editor(content)
                .on_action(move |a| Message::FieldValueEdited(i, a))
                .key_binding(editor_keys_basic)
                .font(CJK).size(self.fs())
                .height(Fill)
                .padding(10)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                .style(self.editor_style_strong()),
            row![hspace(Fill), self.dialog_btn("Done", true, Message::CloseOverlay)],
        ].spacing(12);
        self.scrim(self.modal_card(body.into(), 560.0, 460.0))
    }

    fn context_menu_overlay(&self) -> Element<'_, Message> {
        let p = self.palette;
        let primary = if self.ctx_is_collection {
            if self.ctx_multi { "Duplicate all checked" } else { "Duplicate collection" }
        } else if self.ctx_multi { "Duplicate all checked" } else { "Duplicate item" };
        let danger = if self.ctx_is_collection {
            if self.ctx_multi { "Delete all checked" } else { "Delete collection" }
        } else if self.ctx_multi { "Delete all checked" } else { "Delete item" };

        let mut menu = column![].spacing(0);
        if !self.ctx_multi {
            menu = menu.push(self.menu_item("Rename", false, Message::CtxRename));
        }
        menu = menu.push(self.menu_item(primary, false, Message::CtxPrimary));
        menu = menu.push(self.menu_item(danger, true, Message::CtxDanger));

        let menu_w = 180.0f32;
        let card = container(menu).width(Length::Fixed(menu_w)).padding(4)
            .style(self.card_style(p.bg_panel, p.border, 1.0, 10.0));

        // Anchor the card at the cursor, clamped so it stays fully on-screen on
        // both axes. Use the live window size (not a fixed nominal) so the menu
        // stays at the cursor when the window is resized or maximized. Estimate
        // the menu height from its row count.
        let win_w = self.nominal_window_w();
        let win_h = self.nominal_window_h();
        let rows = if self.ctx_multi { 2.0 } else { 3.0 };
        let menu_h = rows * (self.fs() + 16.0) + 8.0; // item height ≈ fs+padding
        let left = self.ctx_pos.x.max(0.0).min((win_w - menu_w - 8.0).max(0.0));
        let top = self.ctx_pos.y.max(0.0).min((win_h - menu_h - 8.0).max(0.0));
        // Wrap just the menu CARD in a no-op mouse_area so clicking the menu
        // itself doesn't dismiss it (its buttons still fire). The card is then
        // positioned at the cursor via leading spacers.
        let card_guarded = mouse_area(card).on_press(Message::Noop);
        let positioned = container(
            column![
                vspace(Length::Fixed(top)),
                row![hspace(Length::Fixed(left)), card_guarded],
            ]
        ).width(Fill).height(Fill);

        // A transparent full-area backdrop sits BEHIND the positioned card and
        // closes the menu on any click that doesn't land on the card — including
        // clicks on panel buttons and empty space. Because the positioned layer
        // is mostly empty (only the small card is interactive), backdrop clicks
        // outside the card reach the backdrop and dismiss the menu. Right-press
        // also closes it; each row's on_right_press then reopens the menu at the
        // new target. Esc and selecting a row dismiss it too.
        stack![
            mouse_area(container(boxspace(Fill, Fill)))
                .on_press(Message::CloseOverlay)
                .on_right_press(Message::CloseOverlay),
            positioned,
        ].into()
    }

    fn menu_item<'a>(&'a self, label: &'a str, danger: bool, msg: Message) -> Element<'a, Message> {
        let p = self.palette;
        button(text(label).font(CJK).size(self.fs() - 2.0))
            .width(Fill).padding([8, 12])
            .on_press(msg)
            .style(move |_t, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                let bg = if hovered {
                    if danger { p.danger_bg } else { p.bg_surface }
                } else { Color::TRANSPARENT };
                let fg = if danger { p.danger_text } else { p.text_primary };
                button::Style {
                    background: Some(bg.into()),
                    text_color: fg,
                    border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 6.0.into() },
                    ..button::Style::default()
                }
            }).into()
    }
}

// pick_list needs a Display + Clone + Eq type carrying the panel context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SortLabel { mode: SortMode, is_collection: bool }

impl std::fmt::Display for SortLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.mode.label(self.is_collection))
    }
}
