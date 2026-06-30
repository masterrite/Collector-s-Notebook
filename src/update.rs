// src/update.rs — included into main.rs. The `update` method + helpers.
// Contains all logic that was spread across ~40 Slint callbacks.

// Limit a numeric editor to `max` digits and digits-only, returning the kept text.
fn clamp_numeric(content: &EdContent, max: usize) -> String {
    let raw = content.text();
    raw.chars().filter(|c| c.is_ascii_digit()).take(max).collect()
}

// Rebuild a numeric date editor from `kept` ONLY if its current text differs,
// and re-home the caret to the end of the kept text. Rebuilding with
// `with_text` resets the cursor to position 0, so without the explicit move a
// rejected keystroke mid-string would yank the caret to the front. Returning
// early when nothing changed avoids needless rebuilds (and the caret jump) on
// every valid keystroke.
fn apply_numeric(editor: &mut EdContent, kept: &str) {
    if kept == editor.text().trim_end_matches('\n') {
        return;
    }
    *editor = EdContent::with_text(kept);
    // Move the caret to the end of the (short, single-line) field so typing
    // continues where the user expects.
    editor.perform(EdAction::Move(iced::widget::text_editor::Motion::DocumentEnd));
}

impl App {
    /// Pull the two split ratios out of the pane_grid layout and store them in
    /// settings. Layout shape is [Left | [Mid | Right]].
    fn sync_pane_ratios(&mut self) {
        use iced::widget::pane_grid::Node;
        // Extract into locals first so the immutable borrow from layout() ends
        // before we mutably write to self.settings.
        let (mut left, mut mid) = (None, None);
        if let Node::Split { ratio, b, .. } = self.panes.layout() {
            left = Some(*ratio);
            if let Node::Split { ratio: m, .. } = b.as_ref() {
                mid = Some(*m);
            }
        }
        if let Some(l) = left { self.settings.left_ratio = l.clamp(0.08, 0.33); }
        if let Some(m) = mid { self.settings.mid_ratio = m.clamp(0.12, 0.7); }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Noop => Task::none(),
            Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
                use iced::widget::pane_grid::Node;
                // Figure out which divider is being dragged so we can apply the
                // right cap: the outer split is the left|rest divider (cap 0.33),
                // the inner split is the mid|right divider (cap 0.7).
                let outer_split = if let Node::Split { id, .. } = self.panes.layout() {
                    Some(*id)
                } else {
                    None
                };
                let ratio = if Some(split) == outer_split {
                    ratio.clamp(0.08, 0.33)
                } else {
                    ratio.clamp(0.12, 0.7)
                };
                self.panes.resize(split, ratio);
                self.sync_pane_ratios();
                self.persist_settings();
                Task::none()
            }
            Message::CloseOverlay => {
                // If we were enlarging a field, persist the edits before closing.
                if self.overlay == Overlay::Enlarge {
                    self.flush_editors_to_item();
                    self.persist();
                    self.enlarge_target = None;
                }
                // Free the decoded full-resolution lightbox image (can be tens of
                // MB) when leaving the lightbox.
                if self.overlay == Overlay::Lightbox {
                    self.lightbox_handle = None;
                }
                self.overlay = Overlay::None;
                self.template_rename = None;
                Task::none()
            }

            // ── selection ────────────────────────────────────────────────
            Message::SelectCollection(idx) => {
                // A left-click while the context menu is open should dismiss it
                // and act on the row in one go.
                if self.overlay == Overlay::ContextMenu { self.overlay = Overlay::None; }
                let (ctrl, shift) = (self.modifiers.command(), self.modifiers.shift());
                self.select_collection(idx, ctrl, shift);
                Task::none()
            }
            Message::SelectItem(idx) => {
                if self.overlay == Overlay::ContextMenu { self.overlay = Overlay::None; }
                let (ctrl, shift) = (self.modifiers.command(), self.modifiers.shift());
                self.select_item(idx, ctrl, shift);
                Task::none()
            }
            Message::ModifiersChanged(m) => { self.modifiers = m; Task::none() }
            Message::CursorMoved(p) => { self.cursor = p; Task::none() }
            Message::WindowResized(w, h) => { self.window_w = w; self.window_h = h; Task::none() }
            Message::EscapePressed => {
                if self.overlay != Overlay::None {
                    if self.overlay == Overlay::Lightbox {
                        self.lightbox_handle = None;
                    }
                    self.overlay = Overlay::None;
                    self.template_rename = None;
                } else if !self.item_search.is_empty() || !self.coll_search.is_empty() {
                    // Esc first cancels any active search.
                    self.item_search.clear();
                    self.coll_search.clear();
                    self.rebuild_item_checked();
                } else {
                    self.rebuild_coll_checked();
                    self.rebuild_item_checked();
                    self.anchor_coll = None;
                    self.anchor_item = None;
                }
                Task::none()
            }
            Message::ToggleCollCheck(i) => {
                if let Some(c) = self.coll_checked.get_mut(i) { *c = !*c; }
                self.coll_multi = self.coll_checked.iter().any(|c| *c);
                Task::none()
            }
            Message::ToggleItemCheck(i) => {
                if let Some(c) = self.item_checked.get_mut(i) { *c = !*c; }
                self.item_multi = self.item_checked.iter().any(|c| *c);
                Task::none()
            }
            Message::ClearSelection => {
                self.rebuild_coll_checked();
                self.rebuild_item_checked();
                self.anchor_coll = None;
                self.anchor_item = None;
                Task::none()
            }

