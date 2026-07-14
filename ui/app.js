// Collector's Notebook — webview UI. State machine transcribed from the iced
// update.rs/view.rs (selection toggles, ctrl/shift anchors, flush semantics,
// Escape chain, context-menu id re-resolution, sort-preserves-checked, …).
"use strict";
const invoke = window.__TAURI__.core.invoke;

// Clipboard via native Rust commands (see main.rs). Using invoke instead of
// navigator.clipboard avoids the webview's "wants to see clipboard" permission
// dialog, and instead of the clipboard-manager JS bindings avoids depending on
// withGlobalTauri exposing them.
const readClipboardText = () => invoke("clipboard_read").catch(() => "");
const writeClipboardText = (t) => invoke("clipboard_write", { text: t }).catch(() => {});

// ─── state (the iced App struct) ────────────────────────────────────────────
const S = {
  data: { collections: [], items: [], templates: [] },
  settings: null,
  selColl: null, selItem: null,
  collSearch: "", itemSearch: "",
  collChecked: [], itemChecked: [],
  collMulti: false, itemMulti: false,
  anchorColl: null, anchorItem: null,
  isEditing: false,
  corrupt: null,
  lightbox: { open: false, index: 0, count: 0, zoom: 1, panX: 0, panY: 0 },
  lastClick: null, // {id, t} — manual double-click detection
  namePurpose: null, // {kind:'coll'|'item'|'template', id}
  dragSplit: 0,
};
const SORTS = ["added", "name-asc", "name-desc", "low-or-old", "high-or-new"];
const sortLabel = (m, isColl) => ({
  "added": "Date added", "name-asc": "Name A–Z", "name-desc": "Name Z–A",
  "low-or-old": isColl ? "Fewest items" : "Oldest first",
  "high-or-new": isColl ? "Most items" : "Newest first",
}[m]);
const ICONS = ["🚗","📁","🎧","🖊","📷","🎮","📚","⌚","💍","🎸","🎨","🏆","🎯","🔬","🚀","🌿",
  "🍷","⚽","🎲","💎","🖥","📻","🎺","🎻","🏺","💰","🔑","🔧","🔭","🎁","🚲","🌱",
  "🐾","🦋","🌊","🏠","🎭","🍵","🌍","🐶","📦"];
const NEW_COLL_ICONS = ["📁","📂","🎧","✒","📷","🎮","📚","⌚","💍","🎸"];
const ACCENTS = ["#4f8ef7","#7c5cbf","#2ecc71","#e67e22","#e74c3c","#1abc9c","#e91e8c","#f0c040"];

const $ = (id) => document.getElementById(id);
const el = (tag, cls, text) => {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text !== undefined) n.textContent = text;
  if (tag === "input") {
    // Turn off the webview's native autofill/suggestion dropdown. It guesses at
    // name/date fields and its suggestions interfered with saving edited values.
    n.setAttribute("autocomplete", "off");
    n.setAttribute("autocorrect", "off");
    n.setAttribute("autocapitalize", "off");
    n.setAttribute("spellcheck", "false");
    n.setAttribute("data-form-type", "other"); // hint some autofill engines respect
  }
  return n;
};
const newId = () => crypto.randomUUID();

// ─── model helpers (ports of model.rs) ──────────────────────────────────────
const itemCount = (cid) => S.data.items.filter((i) => i.collection_id === cid).length;
/// Per-currency totals of the VALUE / PRICE field. Buckets are keyed by the
/// currency symbol prefix ("" = plain number). If EXACTLY one real symbol
/// exists, plain numbers fold into it (the "usually type $, sometimes
/// forget" case); with genuinely mixed currencies they stay their own
/// bucket — sums across currencies are never merged.
function collectionValueBuckets(cid) {
  const buckets = new Map(); // symbol -> {sum, count}
  let unvalued = 0;
  for (const it of S.data.items) {
    if (it.collection_id !== cid) continue;
    const f = it.custom_fields.find((f) => /value|price/i.test(f.label));
    const m = f && f.value.match(/(-?\d[\d,]*\.?\d*)/);
    if (!m) { unvalued++; continue; }
    const num = parseFloat(m[1].replace(/,/g, "")) || 0;
    const s = f.value.match(/([^\s\d.,-]+)\s*-?\d/);
    const sym = s ? s[1] : "";
    const b = buckets.get(sym) || { sum: 0, count: 0 };
    b.sum += num; b.count += 1;
    buckets.set(sym, b);
  }
  const realSyms = [...buckets.keys()].filter((k) => k !== "");
  if (realSyms.length === 1 && buckets.has("")) {
    const plain = buckets.get(""), main = buckets.get(realSyms[0]);
    main.sum += plain.sum; main.count += plain.count;
    buckets.delete("");
  }
  const list = [...buckets.entries()].map(([symbol, b]) => ({ symbol, ...b }));
  // dominant first: most items, then largest sum
  list.sort((a, b) => b.count - a.count || b.sum - a.sum);
  return { list, unvalued };
}
const fmtVal = (n) => n % 1 === 0
  ? n.toLocaleString()
  : n.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
const bucketText = (b) => `${b.symbol}${fmtVal(b.sum)}`;
const keepDigits = (s, max) => (s.match(/\d/g) || []).slice(0, max).join("");
function splitDate(key) {
  if (!key) return ["", "", ""];
  const p = key.split("-");
  const bz = (s) => { const n = parseInt(s || "0", 10) || 0; return n === 0 ? "" : String(n); };
  return [bz(p[0]), bz(p[1]), bz(p[2])];
}
function assembleDate(y, m, d) {
  y = y.trim(); m = m.trim(); d = d.trim();
  if (!y && !m && !d) return "";
  const yr = Math.min(Math.max(parseInt(y, 10) || 0, 0), 9999);
  let mo = parseInt(m, 10) || 0; if (mo !== 0) mo = Math.min(Math.max(mo, 1), 12);
  let dy = parseInt(d, 10) || 0; if (dy !== 0) dy = Math.min(Math.max(dy, 1), 31);
  return `${String(yr).padStart(4, "0")}-${String(mo).padStart(2, "0")}-${String(dy).padStart(2, "0")}`;
}
function displayDate(key) {
  const [y, m, d] = splitDate(key);
  if (!y && !m && !d) return "—";
  let s = y || "?";
  if (m) s += "-" + m;
  if (d) s += "-" + d;
  return s;
}
function sortItemsInPlace(arr, mode) {
  const cmpDates = (a, b, asc) => {
    if (!a && !b) return 0;
    if (!a) return 1;   // undated last
    if (!b) return -1;
    return asc ? a.localeCompare(b) : b.localeCompare(a);
  };
  if (mode === "name-asc") arr.sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
  else if (mode === "name-desc") arr.sort((a, b) => b.name.toLowerCase().localeCompare(a.name.toLowerCase()));
  else if (mode === "low-or-old") arr.sort((a, b) => cmpDates(a.acquired_date, b.acquired_date, true));
  else if (mode === "high-or-new") arr.sort((a, b) => cmpDates(a.acquired_date, b.acquired_date, false));
}
function filteredItems() {
  if (!S.selColl) return [];
  const q = S.itemSearch.toLowerCase();
  const v = S.data.items.filter((i) => i.collection_id === S.selColl).filter((i) => {
    if (!q) return true;
    return i.name.toLowerCase().includes(q) || i.short_desc.toLowerCase().includes(q) ||
      i.custom_fields.some((f) => f.label.toLowerCase().includes(q) || f.value.toLowerCase().includes(q));
  });
  sortItemsInPlace(v, S.settings.item_sort);
  return v;
}
function sortCollectionsInPlace(mode) {
  const c = S.data.collections;
  if (mode === "added") c.sort((a, b) => (a.order || 0) - (b.order || 0));
  else if (mode === "name-asc") c.sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
  else if (mode === "name-desc") c.sort((a, b) => b.name.toLowerCase().localeCompare(a.name.toLowerCase()));
  else {
    const counts = Object.fromEntries(c.map((x) => [x.id, itemCount(x.id)]));
    c.sort((a, b) => mode === "low-or-old" ? counts[a.id] - counts[b.id] : counts[b.id] - counts[a.id]);
  }
}
const selectedItem = () => S.data.items.find((i) => i.id === S.selItem) || null;
const defaultFields = () =>
  ["CONDITION", "VALUE / PRICE", "TAGS", "NOTES"].map((l) => ({ id: newId(), label: l, value: "" }));

// ─── persistence ─────────────────────────────────────────────────────────────
const persist = () => invoke("save_data_cmd", { data: S.data });
const persistSettings = () => invoke("save_settings_cmd", { settings: S.settings });

// Debounced autosave for in-progress edits. Live edits write to the model on
// each keystroke, but if the webview is suspended while idle (WebView2 can
// discard an inactive view and reload from disk on resume), only what reached
// disk survives. Persisting shortly after typing stops closes that gap so an
// idle-suspend can't eat unsaved text. Trailing debounce keeps it cheap.
let _persistTimer = null;
const persistSoon = () => {
  if (_persistTimer) clearTimeout(_persistTimer);
  _persistTimer = setTimeout(() => { _persistTimer = null; persist(); }, 600);
};

