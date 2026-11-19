# Usage guide

English · [简体中文](usage.zh-CN.md)

## Text and HEX controls

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
## Log filtering

Use the filter icon next to Search in the receive toolbar to show or hide the
filter drawer. It expands upward inside the same bordered toolbar surface.
Click the direction button to cycle through **All → RX → TX → All**.
It starts at **All**. Edits to the direction, keyword and the options below stay
in a draft until you click **Apply** or press Enter in the keyword box; **Apply**
is highlighted while the draft differs from the active filter.
`Aa` enables case sensitivity, `.*` enables regular expressions, and `≠` excludes
matching payloads (for example, heartbeat messages). These switches start off.
The keyword is a single literal string unless regex is enabled; an empty keyword
does not restrict content, even when exclusion is enabled. Direction and content
conditions must both pass. Matching uses whole retained log entries, without
joining adjacent entries or splitting visual wraps.

Only payloads are matched, excluding timestamps and direction tags. Text mode
matches the displayed text; HEX mode matches the displayed space-separated HEX
text: `AA 55` matches those displayed bytes, while `AA55` does not. Regex operates
on that same text, not raw bytes. Changing text/HEX mode reapplies the filter.
An invalid or overly complex regex shows an error and keeps the complete last
valid filter active, including for new data. The count shows visible / cached
entries. `↺` resets the direction and all content options and takes effect
immediately. While a filter is active, the toolbar has a tinted background. The
filter remains active when the drawer is collapsed.

Filters belong to each session and survive tab switches, but are not saved across
app restarts. Search, selection copying and Save operate on filtered entries.
Clear removes **all** displayed-cache entries, including those hidden by the
filter, while keeping the filter settings and byte counters. Filtering does not
affect serial reception, paused data retention, Lua input, or live stream output
(including its initial buffer snapshot). Lua `print` / `log` output and connection
errors remain in their separate panels or status messages. Incoming data updates
the filtered view; automatic scrolling follows only changes to visible entries.

## Log markers

Right-click an entry in the receive log to mark it. A small form asks for a **name** (up to
32 characters) and one of eight **colors**; **OK** (or `Enter`) marks the whole entry with
that color, and `Esc` or a click outside closes the form. A marker belongs to one log row, one
received line or one send, and highlights that whole row. Right-click a marked entry to
rename it, change its color or remove the marker.

- **Mini map**: a narrow strip to the right of the log scrollbar shows where the marked
  entries are in the whole visible log, plus a frame for the part on screen. Click a block to
  scroll to that entry; auto-scroll is switched off so new data does not pull the view away.
- **Markers section**: the right dock lists every marker of the session below **Quick send**,
  with its name, color, timestamp and direction. Click a row to scroll to it, use the pencil to
  edit it or `×` to remove it, and the `×` in the title to remove all markers (with a
  confirmation). Entries hidden by the log filter stay in the list, greyed out and not
  clickable, and they are also left off the mini map until the filter is cleared. The section
  can be collapsed and is hidden when the right dock is narrow.

Markers are kept in memory for the session only: they are not saved to `config.toml`, not
written to exports or live stream output, and they disappear when the log is cleared or when
the log buffer limit drops the marked entry.

## Live stream output

Click `⇥` in the receive toolbar and choose an output path. Existing files are appended without replacing their content. The button stays highlighted while capturing; click it again to stop, or hover to see the path. Capture is per session, and a file can be used by only one session at a time. Lua debug `log` output is excluded.

Under **Settings → General → Stream output scope**, choose **Current buffer and future** or **Future only** (default). The former snapshots that session's retained visible and paused logs when the file is ready, then records subsequent events in order; cleared or trimmed history and other sessions are excluded. The latter records only events after the file is ready and capture is attached. This global preference is persisted and applies to the next capture; changing it does not backfill an active file again.

The receive text/HEX mode at activation is fixed for that capture. Text follows device LF/CRLF line endings, prefixing each line once with its first byte's timestamp and direction. Consecutive RX chunks continue the same line, and unfinished lines are written immediately. Direction changes separate unfinished lines; TX events remain separate, and stopping terminates an unfinished line. Text is decoded incrementally as UTF-8, preserving readable Unicode, CR/LF, tabs and backslashes. NUL and other controls appear as `\0` or `\xHH`; invalid UTF-8 bytes are also escaped. Only up to three trailing bytes of a split character wait for completion; stopping or changing direction escapes any incomplete bytes. For binary data, select HEX before starting capture; HEX continues to record all bytes per transfer event.