            // ── collection CRUD ──────────────────────────────────────────
            Message::NewCollection => {
                let icons = ["📁","📂","🎧","✒","📷","🎮","📚","⌚","💍","🎸"];
                let n = self.data.collections.len();
                let order = self.data.next_coll_order();
                self.data.collections.push(Collection {
                    id: new_id(),
                    name: format!("New Collection {}", n + 1),
                    icon: icons[n % icons.len()].into(),
                    order,
                });
                self.persist();
                self.rebuild_coll_checked();
                Task::none()
            }
            Message::DeleteCollection(idx) => {
                if let Some(c) = self.data.collections.get(idx).cloned() {
                    for it in self.data.items.iter().filter(|i| i.collection_id == c.id) {
                        for p in &it.photos { image_util::delete_photo_files(p); self.evict_thumb(p); }
                    }
                    self.data.items.retain(|i| i.collection_id != c.id);
                    self.data.collections.remove(idx);
                    if self.sel_coll.as_deref() == Some(&c.id) {
                        self.sel_coll = None;
                        self.sel_item = None;
                        self.is_editing = false;
                        self.reload_editors();
                    }
                    self.persist();
                }
                self.rebuild_coll_checked();
                self.rebuild_item_checked();
                self.overlay = Overlay::None;
                Task::none()
            }
            Message::DuplicateCollection(idx) => {
                self.duplicate_collection_by_index(idx);
                self.overlay = Overlay::None;
                Task::none()
            }
            Message::DeleteSelectedCollections => {
                let ids: Vec<String> = self.coll_checked.iter().enumerate()
                    .filter(|(_, c)| **c)
                    .filter_map(|(i, _)| self.data.collections.get(i).map(|c| c.id.clone()))
                    .collect();
                for cid in &ids {
                    for it in self.data.items.iter().filter(|i| &i.collection_id == cid) {
                        for p in &it.photos { image_util::delete_photo_files(p); self.evict_thumb(p); }
                    }
                }
                self.data.items.retain(|i| !ids.contains(&i.collection_id));
                self.data.collections.retain(|c| !ids.contains(&c.id));
                if self.sel_coll.as_ref().map(|s| ids.contains(s)).unwrap_or(false) {
                    self.sel_coll = None;
                    self.sel_item = None;
                    self.reload_editors();
                }
                self.persist();
                self.rebuild_coll_checked();
                self.rebuild_item_checked();
                self.overlay = Overlay::None;
                Task::none()
            }
            Message::DuplicateSelectedCollections => {
                let ids: Vec<String> = self.coll_checked.iter().enumerate()
                    .filter(|(_, c)| **c)
                    .filter_map(|(i, _)| self.data.collections.get(i).map(|c| c.id.clone()))
                    .collect();
                for cid in ids {
                    if let Some(pos) = self.data.collections.iter().position(|c| c.id == cid) {
                        self.duplicate_collection_by_index(pos);
                    }
                }
                self.rebuild_coll_checked();
                self.overlay = Overlay::None;
                Task::none()
            }

            // ── item CRUD ────────────────────────────────────────────────
            Message::NewItem => {
                let coll_id = match &self.sel_coll { Some(c) => c.clone(), None => return Task::none() };
                let item = Item {
                    id: new_id(),
                    collection_id: coll_id,
                    name: "New Item".into(),
                    custom_fields: default_fields(),
                    ..Default::default()
                };
                let nid = item.id.clone();
                self.data.items.push(item);
                self.persist();
                self.sel_item = Some(nid);
                self.is_editing = true;
                self.reload_editors();
                self.rebuild_item_checked();
                Task::none()
            }
            Message::DeleteItem => {
                if let Some(id) = self.sel_item.clone() {
                    if let Some(it) = self.data.items.iter().find(|i| i.id == id) {
                        for p in &it.photos { image_util::delete_photo_files(p); self.evict_thumb(p); }
                    }
                    self.data.items.retain(|i| i.id != id);
                    self.sel_item = None;
                    self.is_editing = false;
                    self.reload_editors();
                    self.persist();
                }
                self.rebuild_item_checked();
                self.overlay = Overlay::None;
                Task::none()
            }
            Message::DuplicateItem => {
                if let Some(id) = self.sel_item.clone() {
                    self.duplicate_item_by_id(&id);
                }
                self.overlay = Overlay::None;
                self.rebuild_item_checked();
                Task::none()
            }
            Message::DeleteSelectedItems => {
                let view = self.current_items();
                let ids: Vec<String> = view.iter().enumerate()
                    .filter(|(i, _)| self.item_checked.get(*i).copied().unwrap_or(false))
                    .map(|(_, it)| it.id.clone())
                    .collect();
                for id in &ids {
                    if let Some(it) = self.data.items.iter().find(|i| &i.id == id) {
                        for p in &it.photos { image_util::delete_photo_files(p); self.evict_thumb(p); }
                    }
                }
                self.data.items.retain(|i| !ids.contains(&i.id));
                if self.sel_item.as_ref().map(|s| ids.contains(s)).unwrap_or(false) {
                    self.sel_item = None;
                    self.reload_editors();
                }
                self.persist();
                self.rebuild_item_checked();
                self.overlay = Overlay::None;
                Task::none()
            }
            Message::DuplicateSelectedItems => {
                let view = self.current_items();
                let ids: Vec<String> = view.iter().enumerate()
                    .filter(|(i, _)| self.item_checked.get(*i).copied().unwrap_or(false))
                    .map(|(_, it)| it.id.clone())
                    .collect();
                for id in ids {
                    self.duplicate_item_by_id(&id);
                }
                self.rebuild_item_checked();
                self.overlay = Overlay::None;
                Task::none()
            }