// ─── thumbnails (async, cached) ──────────────────────────────────────────────
const thumbCache = new Map();
function setThumbBg(divEl, name) {
  if (thumbCache.has(name)) { divEl.style.backgroundImage = `url("${thumbCache.get(name)}")`; return; }
  invoke("thumb_b64", { name }).then((url) => {
    if (url) { thumbCache.set(name, url); divEl.style.backgroundImage = `url("${url}")`; }
  });
}
function setThumb(imgEl, name) {
  if (thumbCache.has(name)) { imgEl.src = thumbCache.get(name); return; }
  invoke("thumb_b64", { name }).then((url) => {
    if (url) { thumbCache.set(name, url); imgEl.src = url; }
  });
}

// ─── selection (update.rs::select_collection / select_item, verbatim) ───────
function flushIfEditing() {
  if (S.isEditing && S.selItem) { flushEditors(); persist(); S.isEditing = false; }
}
function selectCollection(idx, ctrl, shift) {
  flushIfEditing();
  const n = S.data.collections.length;
  if (!n || idx >= n) return;
  if (S.collChecked.length !== n) S.collChecked = new Array(n).fill(false);
  if (ctrl) {
    S.collChecked[idx] = !S.collChecked[idx];
    S.anchorColl = idx;
    S.collMulti = S.collChecked.some(Boolean);
    renderAll(); return;
  }
  if (shift) {
    let anchor = S.anchorColl != null && S.anchorColl < n ? S.anchorColl
      : S.data.collections.findIndex((c) => c.id === S.selColl);
    if (anchor < 0) anchor = idx;
    S.anchorColl = anchor;
    const [lo, hi] = anchor <= idx ? [anchor, idx] : [idx, anchor];
    const q = S.collSearch.toLowerCase();
    S.data.collections.forEach((c, i) => {
      const visible = !q || c.name.toLowerCase().includes(q);
      S.collChecked[i] = i >= lo && i <= hi && visible;
    });
    S.collMulti = true;
    renderAll(); return;
  }
  S.collChecked.fill(false); S.collMulti = false; S.anchorColl = idx;
  const clicked = S.data.collections[idx].id;
  if (S.selColl === clicked) { S.selColl = null; S.selItem = null; }
  else { S.selColl = clicked; S.selItem = null; }
  S.isEditing = false;
  rebuildItemChecked();
  renderAll();
}
function selectItem(idx, ctrl, shift) {
  flushIfEditing();
  const ids = filteredItems().map((i) => i.id);
  const n = ids.length;
  if (!n || idx >= n) return;
  if (S.itemChecked.length !== n) S.itemChecked = new Array(n).fill(false);
  if (ctrl) {
    S.itemChecked[idx] = !S.itemChecked[idx];
    S.anchorItem = idx;
    S.itemMulti = S.itemChecked.some(Boolean);
    S.selItem = null; S.isEditing = false;
    renderAll(); return;
  }
  if (shift) {
    let anchor = S.anchorItem != null && S.anchorItem < n ? S.anchorItem : ids.indexOf(S.selItem);
    if (anchor < 0) anchor = idx;
    S.anchorItem = anchor;
    const [lo, hi] = anchor <= idx ? [anchor, idx] : [idx, anchor];
    S.itemChecked = ids.map((_, i) => i >= lo && i <= hi);
    S.itemMulti = true;
    S.selItem = null; S.isEditing = false;
    renderAll(); return;
  }
  S.itemChecked.fill(false); S.itemMulti = false; S.anchorItem = idx;
  const clicked = ids[idx];
  S.selItem = S.selItem === clicked ? null : clicked;
  S.isEditing = false;
  renderAll();
}
const rebuildCollChecked = () => {
  S.collChecked = new Array(S.data.collections.length).fill(false);
  S.collMulti = false;
};
const rebuildItemChecked = () => {
  S.itemChecked = new Array(filteredItems().length).fill(false);
  S.itemMulti = false;
};
function clearSelection() { rebuildCollChecked(); rebuildItemChecked(); S.anchorColl = S.anchorItem = null; }

// ─── create / delete / duplicate (update.rs ports) ──────────────────────────
function newCollection() {
  const n = S.data.collections.length;
  const order = S.data.collections.reduce((m, c) => Math.max(m, c.order || 0), -1) + 1;
  S.data.collections.push({
    id: newId(), name: `New Collection ${n + 1}`,
    icon: NEW_COLL_ICONS[n % NEW_COLL_ICONS.length], order,
  });
  persist(); rebuildCollChecked(); renderAll();
}
function newItem() {
  if (!S.selColl) return;
  const item = {
    id: newId(), collection_id: S.selColl, name: "New Item", short_desc: "",
    photos: [], acquired_date: "", custom_fields: defaultFields(),
  };
  S.data.items.push(item);
  persist();
  S.selItem = item.id; S.isEditing = true;
  rebuildItemChecked(); renderAll();
}
function deleteItemByIdNow(id) {
  const it = S.data.items.find((i) => i.id === id);
  if (it) it.photos.forEach((p) => invoke("delete_photo", { name: p }));
  S.data.items = S.data.items.filter((i) => i.id !== id);
  if (S.selItem === id) { S.selItem = null; S.isEditing = false; }
  persist(); rebuildItemChecked(); renderAll();
}
function deleteCollectionByIdNow(cid) {
  S.data.items.filter((i) => i.collection_id === cid)
    .forEach((i) => i.photos.forEach((p) => invoke("delete_photo", { name: p })));
  S.data.items = S.data.items.filter((i) => i.collection_id !== cid);
  S.data.collections = S.data.collections.filter((c) => c.id !== cid);
  if (S.selColl === cid) { S.selColl = null; S.selItem = null; S.isEditing = false; }
  persist(); rebuildCollChecked(); rebuildItemChecked(); renderAll();
}
function deleteSelectedItemsNow() {
  const view = filteredItems();
  const ids = view.filter((_, i) => S.itemChecked[i]).map((i) => i.id);
  ids.forEach((id) => {
    const it = S.data.items.find((i) => i.id === id);
    if (it) it.photos.forEach((p) => invoke("delete_photo", { name: p }));
  });
  S.data.items = S.data.items.filter((i) => !ids.includes(i.id));
  if (ids.includes(S.selItem)) { S.selItem = null; S.isEditing = false; }
  persist(); rebuildItemChecked(); renderAll();
}
function deleteSelectedCollectionsNow() {
  const ids = S.data.collections.filter((_, i) => S.collChecked[i]).map((c) => c.id);
  ids.forEach((cid) => S.data.items.filter((i) => i.collection_id === cid)
    .forEach((i) => i.photos.forEach((p) => invoke("delete_photo", { name: p }))));
  S.data.items = S.data.items.filter((i) => !ids.includes(i.collection_id));
  S.data.collections = S.data.collections.filter((c) => !ids.includes(c.id));
  if (ids.includes(S.selColl)) { S.selColl = null; S.selItem = null; S.isEditing = false; }
  persist(); rebuildCollChecked(); rebuildItemChecked(); renderAll();
}
async function duplicateCollectionByIndex(idx) {
  const src = S.data.collections[idx];
  if (!src) return;
  const newCid = newId();
  const order = S.data.collections.reduce((m, c) => Math.max(m, c.order || 0), -1) + 1;
  S.data.collections.splice(Math.min(idx + 1, S.data.collections.length), 0,
    { id: newCid, name: `${src.name} (copy)`, icon: src.icon, order });
  const srcItems = S.data.items.filter((i) => i.collection_id === src.id);
  for (const it of srcItems) {
    const photos = [];
    for (const p of it.photos) {
      const copied = await invoke("copy_photo", { name: p });
      if (copied) photos.push(copied);
    }
    S.data.items.push({
      ...structuredClone(it), id: newId(), collection_id: newCid, photos,
      custom_fields: it.custom_fields.map((f) => ({ ...f, id: newId() })),
    });
  }
  persist(); rebuildCollChecked(); renderAll();
}
async function duplicateItemById(id) {
  const pos = S.data.items.findLastIndex((i) => i.id === id);
  const src = S.data.items.find((i) => i.id === id);
  if (!src) return;
  const photos = [];
  for (const p of src.photos) {
    const copied = await invoke("copy_photo", { name: p });
    if (copied) photos.push(copied);
  }
  S.data.items.splice(pos < 0 ? S.data.items.length : pos + 1, 0, {
    ...structuredClone(src), id: newId(), name: `${src.name} (copy)`, photos,
    custom_fields: src.custom_fields.map((f) => ({ ...f, id: newId() })),
  });
  persist(); rebuildItemChecked(); renderAll();
}

