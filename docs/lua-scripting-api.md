# Sasha Lua 脚本 API 参考

本文档说明 Sasha 内置 Lua 5.4 脚本运行时的模块接口、参数与返回值约定、接收缓冲消费规则、错误语义、标准库开放范围、资源限制以及常用示例。

用户无需在系统上单独安装 Lua；Sasha 已内置 Lua 5.4 运行时。脚本按加载时的源码快照执行，运行期间修改磁盘文件不会影响正在执行的实例。

---

## 1. 模块与接口总览

### 1.1 `serial` 模块

| 函数签名 | 说明 | 返回值 |
| --- | --- | --- |
| `serial.send(data)` | 发送原始字节或文本（经会话统一发送队列串行写入） | 成功返回 `true`；失败抛出错误 |
| `serial.write(data)` | `serial.send(data)` 的别名 | 成功返回 `true`；失败抛出错误 |
| `serial.send_hex(hex_str)` | 解析十六进制字符串并发送原始字节（支持空格分隔或紧凑格式，大小写均可，如 `"AA BB 01"` 或 `"AABB01"`） | 成功返回 `true`；参数非法或写入失败抛出错误 |
| `serial.read([max_bytes [, timeout_ms]])` | 从本实例独立接收缓冲中读取原始字节，保留 `\0` 与非 UTF-8 二进制数据 | 有数据返回字节字符串 `data`；超时返回 `nil, "timeout"` |
| `serial.wait_text(pattern [, timeout_ms])` | 在本实例独立接收缓冲中等待匹配 `pattern`（支持跨串口读取块匹配） | 匹配成功返回从缓冲起始到匹配末尾（含 `pattern`）的字符串；超时返回 `nil, "timeout"` |
| `serial.clear_rx()` | 清空本实例独立接收缓冲及当前已到达但未消费的挂起数据 | 无返回值（`nil`） |

#### 参数与默认超时说明

- `serial.send(data)` / `serial.write(data)`：
  - `data` 必须为 Lua 字符串（可包含任意二进制字节，包括 `\0`）。传入 `nil`、数字或表将触发参数错误。
  - 发送不读取手动发送区的 HEX、换行符等 UI 设置，所发字节完全由脚本参数决定。
- `serial.send_hex(hex_str)`：
  - `hex_str` 必须为有效 UTF-8 字符串，按空白符分词；每个分片只能包含十六进制字符且长度必须为偶数，空字符串或奇数长度将触发参数错误。
- `serial.read(max_bytes?, timeout_ms?)`：
  - `max_bytes`（可选）：正整数或 `nil`。传入非正数、非整数或非数字将触发参数错误。省略或为 `nil` 时，返回当前缓冲中全部可用字节（若缓冲为空则等待至少 1 字节到达）。传入 `max_bytes` 时，最多返回并消费 `max_bytes` 字节，剩余字节保留在缓冲中。
  - `timeout_ms`（可选）：非负整数/数值，省略或为 `nil` 时默认为 `5000` 毫秒；传入 `0` 表示非阻塞检查，缓冲无数据立即返回 `nil, "timeout"`。
- `serial.wait_text(pattern, timeout_ms?)`：
  - `pattern`（必填）：非空字符串，按原始字节子串精确匹配。空字符串或非字符串将触发参数错误。
  - `timeout_ms`（可选）：非负整数/数值，省略或为 `nil` 时默认为 `5000` 毫秒；传入 `0` 表示仅检查当前已到达的数据。

#### 接收缓冲消费规则

1. **多实例隔离**：每个运行中的脚本实例在执行第一条 Lua 语句前即向会话注册独立的原始 RX 订阅者，并维护独立的字节累积缓冲。同一会话内的多个脚本会同时收到相同的 RX 字节，互不竞争、互不消费。
2. **与显示日志隔离**：界面日志的暂停、清空、HEX/文本切换不影响脚本接收；脚本也不受手动发送框状态影响。
3. **跨块匹配与尾部保留**：串口底层可能将一帧响应拆分为多个数据块上报。`serial.wait_text` 会在实例累积缓冲中持续拼接并匹配，即使 `pattern` 跨越多个读取块也能正确匹配。匹配成功后，**仅消费从缓冲开头到 `pattern` 末尾的字节**，紧随其后的尾部字节会继续保留在缓冲中供后续 `serial.read` 或 `serial.wait_text` 读取。
4. **超时不消费**：若 `serial.wait_text` 超时返回 `nil, "timeout"`，此前累积但尚未匹配的字节**不会被丢弃**，仍保留在缓冲中。

---

### 1.2 `task` 模块

