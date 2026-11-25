# 命令行（`sasha-cli`）

`sasha-cli` 是面向自动化和脚本调用的控制台程序：打开串口，完成一件事，然后以约定的退出码退出。
它与图形界面程序 `sasha` 放在一起，用 `cargo build --release --locked` 构建（`target/release/sasha-cli`）。

```text
sasha-cli ports   [--json]
sasha-cli send    --port <端口> [串口选项] (--text <文本> | --hex <HEX>) [选项]
sasha-cli monitor --port <端口> [串口选项] [选项]
sasha-cli run     <脚本.lua> --port <端口> [串口选项] [--timeout <毫秒>]
```

命令行提示和错误信息只提供英文。正常结果写 stdout，错误和诊断写 stderr。每个命令自己打开串口、
退出前关闭（`ports` 不打开任何端口）。只有 `monitor` 会优雅处理 `Ctrl+C`；中断 `send` 或 `run` 时进程由系统结束，串口也会被系统释放。端口已被其他程序占用（比如已在 Sasha 窗口里打开）时不会重试，
直接以退出码 3 失败。

## 串口选项

`send`、`monitor`、`run` 共用，默认值与界面一致。

| 选项 | 默认 | 取值 |
| --- | --- | --- |
| `--port <名称>` | 必填 | `COM3`、`/dev/ttyUSB0` 等 |
| `--baud <N>` | `115200` | 大于 0 |
| `--data-bits <N>` | `8` | `5`–`8` |
| `--parity <P>` | `none` | `none`、`odd`、`even` |
| `--stop-bits <N>` | `1` | `1`、`2`（`1.5` 会按用法错误拒绝） |
| `--flow-control <F>` | `none` | `none`、`hardware`、`software` |
| `--dtr` / `--rts` | 低电平 | 写出该选项表示打开时把对应线拉高 |

## `ports`

每行输出一个串口：`名称<TAB>描述`。加 `--json` 时输出 JSON 数组，元素为 `{"name": "...", "description": "..."}`。

## `send`

发送一次，可选等待响应，然后退出。

| 选项 | 含义 |
| --- | --- |
| `--text <文本>` | 发送 UTF-8 文本。`--text` 与 `--hex` 必须二选一。 |
| `--hex <HEX>` | 发送十六进制字节，`"AA BB 01"` 或 `"AABB01"`，大小写均可。 |
| `--line-ending none\|crlf\|lf` | 追加到 `--text` 之后（默认 `none`）。 |
| `--checksum <算法>` | 给 `--hex` 追加校验：`sum8`、`xor8`、`crc8`、`crc16_modbus`、`crc16_ccitt`、`crc32`。不能与 `--text` 同用。校验范围是给出的全部字节；CRC-16/Modbus 低字节在前，其余多字节校验高字节在前。 |
| `--expect <正则>` | 发送后等待接收数据匹配该正则。按字节匹配，二进制响应也可以。正则默认按 Unicode 解释，所以 `\xAA` 表示字符 U+00AA；要匹配原始字节，请写 `(?-u:\xAA)`，例如 `--expect '(?-u:\x01\x03)'`。 |
| `--timeout <毫秒>` | 等待 `--expect` 的时长（默认 `1000`），必须与 `--expect` 一起使用。 |
| `--json` | 以 JSON 输出结果。 |

没有 `--expect` 时，字节写出后不输出任何内容并以 `0` 退出。有 `--expect` 时，stdout 是接收到的数据（到匹配结束处）；
加 `--json` 时为 `{"matched": true, "response_hex": "4F 4B", "response_text": "OK"}`（`response_text` 为有损 UTF-8 解码）；
没有 `--expect` 而加 `--json` 时输出 `{"sent": true}`。

发送前就已开始接收，因此立刻到达的响应不会丢失。

```text
sasha-cli send --port COM3 --text "AT" --line-ending crlf --expect "OK" --timeout 2000
sasha-cli send --port COM3 --hex "01 03 00 00 00 0A" --checksum crc16_modbus
```

## `monitor`

只监听、不发送，把收到的数据打印出来。

| 选项 | 含义 |
| --- | --- |
| `--duration <毫秒>` | 到时间后停止。 |
| `--until <正则>` | 接收数据一匹配就停止。 |
| `--hex` | 每个接收块按空格分隔的十六进制输出。 |
| `--timestamp` | 每行前加 `[HH:MM:SS.mmm]`。 |
| `--json` | 每个接收块一个 JSON 对象：`{"ts": "...", "direction": "rx", "hex": "...", "text": "..."}`，优先于 `--hex`。 |

文本模式下，不是合法 UTF-8 的字节会被替换成 `�`；二进制或非 UTF-8（如 GBK）数据请用 `--hex` 或 `--json`。文本模式按 `\n` 分行，去掉行尾的 `\r`，结束时把未写完的一行也输出。不设 `--duration` 和 `--until` 时一直监听，
按 `Ctrl+C` 会先关闭串口再以 `0` 退出。监听中设备被拔出时，向 stderr 报错并以 `3` 退出。

```text
sasha-cli monitor --port COM3 --until "READY" --duration 10000 --timestamp
```

## `run`

不开窗口，对串口运行 Lua 脚本，API 与应用内一致（见 [Lua 脚本 API](lua-scripting-api.md)）。脚本用
`output.print` 输出的内容原样写到 stdout。调试日志（`print`、`log.*`）默认不显示，加 `--script-debug`
后以 `[INFO] ...`、`[WARN] ...`、`[ERROR] ...` 写到 stdout。`--timeout <毫秒>` 到时后停止脚本。
`monitor` 模块（接收区动作）在这里不可用，调用会报错。

```text
sasha-cli run my_script.lua --port COM3 --timeout 5000
```

## 退出码

| 退出码 | 含义 |
| --- | --- |
| `0` | 成功（`--help`、`--version` 也是 0）。 |
| `1` | 失败：脚本出错、写入失败、读不到脚本文件。 |
| `2` | 用法错误：选项无效或冲突、HEX 无效、正则无效。此时不会发送任何数据，也不会打开串口。 |
| `3` | 打不开串口（包括被占用），或运行中串口断开。 |
| `4` | 超时：`send --expect` 到时没匹配、`monitor --until` 在 `--duration` 内没匹配、`run --timeout` 到期。 |

## 更新

应用从发布页自动更新时，如果 `sasha-cli` 就放在应用程序旁边，也会一并更新。