// ─── delete confirmations (photos are deleted with their items, so these are
//     the guard rail; the launch backup covers data.json itself) ─────────────
function deleteItemById(id) {
  const it = S.data.items.find((i) => i.id === id);
  const name = it ? it.name : "this item";
  const ph = it && it.photos.length ? `\nIts ${it.photos.length} photo(s) will also be deleted.` : "";
  openConfirm(`Delete “${name}”?${ph}`, () => deleteItemByIdNow(id));
}
function deleteCollectionById(cid) {
  const c = S.data.collections.find((x) => x.id === cid);
  const name = c ? c.name : "this collection";
  const n = itemCount(cid);
  openConfirm(`Delete “${name}” and its ${n} item(s)?\nAll their photos will also be deleted.`, () => deleteCollectionByIdNow(cid));
}
function deleteSelectedItems() {
  const n = S.itemChecked.filter(Boolean).length;
  if (!n) return;
  openConfirm(`Delete ${n} checked item(s)?\nTheir photos will also be deleted.`, deleteSelectedItemsNow);
}
function deleteSelectedCollections() {
  const n = S.collChecked.filter(Boolean).length;
  if (!n) return;
  openConfirm(`Delete ${n} checked collection(s) and all their items?\nAll their photos will also be deleted.`, deleteSelectedCollectionsNow);
}

// ─── sort (checked sets survive re-sorts by id — update.rs parity) ──────────
function setCollSort(mode) {
  S.settings.coll_sort = mode; persistSettings();
  const checkedIds = new Set(S.data.collections.filter((_, i) => S.collChecked[i]).map((c) => c.id));
  sortCollectionsInPlace(mode);
  S.collChecked = S.data.collections.map((c) => checkedIds.has(c.id));
  S.collMulti = S.collChecked.some(Boolean);
  const i = S.data.collections.findIndex((c) => c.id === S.selColl);
  S.anchorColl = i >= 0 ? i : null;
  renderAll();
}
function setItemSort(mode) {
  const oldView = filteredItems().map((i) => i.id);
  const checkedIds = new Set(oldView.filter((_, i) => S.itemChecked[i]));
  S.settings.item_sort = mode; persistSettings();
  const newView = filteredItems().map((i) => i.id);
  S.itemChecked = newView.map((id) => checkedIds.has(id));
  S.itemMulti = S.itemChecked.some(Boolean);
  const i = newView.indexOf(S.selItem);
  S.anchorItem = i >= 0 ? i : null;
  renderAll();
}

// ─── editing (flush_editors_to_item parity: trims, keep_digits→assemble) ────
function flushEditors() {
  const it = selectedItem();
  if (!it) return;
  const nameEl = $("ed-name"), descEl = $("ed-desc");
  if (nameEl) it.name = nameEl.value.replace(/\n+$/, "");
  if (descEl) it.short_desc = descEl.value.replace(/\n+$/, "");
  const y = $("ed-y"), m = $("ed-m"), d = $("ed-d");
  if (y && m && d) it.acquired_date = assembleDate(keepDigits(y.value, 4), keepDigits(m.value, 2), keepDigits(d.value, 2));
  it.custom_fields.forEach((f) => {
    const le = document.querySelector(`[data-flabel="${f.id}"]`);
    const ve = document.querySelector(`[data-fvalue="${f.id}"]`);
    if (le) f.label = le.value.replace(/\n+$/, "");
    if (ve) f.value = ve.value.replace(/\n+$/, "");
  });
}
function toggleEdit() {
  if (!S.selItem) return;
  if (S.isEditing) { flushEditors(); persist(); S.isEditing = false; }
  else S.isEditing = true;
  renderAll();
}
function addCustomField() {
  const it = selectedItem();
  if (!it) return;
  flushEditors();
  it.custom_fields.push({ id: newId(), label: "NEW FIELD", value: "" });
  persist(); renderDetail();
}
function removeCustomField(fid) {
  const it = selectedItem();
  if (!it) return;
  flushEditors();
  it.custom_fields = it.custom_fields.filter((f) => f.id !== fid);
  persist(); renderDetail();
}
async function addPhotos() {
  const it = selectedItem();
  if (!it) return;
  const names = await invoke("pick_photos");
  if (!names.length) return;
  it.photos.push(...names);
  persist(); renderDetail();
}
function setMainPhoto(i) {
  const it = selectedItem();
  if (!it || i === 0 || i >= it.photos.length) return;
  it.photos.unshift(it.photos.splice(i, 1)[0]);
  persist(); renderDetail();
}
function removePhoto(i) {
  const it = selectedItem();
  if (!it || i >= it.photos.length) return;
  invoke("delete_photo", { name: it.photos.splice(i, 1)[0] });
  persist(); renderDetail();
}