            // ── editing ──────────────────────────────────────────────────
            Message::ToggleEdit => {
                if self.is_editing {
                    self.flush_editors_to_item();
                    self.persist();
                    self.is_editing = false;
                    self.status = "Saved".into();
                    self.reload_editors();
                } else {
                    self.is_editing = true;
                    self.status.clear();
                }
                Task::none()
            }
            Message::NameEdited(a) => { self.editors.name.perform(a); Task::none() }
            Message::DescEdited(a) => { self.editors.desc.perform(a); Task::none() }
            Message::YearEdited(a) => {
                self.editors.year.perform(a);
                let kept = clamp_numeric(&self.editors.year, 4);
                apply_numeric(&mut self.editors.year, &kept);
                Task::none()
            }
            Message::MonthEdited(a) => {
                self.editors.month.perform(a);
                let kept = clamp_numeric(&self.editors.month, 2);
                apply_numeric(&mut self.editors.month, &kept);
                Task::none()
            }
            Message::DayEdited(a) => {
                self.editors.day.perform(a);
                let kept = clamp_numeric(&self.editors.day, 2);
                apply_numeric(&mut self.editors.day, &kept);
                Task::none()
            }
            Message::FieldLabelEdited(i, a) => {
                if let Some((_, lbl, _)) = self.editors.fields.get_mut(i) { lbl.perform(a); }
                Task::none()
            }
            Message::FieldValueEdited(i, a) => {
                if let Some((_, _, val)) = self.editors.fields.get_mut(i) { val.perform(a); }
                Task::none()
            }
            // Tab / Shift+Tab from a focused detail editor: focus the next/prev
            // detail editor by id, keeping cycling within the right-hand panel.
            Message::FocusDetail(n) => iced::widget::operation::focus(detail_id(n)),
            // Ctrl+S / Cmd+S: persist the in-progress edits without leaving edit
            // mode. No-op when not editing so the shortcut is harmless elsewhere.
            Message::SaveShortcut => {
                if self.is_editing {
                    self.flush_editors_to_item();
                    self.persist();
                    self.status = "Saved".into();
                }
                Task::none()
            }
            Message::AddCustomField => {
                if let Some(id) = self.sel_item.clone() {
                    // Save in-progress edits first; reload_editors below would
                    // otherwise discard any unsaved name/desc/field text.
                    self.flush_editors_to_item();
                    if let Some(it) = self.data.items.iter_mut().find(|i| i.id == id) {
                        it.custom_fields.push(CustomField {
                            id: new_id(), label: "NEW FIELD".into(), value: String::new(),
                        });
                    }
                    self.persist();
                    self.reload_editors();
                }
                Task::none()
            }
            Message::DeleteCustomField(fid) => {
                if let Some(id) = self.sel_item.clone() {
                    // Persist any in-progress edits first so they aren't lost.
                    self.flush_editors_to_item();
                    if let Some(it) = self.data.items.iter_mut().find(|i| i.id == id) {
                        it.custom_fields.retain(|f| f.id != fid);
                    }
                    self.persist();
                    self.reload_editors();
                }
                Task::none()
            }

            // ── search ───────────────────────────────────────────────────
            Message::ItemSearchChanged(q) => {
                self.item_search = q;
                // The filtered list changes, so the multi-select checkbox vector
                // must track the new length. Keep the current single selection
                // and focus intact (the stable text_input Id preserves focus).
                self.rebuild_item_checked();
                Task::none()
            }
            Message::CollSearchChanged(q) => { self.coll_search = q; Task::none() }
            Message::ClearItemSearch => {
                self.item_search.clear();
                self.rebuild_item_checked();
                Task::none()
            }
            Message::ClearCollSearch => { self.coll_search.clear(); Task::none() }