| 函数签名 | 说明 | 返回值 |
| --- | --- | --- |
| `task.sleep(ms)` | 可取消延时，等待 `ms` 毫秒；收到停止或断线信号时立即唤醒并终止实例 | 无返回值 |
| `task.is_cancelled()` | 查询当前实例是否已收到取消/停止请求 | 布尔值 `boolean` |

- `task.sleep(ms)` 中的 `ms` 必须为非负数值（`>= 0`）；传入负数、`NaN` 或非数值会触发参数错误。

---

### 1.3 `log` 模块与全局 `print`

| 函数签名 | 说明 | 返回值 |
| --- | --- | --- |
| `log.info(...)` | 输出 `Info` 级别日志到本实例输出缓冲（多参数以 `\t` 拼接） | 无返回值 |
| `log.warn(...)` | 输出 `Warn` 级别日志到本实例输出缓冲 | 无返回值 |
| `log.error(...)` | 输出 `Error` 级别日志到本实例输出缓冲 | 无返回值 |
| `print(...)` | 全局 `print` 已被运行时接管，等价于 `log.info(...)` | 无返回值 |

每条日志均携带所属实例的 `run_id` 与级别标签。

> **这些是调试日志，不是脚本的"输出"。** 图形界面里，`log.*` 和 `print` 默认不显示；在「设置 → 通用 → 脚本调试日志」打开后，它们才会出现在接收区，带 `LOG`、`WARN`、`ERR` 标签（对应 `info`、`warn`、`error`），并且**不会**进入持续保存。`sasha-cli run` 需要加 `--script-debug` 才会打印它们，格式为 `[INFO] ...`、`[WARN] ...`、`[ERROR] ...`。想让用户看到脚本解析出的内容，请使用下面的 `output` 模块。

---

### 1.4 `checksum` 模块
| 函数签名 | 说明 | 返回值 |
| --- | --- | --- |
| `checksum.sum8(data)` / `checksum.xor8(data)` / `checksum.crc8(data)` | 8 位累加和 / 异或 / CRC-8（poly 0x07） | 整数 |
| `checksum.crc16_modbus(data)` / `checksum.crc16_ccitt(data)` | CRC-16/Modbus（init 0xFFFF，反射 0xA001）/ CRC-16/CCITT-FALSE（poly 0x1021） | 整数 |
| `checksum.crc32(data)` | CRC-32（zlib/Ethernet） | 整数 |
| `checksum.append(data, algo)` | 返回 `data` 加上校验字节的新字符串；`algo` 为上述函数名（如 `"crc16_modbus"`）。CRC-16/Modbus 低字节在前，其余多字节校验高字节在前 | 字符串；`algo` 未知抛出错误 |

`data` 必须是字符串（可含 `\0` 与二进制字节），否则抛出参数错误。

---

### 1.5 `monitor` 模块（接收区动作）

让脚本执行与接收区工具栏等价的操作。**仅图形界面可用**；`sasha-cli run` 中调用任何 `monitor.*` 都会抛出 `monitor.<name> is not supported in CLI mode`（可被 `pcall` 捕获）。

| 函数签名 | 说明 | 返回值 |
| --- | --- | --- |
| `monitor.pause()` | 暂停接收区输出，等价于工具栏暂停按钮。暂停期间新数据（RX 与 TX）不会丢失，恢复后显示；脚本自己的接收缓冲不受影响 | 无 |
| `monitor.resume()` | 恢复接收区输出 | 无 |
| `monitor.is_paused()` | 查询接收区当前是否暂停 | 布尔值 |
| `monitor.clear()` | 清空接收区日志与标记，等价于工具栏清空按钮 | 无 |
| `monitor.save(path)` | 把接收区**当前显示的文本**写入文件，内容与工具栏保存一致（含 RX 与 TX 行，受 HEX/文本、时间戳开关和过滤影响） | 成功返回 `true`；写入失败返回 `nil, "错误信息"` |
| `monitor.rotate(path)` | 保存并清空，在界面线程里一步完成：保存**全部行**（不受过滤影响，仍受 HEX/文本与时间戳开关影响），写入成功后才清空日志与标记；写入失败时不清空，返回 `nil, "错误信息"`。保存与清空之间不会丢失新到达的数据，适合定时切割日志 | 成功返回 `true`；失败返回 `nil, "错误信息"` |

`monitor.rotate` 的路径规则、输出目录、等待与取消行为与 `monitor.save` 相同。文件在界面线程内同步写入，写入期间界面会短暂停顿（本地磁盘通常是几十毫秒；输出目录在很慢的网络盘上可能更久）。

#### `monitor.save` 的路径规则