// ─── rendering ───────────────────────────────────────────────────────────────
function applyChrome() {
  document.body.dataset.theme = S.settings.dark_mode ? "dark" : "light";
  document.documentElement.style.setProperty("--fs", S.settings.font_size + "px");
  document.documentElement.style.setProperty("--accent", S.settings.accent_hex);
  const l = Math.min(Math.max(S.settings.left_ratio, 0.08), 0.33);
  const m = Math.min(Math.max(S.settings.mid_ratio, 0.12), 0.7);
  // Two 6px splitters sit between the three panels; subtract their share so
  // left + splitter + mid + splitter + right(flex:1) sum to exactly the
  // window width — no overflow, no body showing through at the edges.
  $("left-panel").style.width = `calc(${(l * 100).toFixed(4)}% - 9px)`;
  $("mid-panel").style.width = `calc(${((1 - l) * m * 100).toFixed(4)}% - 5px)`;
}
function renderCollections() {
  const list = $("coll-list");
  list.textContent = "";
  const q = S.collSearch.toLowerCase();
  let anyRow = false;
  S.data.collections.forEach((c, idx) => {
    if (q && !c.name.toLowerCase().includes(q)) return;
    anyRow = true;
    const row = el("div", "coll-row"
      + (S.selColl === c.id ? " selected" : "")
      + (S.collMulti && S.collChecked[idx] ? " checked" : ""));
    row.dataset.idx = idx; row.dataset.id = c.id; row.dataset.kind = "coll";
    row.appendChild(el("div", "ribbon"));
    if (S.collMulti) {
      const cb = el("div", "checkbox" + (S.collChecked[idx] ? " on" : ""), S.collChecked[idx] ? "✓" : "");
      cb.dataset.check = idx;
      row.appendChild(cb);
    }
    const chip = el("div", "icon-chip", c.icon);
    chip.dataset.iconpick = idx;
    row.appendChild(chip);
    const names = el("div", "names");
    names.appendChild(el("div", "name", c.name));
    const { list: vals } = collectionValueBuckets(c.id);
    let valText = "";
    if (vals.length === 1) valText = ` · ${bucketText(vals[0])}`;
    else if (vals.length > 1) {
      const others = vals.slice(1).reduce((n, b) => n + b.count, 0);
      valText = ` · ${bucketText(vals[0])} +${others} mixed`;
    }
    const count = el("div", "count", `${itemCount(c.id)} items${valText}`);
    if (vals.length) {
      count.title = "Total value: " + vals.map(bucketText).join("  +  ")
        + (vals.length > 1 ? "  (mixed currencies — not summed)" : "");
    }
    names.appendChild(count);
    row.appendChild(names);
    list.appendChild(row);
  });
  if (!S.data.collections.length) {
    const es = el("div", "empty-state");
    es.appendChild(el("div", "big-emoji", "🗂"));
    es.appendChild(el("div", "muted", "Press + to create a collection"));
    list.appendChild(es);
  } else if (!anyRow) {
    list.appendChild(el("div", "empty-state muted", "No matches"));
  }
  const hint = $("multi-hint");
  hint.textContent = "";
  if (S.collMulti || S.itemMulti) {
    const clear = el("button", "pill-btn", "Clear");
    clear.onclick = () => { clearSelection(); renderAll(); };
    hint.appendChild(clear);
    if (S.collMulti) {
      const del = el("button", "pill-btn danger", "Delete");
      del.onclick = deleteSelectedCollections;
      hint.appendChild(del);
    }
  } else {
    const t = el("div", "hint-text", "Ctrl/Shift-click to multi-select\nRight-click for options");
    t.style.whiteSpace = "pre-line";
    hint.appendChild(t);
  }
  $("recovery-note").classList.toggle("hidden", !S.corrupt);
  if (S.corrupt) $("recovery-note").textContent = `Recovered: data.json was corrupt — backup at ${S.corrupt}`;
}
function renderItems() {
  // sort labels first — they must show even when nothing is selected
  // (they used to be set after the early return below, so they were blank
  // at launch)
  $("coll-sort-btn").textContent = sortLabel(S.settings.coll_sort, true) + " ▾";
  $("item-sort-btn").textContent = sortLabel(S.settings.item_sort, false) + " ▾";
  const hasColl = !!S.selColl;
  const coll = S.data.collections.find((c) => c.id === S.selColl);
  $("mid-title").textContent = coll ? coll.name : "Items";
  $("btn-new-item").classList.toggle("hidden", !hasColl);
  $("btn-del-multi").classList.toggle("hidden", !(hasColl && S.itemMulti));
  $("mid-tools").classList.toggle("hidden", !hasColl);
  $("pick-prompt").classList.toggle("hidden", hasColl);
  const list = $("item-list");
  list.classList.toggle("hidden", !hasColl);
  list.textContent = "";
  if (!hasColl) return;
  const view = filteredItems();
  view.forEach((it, idx) => {
    const card = el("div", "item-card"
      + (S.selItem === it.id ? " selected" : "")
      + (S.itemMulti && S.itemChecked[idx] ? " checked" : ""));
    card.dataset.idx = idx; card.dataset.id = it.id; card.dataset.kind = "item";
    if (S.itemMulti) {
      const cb = el("div", "checkbox" + (S.itemChecked[idx] ? " on" : ""), S.itemChecked[idx] ? "✓" : "");
      cb.dataset.check = idx;
      card.appendChild(cb);
    }
    const photo = it.photos.find((p) => p);
    if (photo) {
      const im = el("img", "thumb");
      im.alt = ""; setThumb(im, photo);
      card.appendChild(im);
    } else {
      card.appendChild(el("div", "thumb-ph", "📷"));
    }
    const names = el("div", "names");
    names.appendChild(el("div", "name", it.name));
    const d = el("div", "desc", it.short_desc.replace(/\n/g, " "));
    names.appendChild(d);
    card.appendChild(names);
    list.appendChild(card);
  });
  if (!view.length) {
    const es = el("div", "empty-state");
    es.appendChild(el("div", "big-emoji", "📦"));
    es.appendChild(el("div", "muted", "Press + to add an item"));
    list.appendChild(es);
  }
}
function renderDetail() {
  const it = selectedItem();
  // While editing, if focus is inside the detail panel, don't tear down and
  // rebuild it — that would drop the user's focus/caret mid-type (and, before
  // live-binding, could clear unsaved text). The inputs write straight to the
  // model on input, so the panel is already up to date; skip the rebuild.
  if (S.isEditing && it) {
    const root0 = $("detail");
    const af = document.activeElement;
    if (root0 && af && root0.contains(af) && af.matches("input, textarea")) return;
  }
  $("detail-empty").classList.toggle("hidden", !!it);
  const root = $("detail");
  root.classList.toggle("hidden", !it);
  root.textContent = "";
  if (!it) return;
  const E = S.isEditing;

  const top = el("div", "detail-top");
  top.appendChild(el("div", "detail-title", it.name.trim() || "New Item"));
  const editBtn = el("button", "text-btn", E ? "💾 Save" : "✏ Edit");
  editBtn.onclick = toggleEdit;
  const delBtn = el("button", "text-btn danger", "🗑 Delete");
  delBtn.onclick = () => deleteItemById(it.id);
  top.append(editBtn, delBtn);
  root.appendChild(top);

  // photo + name/desc
  const prow = el("div", "photo-row");
  const main = it.photos.find((p) => p);
  if (main) {
    const im = el("img", "main-photo clickable");
    setThumb(im, main);
    im.onclick = () => (E ? addPhotos() : openLightbox(0));
    im.title = E ? "Add more photos" : "Click to enlarge";
    prow.appendChild(im);
  } else {
    const ph = el("div", "main-photo-ph");
    ph.appendChild(el("div", "", "📷"));
    if (E) { ph.appendChild(el("div", "muted", "Click to add")); ph.style.cursor = "pointer"; ph.onclick = addPhotos; }
    prow.appendChild(ph);
  }
  const col = el("div", "photo-col");
  col.appendChild(subRow("Name", () => openEnlarge("Name",
    () => (E ? $("ed-name").value : it.name),
    E ? (v) => { $("ed-name").value = v.replace(/\n/g, " "); } : null)));
  if (E) {
    const inp = el("input", "value-input"); inp.id = "ed-name"; inp.value = it.name;
    // Write to the model on every keystroke. renderDetail() rebuilds this input
    // from it.name, so if the model lagged the DOM a redraw would wipe what you
    // typed. Keeping them in sync makes any redraw reproduce the same text.
    inp.addEventListener("input", () => { it.name = inp.value.replace(/\n+$/, ""); persistSoon(); });
    col.appendChild(inp);
  } else col.appendChild(valueView(it.name));
  col.appendChild(subRow("Description", () => openEnlarge("Description",
    () => (E ? $("ed-desc").value : it.short_desc),
    E ? (v) => { $("ed-desc").value = v; } : null)));
  if (E) {
    const ta = el("textarea", "value-area"); ta.id = "ed-desc"; ta.rows = 3; ta.value = it.short_desc;
    ta.addEventListener("input", () => { it.short_desc = ta.value.replace(/\n+$/, ""); persistSoon(); });
    col.appendChild(ta);
  } else col.appendChild(valueView(it.short_desc));
  prow.appendChild(col);
  root.appendChild(prow);

  // photo strip (>1 photo, or any while editing)
  if (it.photos.length > 1 || (E && it.photos.length > 0)) {
    root.appendChild(sub("Photos"));
    const strip = el("div", "strip");
    it.photos.forEach((p, i) => {
      const tile = el("div", "strip-tile" + (i === 0 ? " main" : ""));
      // Inline sizing: immune to stylesheet staleness or override — a 56×56
      // block, no matter what.
      tile.style.cssText += "position:relative;flex:0 0 auto;width:56px;height:56px;";
      const im = el("div", "strip-img");
      im.style.cssText +=
        "display:block;width:56px;height:56px;min-width:56px;min-height:56px;" +
        "border-radius:8px;background-size:cover;background-position:center;";
      setThumbBg(im, p);
      im.onclick = () => (E ? setMainPhoto(i) : openLightbox(i));
      tile.appendChild(im);
      if (E) {
        const x = el("div", "strip-x", "×");
        x.onclick = (ev) => { ev.stopPropagation(); removePhoto(i); };
        tile.appendChild(x);
      }
      strip.appendChild(tile);
    });
    root.appendChild(strip);
    root.appendChild(el("div", "muted",
      E ? "Click to set as main · × to remove" : "Click a photo to enlarge"));
  }

  // date
  root.appendChild(sub("Date Acquired"));
  if (E) {
    const dr = el("div", "date-row");
    const [y, m, d] = splitDate(it.acquired_date);
    const mk = (id, v, ph, cls, max) => {
      const i2 = el("input", "value-input date-in " + cls);
      i2.id = id; i2.value = v; i2.placeholder = ph;
      i2.addEventListener("input", () => {
        i2.value = keepDigits(i2.value, max);
        const yy = $("ed-y"), mm = $("ed-m"), dd = $("ed-d");
        if (yy && mm && dd) {
          it.acquired_date = assembleDate(
            keepDigits(yy.value, 4), keepDigits(mm.value, 2), keepDigits(dd.value, 2));
          persistSoon();
        }
      });
      return i2;
    };
    dr.append(mk("ed-y", y, "YYYY", "", 4), el("span", "muted", "–"),
      mk("ed-m", m, "MM", "small", 2), el("span", "muted", "–"),
      mk("ed-d", d, "DD", "small", 2));
    root.appendChild(dr);
  } else {
    const dd = el("div", "", displayDate(it.acquired_date));
    dd.style.textAlign = "center";
    root.appendChild(dd);
  }

  root.appendChild(el("div", "separator"));
  root.appendChild(sub("Details"));

  it.custom_fields.forEach((f) => {
    const fb = el("div", "fieldblock");
    if (E) {
      const lr = el("div", "labelrow");
      const li = el("input", "label-input"); li.value = f.label; li.dataset.flabel = f.id;
      li.addEventListener("input", () => { f.label = li.value.replace(/\n+$/, ""); persistSoon(); });
      const en = el("button", "enlarge-link", "Enlarge");
      en.style.position = "static"; en.style.transform = "none";
      en.onclick = () => openEnlarge(f.label || "Field",
        () => document.querySelector(`[data-fvalue="${f.id}"]`).value,
        (v) => { document.querySelector(`[data-fvalue="${f.id}"]`).value = v; });
      const rx = el("button", "remove-x", "✖");
      rx.onclick = () => removeCustomField(f.id);
      lr.append(li, en, rx);
      fb.appendChild(lr);
      const ta = el("textarea", "value-area"); ta.rows = 2; ta.value = f.value; ta.dataset.fvalue = f.id;
      ta.addEventListener("input", () => { f.value = ta.value.replace(/\n+$/, ""); persistSoon(); });
      fb.appendChild(ta);
    } else {
      const lr = el("div", "sub-row");
      lr.appendChild(el("div", "field-label", f.label));
      const en = el("button", "enlarge-link", "Enlarge");
      en.onclick = () => openEnlarge(f.label || "Field", () => f.value, null);
      lr.appendChild(en);
      fb.appendChild(lr);
      fb.appendChild(valueView(f.value));
    }
    root.appendChild(fb);
  });

  if (E) {
    const ar = el("div", "addfield-row");
    const add = el("button", "text-btn", "➕ Add field");
    add.onclick = addCustomField;
    const tpl = el("button", "text-btn", "📋 Templates");
    tpl.onclick = openTemplatePicker;
    ar.append(add, tpl);
    root.appendChild(ar);
  }
}
const sub = (t) => el("div", "subheader", t);
/// Subheader with an iced-style right-aligned "Enlarge" link.
function subRow(title, onEnlarge) {
  const r = el("div", "sub-row");
  r.appendChild(sub(title));
  const b = el("button", "enlarge-link", "Enlarge");
  b.onclick = onEnlarge;
  r.appendChild(b);
  return r;
}
/// Big-editor overlay. Editable when setText is provided; read-only otherwise.
function openEnlarge(title, getText, setText) {
  let scrim;
  const ta = el("textarea", "value-area enlarge-area");
  ta.value = getText();
  if (!setText) ta.readOnly = true;
  const row = el("div", "set-row");
  row.appendChild(el("span", "spacer"));
  if (setText) {
    const cancel = el("button", "plain-btn", "Cancel");
    cancel.onclick = () => scrim.remove();
    const ok = el("button", "ok-btn", "Done");
    ok.onclick = () => { setText(ta.value); scrim.remove(); };
    row.append(cancel, ok);
  } else {
    const ok = el("button", "ok-btn", "Close");
    ok.onclick = () => scrim.remove();
    row.appendChild(ok);
  }
  scrim = modal([modalHead(title, () => scrim), ta, row], { width: Math.min(720, innerWidth - 80) });
  if (setText) ta.focus();
}
const valueView = (t) => { const v = el("div", "value-view", t); return v; };
function renderAll() { applyChrome(); renderCollections(); renderItems(); renderDetail(); }