            // ── sort ─────────────────────────────────────────────────────
            Message::SetCollSort(mode) => {
                self.settings.coll_sort = mode;
                self.persist_settings();
                let sel = self.sel_coll.clone();
                // Capture which collections are checked (by id) so the
                // multi-selection survives the reorder instead of being wiped.
                let checked_ids: std::collections::HashSet<String> = self
                    .coll_checked.iter().enumerate()
                    .filter(|(_, c)| **c)
                    .filter_map(|(i, _)| self.data.collections.get(i).map(|c| c.id.clone()))
                    .collect();
                sort_collections(&mut self.data, mode);
                // Rebuild the parallel checkbox vector in the NEW order.
                self.coll_checked = self.data.collections.iter()
                    .map(|c| checked_ids.contains(&c.id))
                    .collect();
                self.coll_multi = self.coll_checked.iter().any(|c| *c);
                self.anchor_coll = sel.as_ref()
                    .and_then(|id| self.data.collections.iter().position(|c| &c.id == id));
                Task::none()
            }
            Message::SetItemSort(mode) => {
                self.settings.item_sort = mode;
                self.persist_settings();
                // Capture checked item ids against the CURRENT (pre-sort) view,
                // then rebuild the vector against the new view order so the
                // selection persists across the sort.
                let checked_ids: std::collections::HashSet<String> = {
                    let view = self.current_items();
                    view.iter().enumerate()
                        .filter(|(i, _)| self.item_checked.get(*i).copied().unwrap_or(false))
                        .map(|(_, it)| it.id.clone())
                        .collect()
                };
                let sel = self.sel_item.clone();
                let new_ids: Vec<String> =
                    self.current_items().iter().map(|i| i.id.clone()).collect();
                self.item_checked = new_ids.iter()
                    .map(|id| checked_ids.contains(id))
                    .collect();
                self.item_multi = self.item_checked.iter().any(|c| *c);
                self.anchor_item = sel.as_ref()
                    .and_then(|id| new_ids.iter().position(|i| i == id));
                Task::none()
            }

            // ── photos ───────────────────────────────────────────────────
            Message::PickPhotos => {
                Task::perform(pick_photos_dialog(), |paths| match paths {
                    Some(p) if !p.is_empty() => Message::PhotosPicked(p),
                    _ => Message::Noop,
                })
            }
            Message::PhotosPicked(paths) => {
                if let Some(id) = self.sel_item.clone() {
                    self.flush_editors_to_item();
                    if let Some(it) = self.data.items.iter_mut().find(|i| i.id == id) {
                        for p in paths { it.photos.push(p); }
                    }
                    self.persist();
                    self.reload_editors();
                }
                Task::none()
            }
            Message::RemovePhoto(idx) => {
                if let Some(id) = self.sel_item.clone() {
                    let mut removed_name: Option<String> = None;
                    if let Some(it) = self.data.items.iter_mut().find(|i| i.id == id) {
                        if idx < it.photos.len() {
                            let removed = it.photos.remove(idx);
                            image_util::delete_photo_files(&removed);
                            removed_name = Some(removed);
                        }
                    }
                    // Evict after the mutable borrow of self.data has ended.
                    if let Some(name) = removed_name { self.evict_thumb(&name); }
                    self.persist();
                    self.reload_editors();
                }
                Task::none()
            }
            Message::SetMainPhoto(idx) => {
                if let Some(id) = self.sel_item.clone() {
                    if let Some(it) = self.data.items.iter_mut().find(|i| i.id == id) {
                        if idx < it.photos.len() && idx != 0 {
                            let chosen = it.photos.remove(idx);
                            it.photos.insert(0, chosen);
                        }
                    }
                    self.persist();
                    self.reload_editors();
                }
                Task::none()
            }
            Message::OpenLightbox(idx) => {
                // Pull what we need out of the borrow before mutating self.
                let photos: Vec<String> = self.selected_item()
                    .map(|it| it.photos.clone()).unwrap_or_default();
                if !photos.is_empty() {
                    self.lightbox_count = photos.len();
                    self.lightbox_index = idx.min(self.lightbox_count.saturating_sub(1));
                    if let Some(p) = photos.get(self.lightbox_index) {
                        self.lightbox_handle = image_util::full_handle(p);
                    }
                    self.overlay = Overlay::Lightbox;
                }
                Task::none()
            }
            Message::LightboxPrev => {
                if self.overlay == Overlay::Lightbox { self.lightbox_step(-1); }
                Task::none()
            }
            Message::LightboxNext => {
                if self.overlay == Overlay::Lightbox { self.lightbox_step(1); }
                Task::none()
            }
            Message::LightboxArrowPrev => {
                if self.overlay == Overlay::Lightbox { self.lightbox_step(-1); }
                Task::none()
            }
            Message::LightboxArrowNext => {
                if self.overlay == Overlay::Lightbox { self.lightbox_step(1); }
                Task::none()
            }

