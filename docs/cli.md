# Command line (`sasha-cli`)

`sasha-cli` is a console program for automation and scripting. It opens a serial port,
does one job and exits with a documented exit code. It ships next to `sasha` (the GUI);
build it with `cargo build --release --locked` (`target/release/sasha-cli`).

```text
sasha-cli ports   [--json]
sasha-cli send    --port <PORT> [serial options] (--text <TEXT> | --hex <HEX>) [options]
sasha-cli monitor --port <PORT> [serial options] [options]
sasha-cli run     <SCRIPT.lua> --port <PORT> [serial options] [--timeout <MS>]
```

Messages are in English. Normal results go to stdout; errors and diagnostics go to stderr.
Every command opens the port itself and closes it before exiting (`ports` does not open
any port). A port that is already open elsewhere, such as in the Sasha window, is not
retried: the command fails with exit code 3.

## Serial options

Used by `send`, `monitor` and `run`. Defaults match the GUI.

| Option | Default | Values |
| --- | --- | --- |
| `--port <NAME>` | required | `COM3`, `/dev/ttyUSB0`, … |
| `--baud <N>` | `115200` | greater than 0 |
| `--data-bits <N>` | `8` | `5`–`8` |
| `--parity <P>` | `none` | `none`, `odd`, `even` |
| `--stop-bits <N>` | `1` | `1`, `1.5`, `2` (1.5 is rejected by real ports) |
| `--flow-control <F>` | `none` | `none`, `hardware`, `software` |
| `--dtr` / `--rts` | low | present = drive the line high when opening |

## `ports`

Lists the serial ports, one `name<TAB>description` line each. With `--json`, prints a JSON
array of `{"name": "...", "description": "..."}` objects.

## `send`

Sends once, optionally waits for a response, then exits.

| Option | Meaning |
| --- | --- |
| `--text <TEXT>` | Send UTF-8 text. Exactly one of `--text` / `--hex` is required. |
| `--hex <HEX>` | Send bytes given as hex pairs, `"AA BB 01"` or `"AABB01"`, any case. |
| `--line-ending none\|crlf\|lf` | Appended to `--text` (default `none`). |
| `--checksum <ALGO>` | Append a checksum to `--hex`: `sum8`, `xor8`, `crc8`, `crc16_modbus`, `crc16_ccitt`, `crc32`. Not allowed with `--text`. The checksum covers all the bytes given; CRC-16/Modbus is sent low byte first, other multi-byte checksums high byte first. |
| `--expect <REGEX>` | After sending, wait until the received data matches this regular expression. Works on bytes, so binary responses are fine. |
| `--timeout <MS>` | How long to wait for `--expect` (default `1000`). Needs `--expect`. |
| `--json` | Print the result as JSON. |

Without `--expect` the command prints nothing and exits `0` once the bytes are written.
With `--expect`, stdout is the received data up to the end of the match. With `--json` it is
`{"matched": true, "response_hex": "4F 4B", "response_text": "OK"}` (`response_text` is a
lossy UTF-8 decoding); without `--expect`, `--json` prints `{"sent": true}`.

Receiving starts before the bytes are written, so a response that arrives immediately is
not missed.

```text
sasha-cli send --port COM3 --text "AT" --line-ending crlf --expect "OK" --timeout 2000
sasha-cli send --port COM3 --hex "01 03 00 00 00 0A" --checksum crc16_modbus
```

## `monitor`

Listens without sending and prints what arrives.

| Option | Meaning |
| --- | --- |
| `--duration <MS>` | Stop after this long. |
| `--until <REGEX>` | Stop as soon as the received data matches. |
| `--hex` | Print each received chunk as space-separated hex. |
| `--timestamp` | Prefix each line with `[HH:MM:SS.mmm]`. |
| `--json` | One JSON object per chunk: `{"ts": "...", "direction": "rx", "hex": "...", "text": "..."}`. Takes precedence over `--hex`. |

In text mode output is split into lines at `\n`; a trailing `\r` is dropped and an unfinished
line is printed when the command ends. With neither `--duration` nor `--until`, it listens
until you press `Ctrl+C`, which closes the port and exits with `0`. If the device is
unplugged while listening, the command reports it on stderr and exits with `3`.

```text
sasha-cli monitor --port COM3 --until "READY" --duration 10000 --timestamp
```

## `run`

Runs a Lua script against the port without a window, using the same API as the app (see
the [Lua scripting API](lua-scripting-api.md)). Script output (`print`, `log.*`) goes to
stdout as `[INFO] ...`, `[WARN] ...` and `[ERROR] ...`. `--timeout <MS>` stops the script
after that long.

```text
sasha-cli run scripts/periodic_send.lua --port COM3 --timeout 5000
```

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success (also `--help` / `--version`). |
| `1` | Failure: the script failed, a write failed, the script file could not be read. |
| `2` | Usage error: bad or conflicting options, invalid hex, invalid regular expression. Nothing is sent and no port is opened. |
| `3` | The port could not be opened (including when it is busy), or it disconnected while running. |
| `4` | Timeout: `send --expect` got no match in time, `monitor --until` did not match before `--duration`, or `run --timeout` expired. |

## Updates

When the app updates itself from a release, it also refreshes `sasha-cli` if that file sits
next to the app executable.