// ─── context menu (iced overlay: id captured at open, re-resolved) ──────────
function openContextMenu(isColl, id, x, y) {
  closeOverlays();
  const multi = isColl
    ? S.collChecked.filter(Boolean).length >= 2
    : S.itemChecked.filter(Boolean).length >= 2;
  const menu = el("div", "ctx-menu");
  const mk = (label, danger, fn) => {
    const b = el("button", "ctx-item" + (danger ? " danger" : ""), label);
    b.onclick = () => { closeOverlays(); fn(); };
    menu.appendChild(b);
  };
  if (!multi) mk("Rename", false, () => (isColl ? openRename("coll", id) : openRename("item", id)));
  if (isColl && !multi) mk("Total value…", false, () => openValuePanel(id));
  mk(multi ? "Duplicate all checked" : (isColl ? "Duplicate collection" : "Duplicate item"), false, () => {
    if (isColl) {
      if (multi) {
        const ids = S.data.collections.filter((_, i) => S.collChecked[i]).map((c) => c.id);
        (async () => { for (const cid of ids) {
          const i = S.data.collections.findIndex((c) => c.id === cid);
          if (i >= 0) await duplicateCollectionByIndex(i);
        } })();
      } else {
        const i = S.data.collections.findIndex((c) => c.id === id);
        if (i >= 0) duplicateCollectionByIndex(i);
      }
    } else if (multi) {
      const view = filteredItems().map((i) => i.id);
      (async () => { for (const iid of view.filter((_, i) => S.itemChecked[i])) await duplicateItemById(iid); })();
    } else duplicateItemById(id);
  });
  mk(multi ? "Delete all checked" : (isColl ? "Delete collection" : "Delete item"), true, () => {
    if (isColl) multi ? deleteSelectedCollections() : deleteCollectionById(id);
    else multi ? deleteSelectedItems() : deleteItemById(id);
  });
  document.body.appendChild(menu);
  const r = menu.getBoundingClientRect();
  menu.style.left = Math.max(0, Math.min(x, innerWidth - r.width - 8)) + "px";
  menu.style.top = Math.max(0, Math.min(y, innerHeight - r.height - 8)) + "px";
}
function closeOverlays() {
  document.querySelectorAll(".ctx-menu").forEach((n) => n.remove());
}

// Minimal right-click menu for text inputs/textareas: Copy, Paste, Select All.
// Replaces the webview's native menu (which is suppressed everywhere) so the
// only menus in the app are ones we control. Uses execCommand, which keeps the
// field's native undo stack intact; paste falls back to the async clipboard API
// where execCommand("paste") is blocked.
function openTextContextMenu(field, x, y) {
  closeOverlays();
  const hasSelection = field.selectionStart !== field.selectionEnd;
  const menu = el("div", "ctx-menu");
  const mk = (label, enabled, fn) => {
    const b = el("button", "ctx-item" + (enabled ? "" : " disabled"), label);
    if (enabled) b.onclick = () => { closeOverlays(); field.focus(); fn(); };
    else { b.disabled = true; b.style.opacity = "0.4"; b.style.cursor = "default"; }
    menu.appendChild(b);
  };
  mk("Copy", hasSelection, () => {
    const sel = field.value.substring(field.selectionStart, field.selectionEnd);
    if (sel) writeClipboardText(sel);
  });
  mk("Paste", !field.readOnly && !field.disabled, async () => {
    // Read via the Tauri clipboard plugin (native, no webview permission
    // prompt) and insert at the caret. We intentionally do NOT use
    // navigator.clipboard here — in the webview that triggers a "wants to see
    // clipboard" allow/block dialog. setRangeText keeps the field's own value
    // and fires input so autosave/live-binding pick it up.
    try {
      const text = await readClipboardText();
      if (text) {
        const s = field.selectionStart, e = field.selectionEnd;
        field.setRangeText(text, s, e, "end");
        field.dispatchEvent(new Event("input", { bubbles: true }));
      }
    } catch { /* clipboard unavailable */ }
  });
  mk("Select All", field.value.length > 0, () => { field.select(); });
  document.body.appendChild(menu);
  const r = menu.getBoundingClientRect();
  menu.style.left = Math.max(0, Math.min(x, innerWidth - r.width - 8)) + "px";
  menu.style.top = Math.max(0, Math.min(y, innerHeight - r.height - 8)) + "px";
}

// ─── modals ──────────────────────────────────────────────────────────────────
function modal(children, { width } = {}) {
  const scrim = el("div", "scrim");
  const card = el("div", "modal-card");
  if (width) card.style.width = width + "px";
  card.onclick = (e) => e.stopPropagation();
  children.forEach((c) => card.appendChild(c));
  scrim.appendChild(card);
  scrim.onclick = () => scrim.remove();
  $("overlay-root").appendChild(scrim);
  return scrim;
}
const modalHead = (title, scrimGetter) => {
  const h = el("div", "modal-head");
  h.appendChild(el("div", "heading", title));
  h.appendChild(el("span", "spacer"));
  const x = el("button", "icon-btn", "✖");
  x.onclick = () => scrimGetter().remove();
  h.appendChild(x);
  return h;
};
function openRename(kind, id) {
  const cur = kind === "coll"
    ? (S.data.collections.find((c) => c.id === id)?.name ?? "")
    : (S.data.items.find((i) => i.id === id)?.name ?? "");
  const title = kind === "coll" ? "Rename collection" : "Rename item";
  openNameInput(title, cur, (txt) => {
    if (!txt) return;
    if (kind === "coll") {
      const c = S.data.collections.find((c) => c.id === id);
      if (c) c.name = txt;
      rebuildCollChecked();
    } else {
      const i = S.data.items.find((i) => i.id === id);
      if (i) i.name = txt;
      rebuildItemChecked();
    }
    persist(); renderAll();
  });
}
/// One-button notice modal.
function openNotice(title, message) {
  let scrim;
  const msg = el("div", "", message);
  msg.style.whiteSpace = "pre-line";
  const row = el("div", "set-row");
  row.appendChild(el("span", "spacer"));
  const ok = el("button", "ok-btn", "OK");
  ok.onclick = () => scrim.remove();
  row.appendChild(ok);
  scrim = modal([modalHead(title, () => scrim), msg, row], { width: 360 });
}

/// Styled confirmation for destructive actions. Enter/click Delete confirms,
/// Esc/Cancel/backdrop aborts.
function openConfirm(message, onYes) {
  let scrim;
  const msg = el("div", "", message);
  msg.style.whiteSpace = "pre-line";
  const row = el("div", "set-row");
  row.appendChild(el("span", "spacer"));
  const cancel = el("button", "plain-btn", "Cancel");
  cancel.onclick = () => scrim.remove();
  const yes = el("button", "ok-btn danger", "Delete");
  yes.onclick = () => { scrim.remove(); onYes(); };
  row.append(cancel, yes);
  scrim = modal([modalHead("Confirm delete", () => scrim), msg, row], { width: 380 });
  yes.focus();
}

