<p align="center">
  <img src="assets/icons/sasha-arrows-transparent.png" alt="Sasha" width="96" />
</p>

<h1 align="center">Sasha</h1>
<p align="center">English · <a href="README.zh-CN.md">简体中文</a></p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--only-blue" alt="License: GPL-3.0-only" /></a>
  <img src="https://img.shields.io/badge/Rust-2024_edition-orange" alt="Rust 2024 edition" />
  <img src="https://img.shields.io/badge/platform-Windows-lightgrey" alt="Platform: Windows" />
</p>

Sasha is an open-source serial port debugging tool for the desktop, written in Rust
with GPUI. It combines multiple independent sessions, text and HEX inspection, quick
and timed sending, Lua scripting, live log streaming, and customizable themes in a
compact, native interface.

## Preview

<table>
  <tr>
    <th width="50%">Connect your device</th>
    <th width="50%">Inspect and send data</th>
  </tr>
  <tr>
    <td><a href="docs/images/sasha-connection.png"><img src="docs/images/sasha-connection.png" alt="Serial port selection and communication settings" width="100%" /></a></td>
    <td><a href="docs/images/sasha-terminal.png"><img src="docs/images/sasha-terminal.png" alt="Example terminal session showing received and sent data" width="100%" /></a></td>
  </tr>
</table>

Click either screenshot to view it at full size.

The project is under active development. Windows is the current development and
validation platform; macOS and Linux builds have not been verified.

## Features

- **Multiple sessions** — open, switch, and close connections from the right sidebar.
- **Serial configuration** — discover available ports or enter one manually; configure
  baud rate, data bits, parity, stop bits, flow control, and DTR/RTS signals.
- **Receive inspection** — switch between text and HEX, toggle timestamps, pause
  display updates, control auto-scroll, search the log, select and copy text, and
  export visible logs.
- **Flexible sending** — send UTF-8 text or HEX bytes, with optional CRLF or LF
  endings for text messages. Input HEX display and HEX sending are separate controls.
- **Quick and timed send** — keep reusable commands in a grouped panel with `Alt+1` …
  `Alt+9` shortcuts, or repeat the send field content at a fixed interval.
- **Lua scripting** — automate serial workflows with the built-in Lua 5.4 runtime; no
  separate Lua installation is needed. See the [scripting API](docs/lua-scripting-api.md).
- **Live stream output** — append all RX/TX logs of a session to a file in real time.
- **Themes** — built-in Dark and Light themes, custom TOML themes with live reload, and
  an optional workspace background image.
- **Connection feedback** — monitor connection state and RX/TX byte counters.
- **Native editing** — a blinking block caret in the send field, with normal text
  selection and editing behavior.
- **Preferences** — switch between English and Simplified Chinese and adjust log
  font size. Global preferences are stored locally in TOML.

## Build and run

You need Git, Rust and Cargo with support for the Rust 2024 edition, and the native
build tools required by your Rust target. On Windows, use the MSVC toolchain with
Visual Studio C++ Build Tools and a Windows SDK installed.

```powershell
git clone https://github.com/yanggx98/Sasha.git
cd Sasha
cargo run --locked
```

To create an optimized executable:

```powershell
cargo build --release --locked
```

The Windows executable is written to `target/release/sasha.exe`.
The first build downloads dependencies; subsequent builds can run offline once
those dependencies are cached.

## Getting started

1. Select a detected serial port, or enter its name manually.
2. Set the communication parameters to match your device, then connect.
3. Inspect incoming data in the log area and enter outgoing data in the send field.
4. Use the arrow button to send. Create additional sessions from the sidebar.

### Text and HEX controls

The three HEX controls are independent:

| Control | What it changes |
| --- | --- |
| Receive HEX display | How log payloads are displayed. |
| Send-field `H` | How the draft is displayed and converted between text and HEX. |
| HEX send mode in the receive toolbar | Whether sending encodes the draft as UTF-8 text or parses it as HEX bytes. |

HEX input uses space-separated byte pairs, such as `0A FF 1B`.
Text mode can append no line ending, CRLF (`\r\n`), or LF (`\n`); HEX mode sends the
parsed bytes without appending a text line ending. Converting HEX back to text
requires valid UTF-8.

### Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| `Ctrl+T` | New session |
| `Ctrl+W` | Close the current session |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous session |
| `Ctrl+1` … `Ctrl+8` | Switch to the corresponding session |
| `Ctrl+9` | Switch to the last session |
| `Alt+1` … `Alt+9` | Send the corresponding command in the quick send list |
| `Ctrl+C` | Copy selected text from the focused editor or log area |
| `Enter` / `Esc` in a parameter picker | Confirm / cancel the parameter change |

### Live stream output

Click `⇥` in the receive toolbar and choose an output path. Existing files are appended without replacing their content. The button stays highlighted while capturing; click it again to stop, or hover to see the path. Capture is per session, and a file can be used by only one session at a time. Lua debug `log` output is excluded.

Under **Settings → General → Stream output scope**, choose **Current buffer and future** or **Future only** (default). The former snapshots that session's retained visible and paused logs when the file is ready, then records subsequent events in order; cleared or trimmed history and other sessions are excluded. The latter records only events after the file is ready and capture is attached. This global preference is persisted and applies to the next capture; changing it does not backfill an active file again.

The receive text/HEX mode at activation is fixed for that capture. Text follows device LF/CRLF line endings, prefixing each line once with its first byte's timestamp and direction. Consecutive RX chunks continue the same line, and unfinished lines are written immediately. Direction changes separate unfinished lines; TX events remain separate, and stopping terminates an unfinished line. Text is decoded incrementally as UTF-8, preserving readable Unicode, CR/LF, tabs and backslashes. NUL and other controls appear as `\0` or `\xHH`; invalid UTF-8 bytes are also escaped. Only up to three trailing bytes of a split character wait for completion; stopping or changing direction escapes any incomplete bytes. For binary data, select HEX before starting capture; HEX continues to record all bytes per transfer event.

New or empty files receive one UTF-8 BOM to help Windows editors recognize their encoding. Appending to a nonempty file neither inserts another BOM nor changes existing content. UTF-16/UTF-32 BOM files are rejected; choose a new file. Existing BOMless logs that display incorrectly are not automatically repaired; start capture in a new file.

Writes run in the background and flush application buffers after each record. Pausing, clearing, memory trimming, switching tabs and disconnecting do not stop capture. Stopping drains queued records in the background; closing a session waits for them to drain, and normal app exit drains them within the framework shutdown window. File errors or a full writer queue stop capture and display an error above the toolbar.

### Quick send

The **Quick send** section sits in the right dock below **Scripts**. Each saved command stores `id`, `name`, optional `group`, `content`, `hex`, and `line_ending` (`"none"`, `"crlf"`, or `"lf"`, ignored when `hex = true`). Click any command to send it directly using its own HEX/line-ending settings without modifying the send card draft, or press `Alt+1` … `Alt+9` in the workspace to send the first 9 commands in display order (across groups, regardless of search filter or collapsed groups). Commands are shared across sessions and persisted in `config.toml`:

```toml
[[quick_commands]]
id = "qc-1"
name = "AT"
group = "Init"
content = "AT"
hex = false
line_ending = "crlf"

[[quick_commands]]
id = "qc-2"
name = "Ping"
group = "Init"
content = "AA 55 01"
hex = true
```

Invalid or duplicate-id entries in `config.toml` are skipped on load without discarding the rest of the configuration, and a notice is shown in the Quick send section.

## Timed send

An always-visible capsule `[⏱ | 1000 ms]` sits immediately left of the send button in the send card's bottom row and repeats the input box content at a fixed interval. Clicking the clock icon only toggles **enabled** (enter or leave timed-send mode); it never sends by itself. The right part is the interval (a whole number from 10 to 3600000 ms) and the last value is remembered (`timer_interval_ms` in `config.toml`). The capsule is greyed out while disconnected and cannot be enabled.

Three states:

- **Off**: grey outline on a transparent fill; the send button is the normal send `↑`.
- **Enabled (standby)**: solid accent fill; the send button becomes Play `▶`. Click it (or press Ctrl+Enter in the input box) to start. The interval draft and the content are validated first, and an invalid one shows an error and does not start.
- **Running**: a breathing dot on the icon and the send button becomes Pause `⏸`. Pause stops sending but stays enabled, and the button goes back to `▶`. Ctrl+Enter in the input box is ignored with a hint while running (it is never treated as pause). Clicking the clock icon again cancels the enabled state (stopping a running timer too).

The first send happens immediately on start, then at a fixed rate. If the app stalls, missed cycles are skipped rather than replayed. If the port cannot keep up with the interval (e.g. low baud rate), cycles are also skipped once too many timed sends are waiting, so the TX queue never fills up.