            // ── templates ────────────────────────────────────────────────
            Message::OpenTemplatePicker => { self.overlay = Overlay::TemplatePicker; Task::none() }
            Message::ApplyTemplate(tid) => {
                if let Some(id) = self.sel_item.clone() {
                    let labels = self.data.templates.iter()
                        .find(|t| t.id == tid).map(|t| t.field_labels.clone()).unwrap_or_default();
                    if let Some(it) = self.data.items.iter_mut().find(|i| i.id == id) {
                        it.custom_fields = labels.into_iter()
                            .map(|label| CustomField { id: new_id(), label, value: String::new() })
                            .collect();
                    }
                    self.persist();
                    self.reload_editors();
                    self.status = "Loaded template".into();
                }
                self.overlay = Overlay::None;
                Task::none()
            }
            Message::DeleteTemplate(tid) => {
                self.data.templates.retain(|t| t.id != tid);
                self.persist();
                Task::none()
            }
            Message::StartTemplateRename(tid) => {
                let cur = self.data.templates.iter().find(|t| t.id == tid)
                    .map(|t| t.name.clone()).unwrap_or_default();
                self.template_rename = Some((tid, EdContent::with_text(&cur)));
                Task::none()
            }
            Message::TemplateRenameEdited(a) => {
                if let Some((_, ed)) = self.template_rename.as_mut() { ed.perform(a); }
                Task::none()
            }
            Message::CommitTemplateRename => {
                if let Some((tid, ed)) = self.template_rename.take() {
                    let name = ed.text().trim().to_string();
                    if !name.is_empty() {
                        if let Some(t) = self.data.templates.iter_mut().find(|t| t.id == tid) {
                            t.name = name;
                        }
                        self.persist();
                    }
                }
                Task::none()
            }

            // ── icon picker ──────────────────────────────────────────────
            Message::OpenIconPicker(idx) => {
                self.icon_target = Some(idx);
                self.overlay = Overlay::IconPicker;
                Task::none()
            }
            Message::IconPicked(ico) => {
                if let Some(idx) = self.icon_target {
                    if let Some(c) = self.data.collections.get_mut(idx) { c.icon = ico; }
                    self.persist();
                }
                self.overlay = Overlay::None;
                Task::none()
            }

            // ── name modal ───────────────────────────────────────────────
            Message::OpenRenameCollection(idx) => {
                let cur = self.data.collections.get(idx).map(|c| c.name.clone()).unwrap_or_default();
                self.name_title = "Rename collection".into();
                self.name_value = EdContent::with_text(&cur);
                self.name_purpose = Some(NamePurpose::RenameColl(idx));
                self.overlay = Overlay::NameInput;
                Task::none()
            }
            Message::OpenRenameItem(idx) => {
                let cur = self.current_items().get(idx).map(|i| i.name.clone()).unwrap_or_default();
                self.name_title = "Rename item".into();
                self.name_value = EdContent::with_text(&cur);
                self.name_purpose = Some(NamePurpose::RenameItem(idx));
                self.overlay = Overlay::NameInput;
                Task::none()
            }
            Message::OpenSaveTemplate => {
                self.name_title = "Save as template".into();
                self.name_value = EdContent::new();
                self.name_purpose = Some(NamePurpose::SaveTemplate);
                self.overlay = Overlay::NameInput;
                Task::none()
            }
            Message::OpenEnlarge(target) => {
                // Enlarge opens a big editable view of a single field. Editing is
                // turned on so the changes are live and get flushed/persisted the
                // same way as in-panel edits.
                self.enlarge_target = Some(target);
                self.is_editing = true;
                self.overlay = Overlay::Enlarge;
                Task::none()
            }
            Message::HoverColl(i) => { self.hover_coll = i; Task::none() }
            Message::HoverItem(i) => { self.hover_item = i; Task::none() }
            Message::HoverMainPhoto(on) => { self.hover_main_photo = on; Task::none() }
            Message::NameValueEdited(a) => { self.name_value.perform(a); Task::none() }
            Message::NameAccepted => {
                let txt = self.name_value.text().trim().to_string();
                match self.name_purpose.take() {
                    Some(NamePurpose::RenameColl(idx)) if !txt.is_empty() => {
                        if let Some(c) = self.data.collections.get_mut(idx) { c.name = txt; }
                        self.persist();
                        self.rebuild_coll_checked();
                    }
                    Some(NamePurpose::RenameItem(idx)) if !txt.is_empty() => {
                        if let Some(target) = self.current_items().get(idx).map(|i| i.id.clone()) {
                            if let Some(it) = self.data.items.iter_mut().find(|i| i.id == target) {
                                it.name = txt;
                            }
                            self.persist();
                            self.reload_editors();
                        }
                    }
                    Some(NamePurpose::SaveTemplate) => {
                        if let Some(id) = self.sel_item.clone() {
                            let labels: Vec<String> = self.data.items.iter()
                                .find(|i| i.id == id)
                                .map(|i| i.custom_fields.iter().map(|f| f.label.clone()).collect())
                                .unwrap_or_default();
                            if !labels.is_empty() {
                                let name = if txt.is_empty() {
                                    format!("Template {}", self.data.templates.len() + 1)
                                } else { txt };
                                self.data.templates.push(Template { id: new_id(), name, field_labels: labels });
                                self.persist();
                                self.status = "Saved template".into();
                            }
                        }
                    }
                    _ => {}
                }
                self.overlay = Overlay::None;
                Task::none()
            }