function openNameInput(title, initial, onOk) {
  let scrim;
  const input = el("input", "value-input");
  input.value = initial;
  const row = el("div", "set-row");
  row.appendChild(el("span", "spacer"));
  const cancel = el("button", "plain-btn", "Cancel");
  cancel.onclick = () => scrim.remove();
  const ok = el("button", "ok-btn", "OK");
  const accept = () => { const t = input.value.trim(); scrim.remove(); onOk(t); };
  ok.onclick = accept;
  input.addEventListener("keydown", (e) => { if (e.key === "Enter") accept(); });
  row.append(cancel, ok);
  scrim = modal([modalHead(title, () => scrim), input, row], { width: 360 });
  input.focus(); input.select();
}
function openIconPicker(collIdx) {
  let scrim;
  const grid = el("div", "icon-grid");
  ICONS.forEach((ico) => {
    const t = el("button", "icon-tile", ico);
    t.onclick = () => {
      const c = S.data.collections[collIdx];
      if (c) { c.icon = ico; persist(); renderAll(); }
      scrim.remove();
    };
    grid.appendChild(t);
  });
  scrim = modal([modalHead("Choose an icon", () => scrim), grid]);
}
function openTemplatePicker() {
  let scrim;
  const rows = [];
  if (!S.data.templates.length) rows.push(el("div", "muted", "No templates saved yet."));
  S.data.templates.forEach((t) => {
    const r = el("div", "template-row");
    const apply = el("button", "action-row t-apply", `${t.name} — ${t.field_labels.length} fields`);
    apply.onclick = () => {
      const it = selectedItem();
      if (it) {
        flushEditors();
        it.custom_fields = t.field_labels.map((l) => ({ id: newId(), label: l, value: "" }));
        persist(); renderDetail();
      }
      scrim.remove();
    };
    const del = el("button", "remove-x", "✖");
    del.onclick = () => {
      S.data.templates = S.data.templates.filter((x) => x.id !== t.id);
      persist(); scrim.remove(); openTemplatePicker();
    };
    r.append(apply, del);
    rows.push(r);
  });
  const saveBtn = el("button", "action-row", "Save current fields as template");
  saveBtn.onclick = () => {
    scrim.remove();
    const it = selectedItem();
    if (!it) return;
    flushEditors();
    openNameInput("Save as template", "", (txt) => {
      if (!txt) return;
      S.data.templates.push({ id: newId(), name: txt, field_labels: it.custom_fields.map((f) => f.label) });
      persist();
    });
  };
  scrim = modal([modalHead("Templates", () => scrim), ...rows, saveBtn], { width: 380 });
}
function allReferencedPhotos() {
  const set = new Set();
  for (const it of S.data.items) for (const p of it.photos) if (p && p.trim()) set.add(p);
  return [...set];
}
function fmtBytes(n) {
  if (n < 1024) return n + " B";
  if (n < 1048576) return (n / 1024).toFixed(0) + " KB";
  if (n < 1073741824) return (n / 1048576).toFixed(1) + " MB";
  return (n / 1073741824).toFixed(2) + " GB";
}

async function openBackups() {
  let scrim;
  const rows = [];
  const backups = await invoke("list_backups");
  rows.push(el("div", "muted",
    "A snapshot of the database (data.json) is taken automatically at every " +
    "launch; the newest 5 are kept. Photos are not part of snapshots. " +
    "A backup is made before every restore, so the restore can be reverted, as well."));
  const now = el("button", "action-row", "Back up now");
  let busy = false;
  now.onclick = async () => {
    if (busy) return;
    busy = true; now.disabled = true; now.style.opacity = "0.6";
    const ok = await invoke("backup_now");
    scrim.remove();
    if (ok) openBackups();
    else openNotice("Backup", "Backup failed — is the data folder writable?");
  };
  rows.push(now);

  // ── Photos section: incremental additive mirror of photos + thumbnails ──
  rows.push(el("div", "separator"));
  rows.push(sub("Photos"));
  const status = await invoke("photo_archive_status", { referenced: allReferencedPhotos() });
  const info = el("div", "muted",
    `Archive: ${status.archived_photos} photo(s), ${fmtBytes(status.archive_bytes)}` +
    (status.deleted_pending ? ` · ${status.deleted_pending} deleted photo(s) retained` : ""));
  rows.push(info);
  rows.push(el("div", "muted",
    "Photos are mirrored to the archive and never removed when you delete " +
    "items — deleted photos are kept so an accidental deletion can be undone."));

  const doPhoto = (label, fn, danger) => {
    const b = el("button", "action-row" + (danger ? " danger" : ""), label);
    b.style.flex = "1";
    let b_busy = false;
    b.onclick = async () => {
      if (b_busy) return;
      b_busy = true; b.disabled = true; b.style.opacity = "0.6";
      const old = b.textContent; b.textContent = "Working…";
      const msg = await fn();
      b.textContent = old; b.disabled = false; b.style.opacity = "";
      b_busy = false;
      if (msg) openNotice("Photos", msg);
      else { scrim.remove(); openBackups(); }
    };
    return b;
  };
  const pairRow = (a2, b2) => {
    const r = el("div", "action-pair");
    r.append(a2, b2);
    return r;
  };

  // Row 1: Back up + Restore
  rows.push(pairRow(
    doPhoto("Back up photos now", async () => { await invoke("backup_photos"); return null; }),
    doPhoto("Restore missing photos", async () => {
      const n = await invoke("restore_missing_photos", { referenced: allReferencedPhotos() });
      return n > 0
        ? `Restored ${n} missing photo(s) from the archive.`
        : "No missing photos to restore — everything referenced is already present.";
    })
  ));

  // Row 2: Purge (red, always visible; disabled when nothing is retained) + Open archive
  const openArch = el("button", "action-row", "Open archive folder");
  openArch.style.flex = "1";
  openArch.onclick = () => invoke("open_photo_archive");
  const purge = doPhoto("Purge deleted (30+ days)", async () => {
    const n = await invoke("purge_deleted_photos", { days: 30 });
    return n > 0 ? `Permanently removed ${n} old deleted photo(s).`
                 : "Nothing old enough to purge yet (30-day grace period).";
  }, true);
  if (!status.deleted_pending) {
    purge.disabled = true;
    purge.style.opacity = "0.5";
    purge.style.cursor = "default";
    purge.title = "No deleted photos are being retained yet";
  }
  rows.push(pairRow(purge, openArch));
  rows.push(el("div", "separator"));
  rows.push(sub("Database snapshots"));
  if (!backups.length) rows.push(el("div", "muted", "No snapshots yet."));
  backups.forEach((b) => {
    const r = el("div", "template-row");
    const when = new Date(b.stamp_secs * 1000).toLocaleString();
    const kb = Math.max(1, Math.round(b.size_bytes / 1024));
    r.appendChild(el("div", "t-apply", `${when}  ·  ${kb} KB`));
    const btn = el("button", "ok-btn", "Restore");
    btn.onclick = () => {
      openConfirmRestore(when, async () => {
        const restored = await invoke("restore_backup", { fileName: b.file_name });
        scrim.remove();
        if (!restored) {
          openNotice("Restore", "That snapshot could not be restored (unreadable or invalid). Your current data is unchanged.");
          return;
        }
        S.data = restored;
        S.data.templates = S.data.templates || [];
        sortCollectionsInPlace(S.settings.coll_sort);
        S.selColl = null; S.selItem = null; S.isEditing = false;
        rebuildCollChecked(); rebuildItemChecked();
        renderAll();
        openNotice("Restore", `Restored the snapshot from ${when}.`);
      });
    };
    r.appendChild(btn);
    rows.push(r);
  });
  const head = el("div", "modal-head");
  const back = el("button", "icon-btn", "←");
  back.title = "Back to Settings";
  back.onclick = () => { scrim.remove(); openSettings(); };
  head.appendChild(back);
  const t = el("div", "heading", "Backup / Restore");
  t.style.marginLeft = "6px";
  head.appendChild(t);
  head.appendChild(el("span", "spacer"));
  const x = el("button", "icon-btn", "✖");
  x.onclick = () => scrim.remove();
  head.appendChild(x);
  scrim = modal([head, ...rows], { width: 460 });
}
function openConfirmRestore(when, onYes) {
  let scrim;
  const msg = el("div", "", `Restore the snapshot from ${when}?\nYour current data will be snapshotted first.`);
  msg.style.whiteSpace = "pre-line";
  const row = el("div", "set-row");
  row.appendChild(el("span", "spacer"));
  const cancel = el("button", "plain-btn", "Cancel");
  cancel.onclick = () => scrim.remove();
  const yes = el("button", "ok-btn", "Restore");
  yes.onclick = () => { scrim.remove(); onYes(); };
  row.append(cancel, yes);
  scrim = modal([modalHead("Restore backup", () => scrim), msg, row], { width: 380 });
}

function openValuePanel(cid) {
  let scrim;
  const c = S.data.collections.find((x) => x.id === cid);
  const { list, unvalued } = collectionValueBuckets(cid);
  const rows = [];
  if (!list.length) {
    rows.push(el("div", "muted", "No items in this collection have a value."));
  } else {
    list.forEach((b) => {
      const r = el("div", "set-row");
      r.appendChild(el("span", "", bucketText(b)));
      r.appendChild(el("span", "spacer"));
      r.appendChild(el("span", "muted", `${b.count} item(s)${b.symbol ? "" : " · no currency symbol"}`));
      rows.push(r);
    });
    if (list.length > 1) {
      rows.push(el("div", "muted",
        "Different currencies are listed separately — they are never summed together."));
    }
  }
  if (unvalued) rows.push(el("div", "muted", `${unvalued} item(s) have no value set.`));
  scrim = modal([modalHead(`Total value — ${c ? c.name : ""}`, () => scrim), ...rows], { width: 380 });
}

