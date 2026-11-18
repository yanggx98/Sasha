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
validation platform; macOS and Linux builds have not been verified on real hardware.
CI compiles and tests on macOS and Ubuntu, and feedback from macOS and Linux users is
welcome.

## Features

- **Multiple sessions** — independent connections, switched from the right sidebar.
- **Full serial configuration** — port discovery, baud rate, frame format, flow control, DTR/RTS.
- **Receive inspection** — text or HEX, timestamps, search, pause, copy, export.
- **Flexible sending** — UTF-8 text or HEX bytes, plus quick-send commands and timed send.
- **Lua scripting** — built-in Lua 5.4 to automate serial workflows; no separate install.
- **Live stream output** — append all RX/TX logs of a session to a file in real time.
- **Themes** — Dark and Light, custom TOML themes with live reload, optional background image.
- **Updates** — checks GitHub Releases and can install new versions in the app.
- **English and Simplified Chinese** UI.

## Quick start

Build from source (Git and Rust with 2024 edition support; on Windows, the MSVC
toolchain with Visual Studio C++ Build Tools and a Windows SDK):

```powershell
git clone https://github.com/yanggx98/Sasha.git
cd Sasha
cargo run --locked            # or: cargo build --release --locked  ->  target/release/sasha.exe
```

1. Select a detected serial port, or enter its name manually.
2. Set the communication parameters to match your device, then connect.
3. Read incoming data in the log area and type outgoing data in the send field.
4. Press the arrow button to send. Open more sessions from the sidebar.

### Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| `Ctrl+T` / `Ctrl+W` | New / close session |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous session |
| `Ctrl+1` … `Ctrl+8`, `Ctrl+9` | Jump to session 1–8 / the last session |
| `Alt+1` … `Alt+9` | Send the matching quick-send command |
| `Ctrl+C` | Copy selected text |

## Documentation

- [Usage guide](docs/usage.md) — HEX controls, live stream output, quick send, timed
  send, updates, and the `config.toml` preferences.
- [Themes](docs/themes.md) — built-in and custom themes.
- [Lua scripting API](docs/lua-scripting-api.md)
- [Contributing](CONTRIBUTING.md)

## Current boundaries

- The visible log history retains the most recent **2,000 entries**. Export saves the
  visible history in its current display format, not a full raw capture.
- Pausing freezes log display, not the serial connection.
- Real serial connections support one or two stop bits; 1.5 is rejected by the backend.
- Only a Latin monospace font is bundled; other scripts such as Chinese use system fallback fonts. On Linux, install a CJK font (e.g. `fonts-noto-cjk`) if they show as boxes.

## Acknowledgements

Built on [GPUI-KIT](https://github.com/longbridge/gpui-kit) and
[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui). The bundled monospace
font is [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono) (SIL OFL 1.1, see
`assets/fonts/OFL-JetBrainsMono.txt`). AI tools used
during development include DeepSeek, Grok, ChatGPT, Gemini, and Claude.

## Contributing and feedback

Pull requests are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md). Report bugs through
[GitHub Issues](https://github.com/yanggx98/Sasha/issues) with your Windows version,
application revision, reproduction steps, and serial parameters (a screenshot and
display scaling help for UI issues).

## License

Sasha is licensed under the **GNU General Public License v3.0 only
(GPL-3.0-only)**. See [LICENSE](LICENSE) for the full terms.

Dependencies, including GPUI (Apache-2.0), retain their respective licenses.