            // ── context menu ─────────────────────────────────────────────
            Message::CollRightClicked(idx) => {
                self.ctx_is_collection = true;
                self.ctx_target = idx;
                self.ctx_target_id = self.data.collections.get(idx).map(|c| c.id.clone());
                self.ctx_multi = self.coll_checked.iter().filter(|c| **c).count() >= 2;
                self.ctx_pos = self.cursor;
                self.overlay = Overlay::ContextMenu;
                Task::none()
            }
            Message::ItemRightClicked(idx) => {
                // Select first so single-item actions act on the right row.
                if !self.item_multi { self.select_item(idx, false, false); }
                self.ctx_is_collection = false;
                self.ctx_target = idx;
                self.ctx_target_id = self.current_items().get(idx).map(|i| i.id.clone());
                self.ctx_multi = self.item_checked.iter().filter(|c| **c).count() >= 2;
                self.ctx_pos = self.cursor;
                self.overlay = Overlay::ContextMenu;
                Task::none()
            }
            Message::CtxRename => {
                self.overlay = Overlay::None;
                if self.ctx_is_collection {
                    match self.ctx_resolved_coll_index() {
                        Some(i) => return self.update(Message::OpenRenameCollection(i)),
                        None => return Task::none(),
                    }
                } else {
                    match self.ctx_resolved_item_index() {
                        Some(i) => return self.update(Message::OpenRenameItem(i)),
                        None => return Task::none(),
                    }
                }
            }
            Message::CtxPrimary => {
                let msg = if self.ctx_is_collection {
                    if self.ctx_multi { Message::DuplicateSelectedCollections }
                    else {
                        match self.ctx_resolved_coll_index() {
                            Some(i) => Message::DuplicateCollection(i),
                            None => { self.overlay = Overlay::None; return Task::none(); }
                        }
                    }
                } else if self.ctx_multi {
                    Message::DuplicateSelectedItems
                } else {
                    // Single-item duplicate acts on sel_item, which ItemRightClicked
                    // already set to the clicked row — no index to re-resolve.
                    Message::DuplicateItem
                };
                self.update(msg)
            }
            Message::CtxDanger => {
                let msg = if self.ctx_is_collection {
                    if self.ctx_multi { Message::DeleteSelectedCollections }
                    else {
                        match self.ctx_resolved_coll_index() {
                            Some(i) => Message::DeleteCollection(i),
                            None => { self.overlay = Overlay::None; return Task::none(); }
                        }
                    }
                } else if self.ctx_multi {
                    Message::DeleteSelectedItems
                } else {
                    // Single-item delete acts on sel_item (set by ItemRightClicked).
                    Message::DeleteItem
                };
                self.update(msg)
            }