function openSettings() {
  let scrim;
  const seg = (label, wantDark) => {
    const b = el("button", "seg" + (S.settings.dark_mode === wantDark ? " active" : ""), label);
    b.onclick = () => { S.settings.dark_mode = wantDark; persistSettings(); scrim.remove(); renderAll(); openSettings(); };
    return b;
  };
  const themeRow = el("div", "set-row");
  themeRow.append(el("span", "", "Theme"), el("span", "spacer"));
  const pill = el("div", "seg-pill");
  pill.append(seg("Light", false), seg("Dark", true));
  themeRow.appendChild(pill);

  const accRow = el("div", "set-row");
  accRow.append(el("span", "", "Accent color"), el("span", "spacer"));
  ACCENTS.forEach((hex) => {
    const sw = el("button", "swatch" + (S.settings.accent_hex.toLowerCase() === hex ? " active" : ""));
    sw.style.background = hex;
    sw.onclick = () => { S.settings.accent_hex = hex; persistSettings(); renderAll(); scrim.remove(); openSettings(); };
    accRow.appendChild(sw);
  });

  const fontRow = el("div", "set-row");
  fontRow.append(el("span", "", "Font size"), el("span", "spacer"));
  const dec = el("button", "small-sq", "−");
  const val = el("span", "", String(Math.round(S.settings.font_size)));
  const inc = el("button", "small-sq", "＋");
  const bump = (d) => {
    S.settings.font_size = Math.min(22, Math.max(11, S.settings.font_size + d));
    persistSettings(); applyChrome();
    val.textContent = String(Math.round(S.settings.font_size));
  };
  dec.onclick = () => bump(-1); inc.onclick = () => bump(1);
  fontRow.append(dec, val, inc);

  const act = (label, fn) => { const b = el("button", "action-row", label); b.onclick = fn; return b; };
  const footer = el("div", "settings-footer");
  const logo = el("img", "settings-logo");
  logo.src = "logo.png";
  logo.onerror = () => logo.remove();
  footer.appendChild(logo);
  const verEl = el("div", "muted", "Version …");
  footer.appendChild(verEl);
  invoke("app_version")
    .then((v) => { verEl.textContent = `Version ${v}`; })
    .catch(() => { verEl.textContent = ""; verEl.remove(); });

  scrim = modal([
    modalHead("Settings", () => scrim),
    sub("Appearance"), themeRow, accRow, fontRow,
    sub("Data"),
    act("Export collection data", () => invoke("export_data_cmd", { data: S.data })),
    act("Import collection data", async () => {
      const imported = await invoke("import_data_cmd");
      if (!imported) return;
      const exC = new Set(S.data.collections.map((c) => c.id));
      const exI = new Set(S.data.items.map((i) => i.id));
      imported.collections.forEach((c) => { if (!exC.has(c.id)) S.data.collections.push(c); });
      imported.items.forEach((i) => {
        if (exI.has(i.id)) return;
        i.photos = (i.photos || []).filter((p) => p.trim());
        S.data.items.push(i);
      });
      (imported.templates || []).forEach((t) => {
        if (!S.data.templates.some((x) => x.id === t.id)) S.data.templates.push(t);
      });
      S.selColl = null; S.selItem = null; S.isEditing = false;
      persist(); rebuildCollChecked(); rebuildItemChecked(); renderAll();
    }),
    act("Backup / Restore…", () => { scrim.remove(); openBackups(); }),
    act("Open data folder", () => invoke("open_data_folder")),
    act("Reset panel sizes", () => {
      S.settings.left_ratio = 0.25; S.settings.mid_ratio = 0.333;
      persistSettings(); applyChrome();
    }),
    footer,
  ], { width: 420 });
}

// ─── lightbox (original look: photo on dark scrim, side arrows) ────────────
// Full-resolution photos are delivered as raw JPEG bytes and wrapped in object
// URLs (revocable), cached in a small LRU. Capping the cache bounds memory no
// matter how many photos are opened; evicting or closing revokes the URL so the
// browser frees the decoded image immediately instead of leaving it to GC.
const LB_CACHE_MAX = 5;
const photoCache = new Map(); // name -> object URL (insertion order = LRU order)

function lbCacheGet(name) {
  if (!photoCache.has(name)) return undefined;
  // touch: move to most-recently-used (re-insert at the end)
  const url = photoCache.get(name);
  photoCache.delete(name);
  photoCache.set(name, url);
  return url;
}
function lbCachePut(name, url) {
  if (photoCache.has(name)) {
    URL.revokeObjectURL(photoCache.get(name));
    photoCache.delete(name);
  }
  photoCache.set(name, url);
  // evict least-recently-used until within cap
  while (photoCache.size > LB_CACHE_MAX) {
    const oldest = photoCache.keys().next().value;
    URL.revokeObjectURL(photoCache.get(oldest));
    photoCache.delete(oldest);
  }
}
function lbCacheClear() {
  for (const url of photoCache.values()) URL.revokeObjectURL(url);
  photoCache.clear();
}

async function openLightbox(idx) {
  const it = selectedItem();
  if (!it || !it.photos.length) return;
  S.lightbox = { open: true, index: Math.min(idx, it.photos.length - 1), count: it.photos.length, zoom: 1, panX: 0, panY: 0 };
  const box = el("div", "lightbox");
  box.id = "lightbox";
  const stage = el("div", "lb-stage");
  const im = el("img", "lb-img"); im.id = "lb-img"; im.draggable = false;
  stage.appendChild(im);
  const prev = el("button", "lb-arrow prev", "‹"); prev.onclick = (e) => { e.stopPropagation(); lightboxStep(-1); };
  const next = el("button", "lb-arrow next", "›"); next.onclick = (e) => { e.stopPropagation(); lightboxStep(1); };
  const close = el("button", "lb-close", "✕"); close.onclick = (e) => { e.stopPropagation(); closeLightbox(); };
  const foot = el("div", "lb-foot");
  const counter = el("span", ""); counter.id = "lb-counter";
  foot.appendChild(counter);
  foot.appendChild(el("span", "muted", "Scroll to zoom · drag to pan · Esc to close"));
  box.append(stage, close, foot);
  if (S.lightbox.count > 1) box.append(prev, next);
  // No click-to-close: the ✕ button and Esc close the lightbox, so clicks
  // and drags anywhere on the photo/backdrop are safe.
  $("overlay-root").appendChild(box);

  stage.addEventListener("wheel", (e) => {
    e.preventDefault();
    const prev = S.lightbox.zoom;
    const next = Math.min(6, Math.max(1, prev * (e.deltaY < 0 ? 1.15 : 1 / 1.15)));
    if (next === prev) return;
    if (next === 1) {
      // fully zoomed out: recenter
      S.lightbox.zoom = 1; S.lightbox.panX = 0; S.lightbox.panY = 0;
      requestAnimationFrame(applyLbTransform);
      return;
    }
    // Anchor the zoom on the cursor: keep the image point under the pointer
    // fixed. Cursor position is measured relative to the stage center, since
    // the transform (translate then scale) is applied about that center.
    const rect = stage.getBoundingClientRect();
    const cx = e.clientX - (rect.left + rect.width / 2);
    const cy = e.clientY - (rect.top + rect.height / 2);
    const f = next / prev;
    // pan' = f*pan + cursor*(1 - f)
    S.lightbox.panX = f * S.lightbox.panX + cx * (1 - f);
    S.lightbox.panY = f * S.lightbox.panY + cy * (1 - f);
    S.lightbox.zoom = next;
    requestAnimationFrame(applyLbTransform);
  }, { passive: false });
  let panning = false, px = 0, py = 0;
  stage.addEventListener("pointerdown", (e) => {
    if (e.target !== $("lb-img")) return;
    panning = true; px = e.clientX; py = e.clientY;
    stage.classList.add("panning"); stage.setPointerCapture(e.pointerId);
  });
  stage.addEventListener("pointermove", (e) => {
    if (!panning) return;
    S.lightbox.panX += e.clientX - px; S.lightbox.panY += e.clientY - py;
    px = e.clientX; py = e.clientY;
    requestAnimationFrame(applyLbTransform);
  });
  stage.addEventListener("pointerup", () => { panning = false; stage.classList.remove("panning"); });
  await loadLightboxPhoto();
}
function applyLbTransform() {
  const im = $("lb-img");
  if (im) im.style.transform = `translate(${S.lightbox.panX}px, ${S.lightbox.panY}px) scale(${S.lightbox.zoom})`;
}
async function loadLightboxPhoto() {
  const it = selectedItem();
  if (!it) return;
  const name = it.photos[S.lightbox.index];
  const shown = S.lightbox.index;
  const im = $("lb-img");
  if (!im) return;
  // instant: show a cached full image if we have one, else the thumbnail
  const cached = lbCacheGet(name);
  if (cached) im.src = cached;
  else if (thumbCache.has(name)) im.src = thumbCache.get(name);
  const c = $("lb-counter");
  if (c) c.textContent = `${S.lightbox.index + 1} / ${S.lightbox.count}`;
  if (!cached) {
    // full resolution (maxPx: 0) as raw JPEG bytes → Blob → revocable object URL
    const res = await invoke("photo_bytes", { name, maxPx: 0 });
    if (res) {
      // If the lightbox closed while this was in flight, don't repopulate the
      // just-cleared cache — revoke immediately and bail.
      if (!S.lightbox.open) return;
      const bytes = res[0] instanceof Uint8Array ? res[0] : new Uint8Array(res[0]);
      const url = URL.createObjectURL(new Blob([bytes], { type: "image/jpeg" }));
      lbCachePut(name, url);
      // only swap in if the user hasn't already stepped away
      if (S.lightbox.open && S.lightbox.index === shown && $("lb-img")) {
        $("lb-img").src = url;
      }
    }
  }
}
function lightboxStep(d) {
  if (!S.lightbox.open || !S.lightbox.count) return;
  S.lightbox.index = (S.lightbox.index + d + S.lightbox.count) % S.lightbox.count;
  S.lightbox.zoom = 1; S.lightbox.panX = 0; S.lightbox.panY = 0;
  applyLbTransform(); loadLightboxPhoto();
}
function closeLightbox() {
  S.lightbox.open = false;
  $("lightbox")?.remove();
  // Free every decoded photo immediately — revoke all object URLs rather than
  // leaving them (and their backing bitmaps) live until GC eventually runs.
  lbCacheClear();
}