New or empty files receive one UTF-8 BOM to help Windows editors recognize their encoding. Appending to a nonempty file neither inserts another BOM nor changes existing content. UTF-16/UTF-32 BOM files are rejected; choose a new file. Existing BOMless logs that display incorrectly are not automatically repaired; start capture in a new file.

Writes run in the background and flush application buffers after each record. Pausing, clearing, memory trimming, switching tabs and disconnecting do not stop capture. Stopping drains queued records in the background; closing a session waits for them to drain, and normal app exit drains them within the framework shutdown window. File errors or a full writer queue stop capture and display an error above the toolbar.
## Quick send

The **Quick send** section sits in the right dock below **Scripts**. Each saved command stores `id`, `name`, optional `group`, `content`, `hex`, and `line_ending` (`"none"`, `"crlf"`, or `"lf"`, ignored when `hex = true`), and `checksum` (`"none"` by default, or `"sum8"`, `"xor8"`, `"crc8"`, `"crc16_modbus"`, `"crc16_ccitt"`, `"crc32"`; appended to the parsed bytes only when `hex = true`; CRC-16/Modbus is sent low byte first, other multi-byte checksums high byte first). Click any command to send it directly using its own HEX/line-ending settings without modifying the send card draft, or press `Alt+1` … `Alt+9` (the modifier is configurable, see [Keyboard shortcuts](#keyboard-shortcuts)) in the workspace to send the first 9 commands in display order (across groups, regardless of search filter or collapsed groups). Commands are shared across sessions and persisted in `config.toml`:

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
## Updates

Sasha checks GitHub Releases once at startup (**Settings → General → Check for
Updates** turns this off) and from **Settings → About → Check for Updates**. Only the
request to the GitHub API is made; nothing else is sent. When a newer version exists, a
dot appears next to **Settings** in the sidebar.

**Download & Install** fetches the release executable, verifies its SHA-256 checksum,
replaces the running executable and asks you to restart. The old file is removed on the
next launch. In-app installation requires the release to contain
`sasha-windows-x86_64.exe` plus a checksum (GitHub's asset digest, or a
`sasha-windows-x86_64.exe.sha256` file) and the executable's folder to be writable;
otherwise use **Release Notes** to download it manually. Versions are compared as
semantic versions, so `0.0.1-b` is older than `0.0.1`.

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

## Keyboard shortcuts

**Settings → Shortcuts** lists the shortcuts you can change. Click **Change** on a row and
press the new keys (`Esc` cancels); **Default** restores one row and **Reset all** restores
every row. Changes apply immediately and are saved in `config.toml`.

| Action | Default | What you can change |
| --- | --- | --- |
| New session | `Ctrl+T` | The whole key |
| Close session | `Ctrl+W` | The whole key |
| Next session | `Ctrl+Tab` | The whole key |
| Previous session | `Ctrl+Shift+Tab` | The whole key |
| Open log search | `Ctrl+F` | The whole key |
| Jump to session 1–9 | `Ctrl+1` … `Ctrl+9` | The modifier keys only |
| Send quick command 1–9 | `Alt+1` … `Alt+9` | The modifier keys only |

For the two 1–9 rows, press the modifier keys together with any digit, for example
`Ctrl+Alt+1`; only the modifiers are kept. `Ctrl+9` jumps to the last session. Keys used
by editing and the system are reserved and refused: `Enter`, `Esc`, `F3`, `Alt+F4`, and
`Ctrl`/`Cmd` with `A`, `C`, `V`, `X`, `Z` or `Y`.

Rules: a shortcut needs `Ctrl`, `Alt` or `Cmd` (Shift alone is not enough), and two actions
cannot share a key. A key that is already taken is refused and the message names the action
using it. A single key also cannot be `Ctrl+1` … `Ctrl+9` while the jump modifiers are `Ctrl`.

```toml
[shortcuts]
new_session = "ctrl-n"
close_session = "ctrl-w"
next_session = "ctrl-tab"
prev_session = "ctrl-shift-tab"
search = "ctrl-f"
jump_session = "ctrl"
quick_send = "ctrl-alt"
```

Values are lower-case and joined with `-`; the last part of a single key is the key name
(`t`, `tab`, `f3`), and the 1–9 rows hold only modifiers (`ctrl`, `alt`, `shift`, `cmd`).
A missing, invalid, reserved or clashing entry keeps its default. If a default then collides
with one of your custom keys, those custom entries go back to their defaults as well; the rest
is kept. Other settings are not affected.

## Command line

For automation without a window, see [Command line](cli.md) (`sasha-cli`).