- While enabled (including standby) manual sending is disabled: normal send from the input box, `Alt+1` … `Alt+9` and the quick send panel do nothing and show a hint. Script sends are not blocked.
- The input text, the H toggle, bottom-bar HEX send and line ending can be edited while running and apply from the next cycle. The interval field holds a draft: it is committed on Enter or when the field loses focus and then applies from the next cycle (a soft tint marks an uncommitted draft). Enter only commits and never starts. An invalid draft is reverted to the previous valid interval with an error.
- Every cycle re-validates the draft before sending: empty content or invalid HEX stops the timer with an error and the capsule stays enabled, so you can fix the content and press Play again.
- Pause, a write failure, or scripts or quick send filling the TX queue stop the timer with an error (and stay enabled); a disconnect, closing the session or cancelling the enabled state also stops it and clears the enabled state. Reconnecting does not resume it. The state is per session and is not persisted.

## Preferences

Open **Settings** from the sidebar. Preferences are saved to `config.toml` beside
the executable. The settings page shows the resolved configuration path.

```toml
version = 1
locale = "en-US"
log_font_size = 14
stream_output_scope = "future_only" # or "current_buffer_and_future"
theme = "dark"                # built-in id or a file in themes/
```

Supported locale values are `en-US` and `zh-CN`; the default is `zh-CN`.
The log font size presets are 12, 13, 14, and 15 logical pixels.
Session drafts and connection states are not restored from this file.
If a malformed TOML file is detected, Sasha attempts to back it up as
`config.toml.bak` and falls back to defaults.

## Themes

Sasha ships a built-in **Dark** (default) and **Light** theme. Pick one in
**Settings > View > Theme**; it applies immediately and is saved as `theme = "<id>"`
in `config.toml`.

Custom themes are `*.toml` files in the `themes` folder next to `config.toml`
(created on first start; **Settings > View > Theme Folder > Open Folder** opens it).
The file name without `.toml` is the theme id (`ocean.toml` -> `ocean`; `dark` and
`light` are reserved). Adding, editing or deleting a file updates the list
automatically, and the active theme is re-applied as soon as its file changes.

A complete example is also in [`docs/themes/example.toml`](docs/themes/example.toml):

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

## Current boundaries

- The visible log history retains the most recent **2,000 entries**. Export saves
  the currently visible history in its current display format, not a full raw capture.
- Pausing freezes log display, not the serial connection. Buffered entries are
  added when display resumes.
- Real serial connections support one or two stop bits. The 1.5-stop-bit option
  is rejected by the current backend.

## Development

```powershell
cargo check --locked
cargo test --locked
```

Tests cover data formatting, send validation, session behavior, configuration,
scripting, and stream output. Hardware communication and desktop interaction also
need manual testing on the target system.

```text
src/
  main.rs          Application startup
  i18n.rs          English and Simplified Chinese strings
  serial.rs        Port discovery and serial I/O
  session_service.rs  Per-session I/O, send queue, and scripting bridge
  model/           Sessions, logs, quick commands, timed send, stream output, config
  scripting/       Lua runtime and the `serial` / `task` / `log` APIs
  ui/              Workspace, connection page, send card, panels, themes, settings
assets/icons/      Application artwork
locales/           English and Simplified Chinese strings
docs/              Lua API reference, theme example, README images
scripts/           Example Lua scripts
```

## Acknowledgements

Sasha is built on the work of these open-source projects:

- **[GPUI-KIT](https://github.com/longbridge/gpui-kit)** — the UI toolkit and
  component library used by Sasha.
- **[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui)** — Zed's
  GPU-accelerated UI framework, maintained in the
  [Zed repository](https://github.com/zed-industries/zed).

AI tools used during the development of Sasha include **DeepSeek, Grok, ChatGPT,
Gemini, and Claude**.

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for the workflow and
checks to run before opening a pull request.

## Feedback

Report bugs and suggest improvements through
[GitHub Issues](https://github.com/yanggx98/Sasha/issues). Include your Windows
version, application revision, reproduction steps, and relevant serial parameters.
For UI issues, a screenshot and display scaling setting are helpful.

## License

Sasha is licensed under the **GNU General Public License v3.0 only
(GPL-3.0-only)**. See [LICENSE](LICENSE) for the full terms.

Dependencies, including GPUI (Apache-2.0), retain their respective licenses.
