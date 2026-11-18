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
## Live stream output

Click `⇥` in the receive toolbar and choose an output path. Existing files are appended without replacing their content. The button stays highlighted while capturing; click it again to stop, or hover to see the path. Capture is per session, and a file can be used by only one session at a time. Lua debug `log` output is excluded.

Under **Settings → General → Stream output scope**, choose **Current buffer and future** or **Future only** (default). The former snapshots that session's retained visible and paused logs when the file is ready, then records subsequent events in order; cleared or trimmed history and other sessions are excluded. The latter records only events after the file is ready and capture is attached. This global preference is persisted and applies to the next capture; changing it does not backfill an active file again.

The receive text/HEX mode at activation is fixed for that capture. Text follows device LF/CRLF line endings, prefixing each line once with its first byte's timestamp and direction. Consecutive RX chunks continue the same line, and unfinished lines are written immediately. Direction changes separate unfinished lines; TX events remain separate, and stopping terminates an unfinished line. Text is decoded incrementally as UTF-8, preserving readable Unicode, CR/LF, tabs and backslashes. NUL and other controls appear as `\0` or `\xHH`; invalid UTF-8 bytes are also escaped. Only up to three trailing bytes of a split character wait for completion; stopping or changing direction escapes any incomplete bytes. For binary data, select HEX before starting capture; HEX continues to record all bytes per transfer event.

New or empty files receive one UTF-8 BOM to help Windows editors recognize their encoding. Appending to a nonempty file neither inserts another BOM nor changes existing content. UTF-16/UTF-32 BOM files are rejected; choose a new file. Existing BOMless logs that display incorrectly are not automatically repaired; start capture in a new file.

Writes run in the background and flush application buffers after each record. Pausing, clearing, memory trimming, switching tabs and disconnecting do not stop capture. Stopping drains queued records in the background; closing a session waits for them to drain, and normal app exit drains them within the framework shutdown window. File errors or a full writer queue stop capture and display an error above the toolbar.
## Quick send

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