            // ── settings ─────────────────────────────────────────────────
            Message::OpenSettings => { self.overlay = Overlay::Settings; Task::none() }
            Message::SetDarkMode(on) => {
                self.settings.dark_mode = on;
                self.reapply_theme();
                self.persist_settings();
                Task::none()
            }
            Message::SetAccent(c) => {
                self.settings.accent_hex = color_to_hex(c);
                self.reapply_theme();
                self.persist_settings();
                Task::none()
            }
            Message::FontInc => {
                self.settings.font_size = (self.settings.font_size + 1.0).min(22.0);
                self.persist_settings();
                Task::none()
            }
            Message::FontDec => {
                self.settings.font_size = (self.settings.font_size - 1.0).max(11.0);
                self.persist_settings();
                Task::none()
            }
            Message::ExportData => {
                let data = self.data.clone();
                Task::perform(export_dialog(data), |_| Message::Noop)
            }
            Message::ImportData => {
                Task::perform(import_dialog(), Message::ImportLoaded)
            }
            Message::ImportLoaded(Some(imported)) => {
                let ex_colls: std::collections::HashSet<String> =
                    self.data.collections.iter().map(|c| c.id.clone()).collect();
                let ex_items: std::collections::HashSet<String> =
                    self.data.items.iter().map(|i| i.id.clone()).collect();
                for c in imported.collections {
                    if !ex_colls.contains(&c.id) { self.data.collections.push(c); }
                }
                // Photos are stored as bare filenames living in photos_dir. On
                // import we keep the filenames as-is (the user copies the
                // photos/ folder alongside the JSON) and regenerate any missing
                // thumbnails for files that are present locally.
                for mut i in imported.items {
                    if ex_items.contains(&i.id) { continue; }
                    for p in &i.photos {
                        if image_util::resolve_photo(p).exists()
                            && !image_util::thumb_path_for(p).exists()
                        {
                            image_util::generate_thumbnail(p);
                        }
                    }
                    // Drop empty/blank refs but keep valid filenames even if the
                    // file isn't present yet, so re-adding the photos folder later
                    // restores them.
                    i.photos.retain(|p| !p.trim().is_empty());
                    self.data.items.push(i);
                }
                self.sel_coll = None;
                self.sel_item = None;
                self.reload_editors();
                self.persist();
                self.rebuild_coll_checked();
                self.rebuild_item_checked();
                self.status = "Imported".into();
                Task::none()
            }
            Message::ImportLoaded(None) => Task::none(),
            Message::OpenDataFolder => {
                let dir = app_dir();
                #[cfg(target_os = "windows")]
                { std::process::Command::new("explorer").arg(&dir).spawn().ok(); }
                #[cfg(target_os = "macos")]
                { std::process::Command::new("open").arg(&dir).spawn().ok(); }
                #[cfg(target_os = "linux")]
                { std::process::Command::new("xdg-open").arg(&dir).spawn().ok(); }
                Task::none()
            }
            Message::ResetPanels => {
                self.settings.left_ratio = default_left_ratio();
                self.settings.mid_ratio = default_mid_ratio();
                self.panes = build_panes(&self.settings);
                self.persist_settings();
                Task::none()
            }
        }
    }

    // ── context-menu target resolution ───────────────────────────────────
    // Map the id captured at right-click time back to a current index, so a
    // list reorder between right-click and action can't target the wrong row.
    // Falls back to the captured positional index only if no id was stored.

    fn ctx_resolved_coll_index(&self) -> Option<usize> {
        match &self.ctx_target_id {
            Some(id) => self.data.collections.iter().position(|c| &c.id == id),
            None => self.data.collections.get(self.ctx_target).map(|_| self.ctx_target),
        }
    }

    fn ctx_resolved_item_index(&self) -> Option<usize> {
        match &self.ctx_target_id {
            Some(id) => self.current_items().iter().position(|i| &i.id == id),
            None => {
                let n = self.current_items().len();
                (self.ctx_target < n).then_some(self.ctx_target)
            }
        }
    }

    // ── selection helpers ────────────────────────────────────────────────

    fn select_collection(&mut self, idx: usize, ctrl: bool, shift: bool) {
        // Auto-save in-progress edits before changing collection selection.
        if self.is_editing {
            self.flush_editors_to_item();
            self.persist();
        }
        let n = self.data.collections.len();
        if n == 0 || idx >= n { return; }
        if self.coll_checked.len() != n { self.coll_checked = vec![false; n]; }

        if ctrl {
            self.coll_checked[idx] = !self.coll_checked[idx];
            self.anchor_coll = Some(idx);
            self.coll_multi = self.coll_checked.iter().any(|c| *c);
            return;
        }
        if shift {
            // Recover the anchor, then validate it: a stale anchor (left over
            // from before a sort/delete/search change) can point past the end
            // of the current list, which would silently clamp the range start.
            // Fall back to the live selection, then to the clicked row itself.
            let anchor = self.anchor_coll
                .filter(|&a| a < n)
                .or(self.sel_coll.as_ref().and_then(|id|
                    self.data.collections.iter().position(|c| &c.id == id)))
                .unwrap_or(idx);
            self.anchor_coll = Some(anchor);
            let (lo, hi) = if anchor <= idx { (anchor, idx) } else { (idx, anchor) };
            let q = self.coll_search.to_lowercase();
            for (i, chk) in self.coll_checked.iter_mut().enumerate() {
                let visible = q.is_empty()
                    || self.data.collections.get(i)
                        .map(|c| c.name.to_lowercase().contains(&q)).unwrap_or(false);
                *chk = i >= lo && i <= hi && visible;
            }
            self.coll_multi = true;
            return;
        }

        // plain click
        for c in self.coll_checked.iter_mut() { *c = false; }
        self.coll_multi = false;
        self.anchor_coll = Some(idx);

        let clicked_id = self.data.collections[idx].id.clone();
        if self.sel_coll.as_deref() == Some(clicked_id.as_str()) {
            self.sel_coll = None;
            self.sel_item = None;
            self.is_editing = false;
            self.reload_editors();
        } else {
            self.sel_coll = Some(clicked_id);
            self.sel_item = None;
            self.is_editing = false;
            self.reload_editors();
        }
        self.rebuild_item_checked();
    }

    fn select_item(&mut self, idx: usize, ctrl: bool, shift: bool) {
        // Auto-save any in-progress edits before the selection changes; every
        // branch below clears edit mode and reloads the editors, which would
        // otherwise silently drop unsaved text.
        if self.is_editing {
            self.flush_editors_to_item();
            self.persist();
        }
        // Snapshot the current view as owned ids so we don't hold a borrow of
        // `self` across the mutations below.
        let ids: Vec<String> = self.current_items().iter().map(|i| i.id.clone()).collect();
        let n = ids.len();
        if n == 0 || idx >= n { return; }
        if self.item_checked.len() != n { self.item_checked = vec![false; n]; }

        if ctrl {
            self.item_checked[idx] = !self.item_checked[idx];
            self.anchor_item = Some(idx);
            self.item_multi = self.item_checked.iter().any(|c| *c);
            self.sel_item = None;
            self.is_editing = false;
            self.reload_editors();
            return;
        }
        if shift {
            // Same stale-anchor guard as collections: the filtered view length
            // changes with search/sort, so an old anchor index may no longer be
            // valid. Validate against the current view length first.
            let anchor = self.anchor_item
                .filter(|&a| a < n)
                .or(self.sel_item.as_ref().and_then(|id| ids.iter().position(|i| i == id)))
                .unwrap_or(idx);
            self.anchor_item = Some(anchor);
            let (lo, hi) = if anchor <= idx { (anchor, idx) } else { (idx, anchor) };
            for (i, chk) in self.item_checked.iter_mut().enumerate() {
                *chk = i >= lo && i <= hi;
            }
            self.item_multi = true;
            self.sel_item = None;
            self.is_editing = false;
            self.reload_editors();
            return;
        }

        // plain click
        for c in self.item_checked.iter_mut() { *c = false; }
        self.item_multi = false;
        self.anchor_item = Some(idx);

        let clicked_id = ids[idx].clone();
        if self.sel_item.as_deref() == Some(clicked_id.as_str()) {
            self.sel_item = None;
            self.is_editing = false;
        } else {
            self.sel_item = Some(clicked_id);
            self.is_editing = false;
        }
        self.reload_editors();
    }

    // ── mutation helpers ───────────────────────────────────────────────────

    fn flush_editors_to_item(&mut self) {
        let id = match &self.sel_item { Some(i) => i.clone(), None => return };
        let name = self.editors.name.text().trim_end_matches('\n').to_string();
        let desc = self.editors.desc.text().trim_end_matches('\n').to_string();
        let date = assemble_date(
            self.editors.year.text().trim(),
            self.editors.month.text().trim(),
            self.editors.day.text().trim(),
        );
        let fields: Vec<(String, String, String)> = self.editors.fields.iter()
            .map(|(fid, lbl, val)| (
                fid.clone(),
                lbl.text().trim_end_matches('\n').to_string(),
                val.text().trim_end_matches('\n').to_string(),
            ))
            .collect();
        if let Some(it) = self.data.items.iter_mut().find(|i| i.id == id) {
            it.name = name;
            it.short_desc = desc;
            it.acquired_date = date;
            for (fid, lbl, val) in fields {
                if let Some(f) = it.custom_fields.iter_mut().find(|f| f.id == fid) {
                    f.label = lbl;
                    f.value = val;
                }
            }
        }
    }

    fn duplicate_collection_by_index(&mut self, idx: usize) {
        if let Some(src) = self.data.collections.get(idx).cloned() {
            let new_cid = new_id();
            let src_items: Vec<Item> = self.data.items.iter()
                .filter(|i| i.collection_id == src.id).cloned().collect();
            let pos = (idx + 1).min(self.data.collections.len());
            let order = self.data.next_coll_order();
            self.data.collections.insert(pos, Collection {
                id: new_cid.clone(),
                name: format!("{} (copy)", src.name),
                icon: src.icon.clone(),
                order,
            });
            for mut it in src_items {
                it.id = new_id();
                it.collection_id = new_cid.clone();
                it.photos = it.photos.iter().filter_map(|p| image_util::copy_photo_file(p)).collect();
                it.custom_fields = it.custom_fields.into_iter()
                    .map(|mut f| { f.id = new_id(); f }).collect();
                self.data.items.push(it);
            }
            self.persist();
        }
    }

    fn duplicate_item_by_id(&mut self, id: &str) {
        let insert_at = self.data.items.iter().rposition(|i| i.id == id)
            .map(|p| p + 1).unwrap_or(self.data.items.len());
        let new_item = self.data.items.iter().find(|i| i.id == id).map(|src| {
            let mut n = src.clone();
            n.id = new_id();
            n.name = format!("{} (copy)", src.name);
            n.photos = src.photos.iter().filter_map(|p| image_util::copy_photo_file(p)).collect();
            n.custom_fields = n.custom_fields.into_iter()
                .map(|mut f| { f.id = new_id(); f }).collect();
            n
        });
        if let Some(it) = new_item {
            self.data.items.insert(insert_at, it);
            self.persist();
        }
    }

    fn lightbox_step(&mut self, dir: i32) {
        // Re-derive the photo list from the live selection rather than trusting
        // the count captured when the lightbox opened. If the underlying item or
        // its photos changed while the overlay was up, the cached count could
        // disagree with the actual vector and stepping would silently fail.
        let photos: Vec<String> = self.selected_item()
            .map(|it| it.photos.clone()).unwrap_or_default();
        self.lightbox_count = photos.len();
        if photos.is_empty() {
            self.lightbox_handle = None;
            self.lightbox_index = 0;
            return;
        }
        let n = photos.len() as i32;
        let next = (self.lightbox_index as i32 + dir).rem_euclid(n) as usize;
        self.lightbox_index = next;
        if let Some(p) = photos.get(next) {
            self.lightbox_handle = image_util::full_handle(p);
        }
    }
}

// ── async dialogs (run off the UI thread via Task::perform) ─────────────────

async fn pick_photos_dialog() -> Option<Vec<String>> {
    let files = rfd::AsyncFileDialog::new()
        .set_title("Choose photos")
        .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif"])
        .pick_files()
        .await?;
    // Copy + thumbnail off-thread.
    let copied: Vec<String> = files.iter()
        .filter_map(|f| image_util::import_picked_photo(f.path()))
        .collect();
    Some(copied)
}

async fn export_dialog(data: AppData) -> Option<()> {
    let handle = rfd::AsyncFileDialog::new()
        .set_title("Export collection data")
        .set_file_name("Collectors-Notebook-export.json")
        .add_filter("JSON", &["json"])
        .save_file()
        .await?;
    if let Ok(json) = serde_json::to_string_pretty(&data) {
        std::fs::write(handle.path(), json).ok();
    }
    Some(())
}

async fn import_dialog() -> Option<AppData> {
    let handle = rfd::AsyncFileDialog::new()
        .set_title("Import collection data")
        .add_filter("JSON", &["json"])
        .pick_file()
        .await?;
    let contents = std::fs::read_to_string(handle.path()).ok()?;
    serde_json::from_str::<AppData>(&contents).ok()
}
