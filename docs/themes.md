# Themes

English · [简体中文](themes.zh-CN.md)

Sasha ships built-in **Dark** (default), **Light**, **Claude Dark** and **Claude Light** themes (the Claude pair uses a warm charcoal/ivory palette with a clay accent). Pick one in
**Settings > View > Theme**; it applies immediately and is saved as `theme = "<id>"`
in `config.toml`.

Custom themes are `*.toml` files in the `themes` folder next to `config.toml`
(created on first start; **Settings > View > Theme Folder > Open Folder** opens it).
The file name without `.toml` is the theme id (`ocean.toml` -> `ocean`; `dark`, `light`, `claude-dark`, `claude-light` are reserved). Adding, editing or deleting a file updates the list
automatically, and the active theme is re-applied as soon as its file changes.

A complete example is also in [`themes/example.toml`](themes/example.toml):

```toml
name = "Ocean"          # display name, defaults to the id
base = "dark"           # "dark" (default) or "light"

# Base colors: #RRGGBB or #RRGGBBAA
canvas = "#0B1620"      # window / log background
card = "#12222F"        # send card and raised surfaces
text = "#E3EEF5"        # primary text
accent = "#14B8A6"      # send button and active highlights
rx = "#4ADE80"          # received data
tx = "#7DD3FC"          # sent data (does not follow accent)
error = "#FB7185"       # validation errors
border = "#FFFFFF1C"    # card border

# Optional: override any derived token by its name
[overrides]
bg_strip = "#08111A"
text_muted = "#6B8394"
```

- **Missing fields** fall back to the built-in theme of the same `base`; unknown keys
  are ignored.
- **Derived colors** are computed from the base colors: hover and toggle backgrounds
  from `text` and `accent`, the strip and popover from `canvas` and `card`, the hover
  accent from `accent`, secondary/muted text by blending `text` toward `canvas`, error
  and badge tints from `error` and `accent`, and the connected status from `rx`.
  Dark and light use different blend strengths.
- **`[overrides]`** accepts the token names `bg_canvas`, `bg_card`, `bg_strip`,
  `bg_popover`, `bg_hover`, `bg_toggle_active`, `border_card`, `border_popover`,
  `accent_blue`, `accent_blue_hover`, `rx_green`, `tx_blue`, `text_primary`,
  `text_secondary`, `text_muted`, `error_text`, `error_bg`, `sim_badge_bg`,
  `sim_badge_text`, `status_disconnected` and `status_connected`, and wins over derivation.
- **Errors:** a file with invalid TOML or an invalid color is skipped and reported in
  the Theme Warnings area with its file and field name; other themes still load. If the
  configured theme is missing or broken, Sasha uses the built-in Dark theme and shows a
  warning, but keeps the configured id, so the theme returns when the file is fixed.

## Editing colors in Settings

**Settings > View > Theme > Edit Colors** opens an editor for the eight base colors
(`canvas`, `card`, `text`, `accent`, `rx`, `tx`, `error`, `border`). Each row is a swatch
plus a `#RRGGBB` / `#RRGGBBAA` field; click the swatch to open a color picker (palette,
sliders, alpha), which stays in sync with the field. Valid values preview live, invalid ones are
outlined in red and not applied.

- Built-in themes can't be edited: enter a name and **Save As** to create
  `themes/<name>.toml` (the file name is the lowercased name; reserved names such as
  `dark`, `light` or `con` and names already in use are refused).
- Custom themes are saved back to their own file with **Save**, and can be removed with
  **Delete** (click twice to confirm). Deleting the active theme switches back to Dark.
- Saving rewrites the file: `[overrides]` are kept, comments are not.
- Leaving the editor, picking another theme or closing Settings discards unsaved changes.