- `path` 必须是相对路径，写入「脚本输出目录」下。脚本输出目录默认是脚本文件所在目录，可在设置中修改。缺失的子目录会自动创建，同名文件会被覆盖。
- 空路径、绝对路径、盘符、UNC 路径、含 `:`、含 `..`、含 `\0` 或控制字符的路径，含 `<>"|?*` 的名称，以 `.` 或空格结尾的名称，以及 Windows 保留设备名（`CON`、`PRN`、`AUX`、`NUL`、`COM1`–`COM9`、`LPT1`–`LPT9`，不论大小写与扩展名）会触发参数错误（抛出错误，不会写文件）。
- 路径经符号链接或目录联接（junction）解析后若落在输出目录之外，保存会失败并返回 `nil, "错误信息"`，且不会在输出目录之外创建任何目录。
- 路径合法但系统无法写入（权限、磁盘已满、上级路径是文件等）时返回 `nil, "错误信息"`，不会终止脚本。
- `monitor.*` 请求队列有上限（256）。界面处理不过来时脚本会在调用处等待，仍可被停止。
- `monitor.save` 保存的是界面**已经显示**的内容。刚到达、界面尚未处理的数据，以及暂停期间暂存的数据不在其中；需要时先 `task.sleep(100)` 再保存。
- `monitor.save` 与 `monitor.clear` 是两个独立动作：从界面取快照到清空之间新到达的数据会随清空一起丢失；开启了过滤时，`save` 只保存过滤后可见的行，而 `clear` 会清掉全部行。需要不丢数据的定时切割时，请使用 `monitor.rotate`。
- 脚本在等待保存结果时被停止或断线，会立即以 `Stopped` 结束；此时已排队的保存请求仍可能被界面执行并写出文件。

---

### 1.6 `output` 模块（脚本输出）

| 函数签名 | 说明 | 返回值 |
| --- | --- | --- |
| `output.print(...)` | 输出脚本想让用户看到的内容；多参数以 `\t` 拼接（与 `print` 相同），含换行的内容按行拆开 | 无返回值 |

- **图形界面**：内容始终显示在接收区，带 `LUA` 标签，不受「脚本调试日志」开关影响；过滤栏的方向按钮循环到 **脚本** 时，会只显示脚本输出（和已开启的调试日志）。它会被 `monitor.save` / `monitor.rotate` 保存，也会进入持续保存；HEX 显示模式下它仍以文本显示。它不计入 RX/TX 字节数，也不会被当成设备数据发给脚本自己。
- **`sasha-cli run`**：原样写到 stdout（不加前缀），不需要 `--script-debug`。
- 事件队列有上限（10000 条），脚本输出过快而宿主来不及处理时，最旧的输出会被丢弃，生命周期事件不会丢。

---

### 1.7 `time` 模块（只读时钟）

脚本无法使用 `os`，`time` 提供只读的时间访问，使用本机本地时区、毫秒精度。图形界面与 `sasha-cli run` 中均可用。

| 函数签名 | 说明 | 返回值 |
| --- | --- | --- |
| `time.now()` | 当前本地时间的 Unix 毫秒时间戳 | 整数 |
| `time.monotonic()` | 自脚本启动起经过的毫秒数（单调，不受系统改时钟影响），用于计算间隔 | 整数 |
| `time.format(fmt [, ms])` | 按 strftime 格式（如 `"%Y-%m-%d_%H-%M-%S"`）格式化本地时间；`ms` 缺省为当前时间。`fmt` 非法或 `ms` 超出范围会触发参数错误 | 字符串 |
| `time.date([ms])` | 返回本地时间的各个字段；`ms` 缺省为当前时间 | 表 `{year, month, day, hour, min, sec, ms, wday}`，`wday` 1 表示周日（与 Lua `os.date` 一致） |

`ms` 必须是有限数值（毫秒时间戳），否则触发参数错误。

---

## 2. 错误语义与实例生命周期

脚本实例状态分为：`Running`（运行中）、`Stopping`（停止中）、`Completed`（已完成）、`Stopped { reason }`（已停止：`User` 或 `Disconnected`）、`Failed { error }`（失败）。

| 类别 | 触发场景 | 运行表现 | 实例最终状态 |
| --- | --- | --- | --- |
| **预期超时** | `serial.read` / `serial.wait_text` 在 `timeout_ms` 内未收到匹配数据 | 返回 `nil, "timeout"`，由脚本自行分支处理 | 继续运行 |
| **参数错误** | 传入非法类型、空 hex、奇数 hex、空 `pattern`、负数延时、非法的 `monitor.save` 路径等 | 抛出 Lua 运行时错误（可被 `pcall` 捕获） | 未捕获时变为 `Failed` |
| **取消 / 断线** | 用户点击停止、会话断开、关闭会话、或调用 `serial.*` 时连接代次过期 | 立即唤醒等待中的 `sleep`/`read`/`wait_text`，指令钩子中断纯 Lua 循环 | `Stopped`（`User` 或 `Disconnected`），**不记为 `Failed`** |
| **资源限制** | 单实例 RX 缓冲溢出、RX 订阅队列溢出、Lua 内存超限 | 抛出不可恢复错误并终止受影响实例 | `Failed` |
| **语法 / 运行错误** | Lua 语法错误、调用 `nil`、未捕获异常、串口写入队列满或底层写失败 | 终止当前实例，不影响同会话其他正在运行的脚本 | `Failed` |

