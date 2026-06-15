# Assets

The binary embeds three fonts at compile time via `include_bytes!` in
`src/main.rs`. Drop these files in `assets/fonts/` before building. They are not
committed here because of their size/licensing; all three are open-licensed and
freely downloadable.

| File | Source | License |
|------|--------|---------|
| `NotoSans-Regular.ttf`      | Google Noto Sans            | OFL 1.1 |
| `NotoSansCJKsc-Regular.otf` | Google Noto Sans CJK SC     | OFL 1.1 |
| `NotoColorEmoji.ttf`        | Google Noto Color Emoji     | OFL 1.1 |

Quick fetch (any one source works):

```sh
cd assets/fonts
# Noto Sans
curl -LO https://github.com/notofonts/notofonts.github.io/raw/main/fonts/NotoSans/hinted/ttf/NotoSans-Regular.ttf
# Noto Sans CJK SC (OTF)
curl -L -o NotoSansCJKsc-Regular.otf \
  https://github.com/notofonts/noto-cjk/raw/main/Sans/OTF/SimplifiedChinese/NotoSansCJKsc-Regular.otf
# Noto Color Emoji
curl -LO https://github.com/googlefonts/noto-emoji/raw/main/fonts/NotoColorEmoji.ttf
```

## Why this fixes the emoji clipping issue

iced renders text in a line box sized by `line_height`. In `src/view.rs` every
emoji glyph goes through `App::emoji_text`, which sets the color-emoji font and
`line_height(1.3)`. Because the line box scales with the font size, the glyph's
ascent/descent are always contained — so emoji do not clip at any font size,
including the largest the settings panel allows (22px).

## Window/taskbar icon (optional, Windows)

Put `assets/icons/Collectors-Notebook.ico` here and `build.rs` will embed it in
the `.exe`. On macOS/Linux the icon is set by the desktop entry / bundle, not by
this build script.