// ─── global events ───────────────────────────────────────────────────────────
function listHandlers(listEl, isColl) {
  listEl.addEventListener("click", (e) => {
    const cb = e.target.closest("[data-check]");
    const row = e.target.closest("[data-kind]");
    if (!row) return;
    if (e.target.closest("[data-iconpick]")) {
      openIconPicker(+e.target.closest("[data-iconpick]").dataset.iconpick);
      return;
    }
    const idx = +row.dataset.idx;
    if (cb) {
      if (isColl) {
        S.collChecked[+cb.dataset.check] = !S.collChecked[+cb.dataset.check];
        S.collMulti = S.collChecked.some(Boolean);
      } else {
        S.itemChecked[+cb.dataset.check] = !S.itemChecked[+cb.dataset.check];
        S.itemMulti = S.itemChecked.some(Boolean);
      }
      renderAll(); return;
    }
    // manual double-click (single-click re-renders the row node, so the
    // native dblclick event can never fire across the swap)
    const id = row.dataset.id, now = performance.now();
    if (S.lastClick && S.lastClick.id === id && now - S.lastClick.t < 300) {
      S.lastClick = null;
      openRename(isColl ? "coll" : "item", id);
      return;
    }
    S.lastClick = { id, t: now };
    isColl ? selectCollection(idx, e.ctrlKey || e.metaKey, e.shiftKey)
           : selectItem(idx, e.ctrlKey || e.metaKey, e.shiftKey);
  });
  listEl.addEventListener("contextmenu", (e) => {
    const row = e.target.closest("[data-kind]");
    if (!row) return;
    e.preventDefault();
    const id = row.dataset.id, idx = +row.dataset.idx;
    if (!isColl) {
      // iced ItemRightClicked: select only if not multi and not already selected
      const already = S.selItem === id;
      if (!S.itemMulti && !already) selectItem(idx, false, false);
    }
    openContextMenu(isColl, id, e.clientX, e.clientY);
  });
}
function sortCycleHandler(btnId, isColl) {
  $(btnId).addEventListener("click", (e) => {
    closeOverlays();
    const menu = el("div", "ctx-menu");
    const current = isColl ? S.settings.coll_sort : S.settings.item_sort;
    SORTS.forEach((m) => {
      const b = el("button", "ctx-item" + (m === current ? " active" : ""), sortLabel(m, isColl));
      b.onclick = () => { closeOverlays(); isColl ? setCollSort(m) : setItemSort(m); };
      menu.appendChild(b);
    });
    document.body.appendChild(menu);
    const r = e.target.getBoundingClientRect();
    menu.style.left = r.left + "px";
    menu.style.top = r.bottom + 4 + "px";
    e.stopPropagation();
  });
}
function initSplitters() {
  const start = (which) => (e) => { S.dragSplit = which; e.preventDefault(); };
  $("split-left").addEventListener("pointerdown", start(1));
  $("split-mid").addEventListener("pointerdown", start(2));
  document.addEventListener("pointermove", (e) => {
    if (!S.dragSplit) return;
    const w = innerWidth;
    if (S.dragSplit === 1) {
      S.settings.left_ratio = Math.min(0.33, Math.max(0.08, e.clientX / w));
    } else {
      const l = Math.min(0.33, Math.max(0.08, S.settings.left_ratio));
      const leftW = w * l;
      S.settings.mid_ratio = Math.min(0.7, Math.max(0.12, (e.clientX - leftW) / Math.max(1, w - leftW)));
    }
    applyChrome();
  });
  document.addEventListener("pointerup", () => {
    if (S.dragSplit) { S.dragSplit = 0; persistSettings(); }
  });
}
function initSearch(inputId, key, after) {
  const inp = $(inputId);
  const clear = document.querySelector(`.search-clear[data-for="${inputId}"]`);
  inp.addEventListener("input", () => {
    S[key] = inp.value;
    clear.classList.toggle("hidden", !inp.value);
    after();
  });
  clear.addEventListener("click", () => {
    inp.value = ""; S[key] = "";
    clear.classList.add("hidden");
    after();
  });
}
function initKeys() {
  document.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      if (S.isEditing && S.selItem) { flushEditors(); persist(); }
      return;
    }
    if (e.key === "Escape") {
      // iced EscapePressed chain: overlay → searches → clear selection
      if (S.lightbox.open) { closeLightbox(); return; }
      if (document.querySelector(".ctx-menu")) { closeOverlays(); return; }
      const scrims = $("overlay-root").querySelectorAll(".scrim");
      if (scrims.length) { scrims[scrims.length - 1].remove(); return; }
      if (S.itemSearch || S.collSearch) {
        S.itemSearch = S.collSearch = "";
        $("item-search").value = ""; $("coll-search").value = "";
        document.querySelectorAll(".search-clear").forEach((c) => c.classList.add("hidden"));
        rebuildItemChecked(); renderAll(); return;
      }
      clearSelection(); renderAll(); return;
    }
    if (S.lightbox.open) {
      if (e.key === "ArrowLeft") lightboxStep(-1);
      if (e.key === "ArrowRight") lightboxStep(1);
    }
    // Suppress the webview's built-in browser shortcuts this app never mapped.
    // Typing in a field is preserved for the letter-based ones; zoom and reload
    // keys are blocked everywhere (they don't insert text). DevTools (F12 /
    // Ctrl+Shift+I) is intentionally left working for the dev build.
    const typing = e.target.closest("input, textarea");
    const k = e.key.toLowerCase();
    const mod = e.ctrlKey || e.metaKey;
    // find, find-next, print, reload, open, view-source, history nav
    if (mod && !typing && ["f", "g", "p", "r", "o", "u", "j", "h"].includes(k)) {
      e.preventDefault(); return;
    }
    // page zoom: Ctrl +/-/0 — block regardless of focus
    if (mod && ["=", "-", "+", "0"].includes(k)) { e.preventDefault(); return; }
    // reload / find via function keys
    if (k === "f5" || k === "f3") { e.preventDefault(); return; }
  });
  // Block Ctrl/Cmd + mouse-wheel page zoom (a webview default), except inside
  // the lightbox where the wheel is the app's own zoom control.
  window.addEventListener("wheel", (e) => {
    if ((e.ctrlKey || e.metaKey) && !S.lightbox.open) e.preventDefault();
  }, { passive: false });
  document.addEventListener("click", () => closeOverlays());
  document.addEventListener("contextmenu", (e) => {
    // Block the webview's native menu app-wide. On a text field, show our own
    // Copy/Paste/Select All menu instead; elsewhere just dismiss any open menu.
    e.preventDefault();
    const field = e.target.closest("input, textarea");
    if (field) {
      openTextContextMenu(field, e.clientX, e.clientY);
      return;
    }
    if (!e.target.closest("[data-kind]")) closeOverlays();
  });
}

// ─── boot ────────────────────────────────────────────────────────────────────
async function init() {
  const loaded = await invoke("load_all");
  S.data = loaded.data;
  S.data.templates = S.data.templates || [];
  S.settings = loaded.settings;
  S.corrupt = loaded.corrupt_backup;
  rebuildCollChecked(); rebuildItemChecked();

  $("btn-settings").onclick = openSettings;
  $("btn-new-coll").onclick = newCollection;
  $("btn-new-item").onclick = newItem;
  $("btn-del-multi").onclick = deleteSelectedItems;
  listHandlers($("coll-list"), true);
  listHandlers($("item-list"), false);
  sortCycleHandler("coll-sort-btn", true);
  sortCycleHandler("item-sort-btn", false);
  initSearch("coll-search", "collSearch", () => renderCollections());
  initSearch("item-search", "itemSearch", () => { rebuildItemChecked(); renderItems(); });
  initSplitters();
  initKeys();

  // Save the moment the view is hidden (tab/window backgrounded, or the webview
  // about to be suspended while idle). This flushes any in-progress edit to disk
  // before WebView2 can discard the page state, so returning after idle shows
  // your text rather than the last explicitly-saved version. `pagehide` covers
  // the harder suspend/unload case that visibilitychange can miss.
  const flushToDisk = () => {
    if (_persistTimer) { clearTimeout(_persistTimer); _persistTimer = null; }
    if (S.isEditing && S.selItem) flushEditors();
    persist();
  };
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") flushToDisk();
  });
  window.addEventListener("pagehide", flushToDisk);

  renderAll();
}
init();