> **错误隔离**：单个实例的语法错误、运行时错误或缓冲溢出只会终止该实例本身，不会影响同一会话中的其他脚本实例或主界面操作。

---

## 3. 标准库范围与安全边界

运行时基于 `mlua` 启用安全标准库子集，禁止文件系统、进程与动态加载能力：

- **已启用**：基础全局函数（`pairs`、`ipairs`、`next`、`tostring`、`tonumber`、`type`、`pcall`、`xpcall`、`error`、`assert`、`select`、元表相关函数等）、`coroutine`、`table`、`string`、`math`、`utf8`、`print`（已重定向至实例日志）。
- **未启用 / 已禁用**：`os`、`io`、`package`（含 `require`）、`debug`，以及全局 `dofile`、`loadfile`、`load`（均置为 `nil`）。

---

## 4. 资源限制

| 资源项 | 默认上限 | 超限行为 |
| --- | --- | --- |
| 单实例 Lua VM 内存 | `16 MiB` (`16 * 1024 * 1024` 字节) | Lua 内存分配失败，实例终止并进入 `Failed` |
| 单实例 RX 累积缓冲 | `1 MiB` (`1024 * 1024` 字节) | 明确报错溢出并终止受影响实例（`Failed`），不静默丢弃数据 |
| 单实例 RX 订阅队列 | `256` 块 / `1 MiB` | 同上，明确报错溢出并终止受影响实例 |
| 单实例日志保留行数 | `500` 行 | 超出时淘汰最早的日志行（环形保留最近 500 行） |
| 指令取消检查间隔 | 每 `1000` 条 Lua VM 指令 | 检查取消标志与连接状态，保证纯 Lua 死循环可及时停止 |
| 会话统一 TX 队列深度 | `256` 项 | 队列满时 `serial.send` / `send_hex` 报错 |

---

## 5. 使用示例

### 5.1 AT 指令交互与超时处理

```lua
serial.clear_rx()
serial.send("AT\r\n")

local resp, err = serial.wait_text("OK\r\n", 1000)
if resp then
    output.print("设备应答:", resp)
else
    log.warn("等待 OK 超时:", err)
end
```

### 5.2 发送 HEX 帧并读取二进制响应

```lua
serial.clear_rx()
serial.send_hex("AA 55 01 00 0D 0A")

local raw, err = serial.read(16, 2000)
if not raw then
    log.error("未收到二进制应答:", err)
    return
end

local hex_parts = {}
for i = 1, #raw do
    hex_parts[#hex_parts + 1] = string.format("%02X", string.byte(raw, i))
end
output.print("收到字节:", table.concat(hex_parts, " "))
```

### 5.3 周期轮询（响应停止请求）

```lua
local count = 0
while not task.is_cancelled() do
    count = count + 1
    serial.send("PING\n")
    local line = serial.wait_text("PONG\n", 500)
    if line then
        output.print("第", count, "次心跳成功")
    else
        log.warn("第", count, "次心跳超时")
    end
    task.sleep(1000)
end
```

### 5.4 收到特定数据后暂停接收区

```lua
local line = serial.wait_text("FATAL", 60000)
if line then
    monitor.pause()
    log.error("设备报告 FATAL，接收区已暂停:", line)
end
```

### 5.5 每小时切割日志（文件名带时间）

```lua
while not task.is_cancelled() do
    task.sleep(3600 * 1000)
    local ok, err = monitor.rotate(time.format("capture/%Y-%m-%d_%H-%M.txt"))
    if not ok then
        log.error("切割失败:", err) -- 失败时接收区保持原样，数据不丢
    end
end
```

### 5.6 对齐到整点切割

```lua
-- 到下一个整点还有多少毫秒（用本地时间字段计算，不依赖时区偏移是否为整小时）
local function ms_to_next_hour()
    local d = time.date()
    return (60 - d.min) * 60000 - d.sec * 1000 - d.ms
end

while not task.is_cancelled() do
    task.sleep(ms_to_next_hour())
    local ok, err = monitor.rotate(time.format("capture/%Y-%m-%d_%H00.txt"))
    if not ok then log.error("切割失败:", err) end
end
```
