<p align="center">
  <img src="assets/icons/sasha-arrows-transparent.png" alt="Sasha" width="96" />
</p>

<h1 align="center">Sasha</h1>
<p align="center"><a href="README.md">English</a> · 简体中文</p>

Sasha 使用 Rust 和 GPUI 构建，将串口连接、文本与 HEX 查看、多会话管理整合在紧凑的深色界面中。

## 界面预览

<table>
  <tr>
    <th width="50%">连接设备</th>
    <th width="50%">查看与发送数据</th>
  </tr>
  <tr>
    <td><a href="docs/images/sasha-connection.png"><img src="docs/images/sasha-connection.png" alt="串口选择与通信参数配置" width="100%" /></a></td>
    <td><a href="docs/images/sasha-terminal.png"><img src="docs/images/sasha-terminal.png" alt="显示 Sasha v0.1.0 的模拟终端会话" width="100%" /></a></td>
  </tr>
</table>

点击截图可查看原图。终端截图展示的是模拟会话。

项目正在持续开发。目前以 Windows 为开发和验证平台，尚未验证 macOS 与 Linux 构建。

## 功能

- **多会话管理**：通过右侧栏新建、切换和关闭连接。
- **串口配置**：枚举可用串口或手动输入端口，配置波特率、数据位、校验位和停止位。
- **接收查看**：切换文本与 HEX、显示时间戳、暂停显示、控制自动滚动、选择复制文字及导出可见日志。
- **灵活发送**：发送 UTF-8 文本或 HEX 字节；文本可追加 CRLF 或 LF。输入框 HEX 显示与 HEX 发送独立控制。
- **连接反馈**：显示连接状态及 RX/TX 字节计数。
- **流输出**：指定文件，实时追加当前会话后续全部 RX/TX 日志，不受显示暂停、清空或内存容量限制影响。
- **原生编辑**：发送框使用闪烁块状光标，保留正常的文本选择和编辑行为。
- **偏好设置**：支持英文与简体中文，提供日志字号调整，全局偏好保存在本地 TOML 文件中。

## 构建与运行

需要 Git、支持 Rust 2024 edition 的 Rust 与 Cargo，以及对应目标平台的本机构建工具。Windows 使用 MSVC 工具链，并安装 Visual Studio C++ Build Tools 和 Windows SDK。

```powershell
git clone https://github.com/yanggx98/Sasha.git
cd Sasha
cargo run --locked
```

构建优化后的可执行文件：

```powershell
cargo build --release --locked
```

Windows 可执行文件位于 `target/release/sasha.exe`。首次构建需要下载依赖，缓存完整后可离线构建。复制源代码时请保留 `vendor/` 目录。

## 开始使用

1. 选择检测到的串口，或手动输入端口名称。
2. 设置与设备一致的通信参数，然后连接。
3. 在日志区查看接收数据，在发送框输入待发送内容。
4. 点击箭头按钮发送，通过右侧栏创建更多会话。

没有设备时，可在连接页选择**进入模拟会话**。模拟数据及连接状态与真实会话分别标注。

### 文本与 HEX 控制

三个 HEX 开关相互独立：

| 控件 | 作用 |
| --- | --- |
| 接收 HEX 显示 | 改变日志中数据内容的显示方式。 |
| 发送框右上角 `H` | 在文本与 HEX 之间显示、转换当前草稿。 |
| 接收工具栏中的 HEX 发送模式 | 决定将草稿编码为 UTF-8 文本，还是解析为 HEX 字节发送。 |

HEX 使用空格分隔的两位字节，例如 `0A FF 1B`。文本模式可选择不追加换行、追加 CRLF（`\r\n`）或 LF（`\n`）；HEX 模式直接发送解析后的字节，不追加文本换行。HEX 转回文本时，字节必须是有效的 UTF-8。

### 实时流输出

点击接收工具栏的 `⇥`，在文件选择器中指定输出路径；已有文件会追加写入，原内容不会被覆盖。按钮高亮时正在记录，悬停可查看路径，再次点击停止。每个会话独立记录；同一文件同时只能由一个会话使用，不记录 Lua 的 `log` 调试输出。

在**设置 → 常规 → 流输出范围**选择“当前缓存及后续”或“仅开始后”（默认）。前者在文件准备就绪时保存所属会话当时仍保留的可见及暂停缓存，再连续记录后续日志；不会恢复已清空或裁剪的历史，也不会包含其他会话。后者仅记录文件准备就绪并开始捕获后的日志。选项全局持久化，修改仅影响下一次开始，不会向正在记录的文件重复回填。

输出固定使用启用时的接收文本/HEX模式。文本模式按设备的 LF/CRLF 换行组织日志，每行只在开头添加首次字节的时间戳、RX/TX方向和模拟数据标记；连续 RX 分块接在同一行，尚未换行的内容也会实时写入。方向或模拟标记变化时会分隔未完行，TX事件分别记录，停止时为未完行补换行。文本按 UTF-8 增量解码，正常中文、CR/LF、制表符和反斜线保留；NUL及其他控制字节显示为 `\0`、`\xHH`，无效 UTF-8 字节也会转义。跨块字符最多暂存末尾3字节等待补全，停止或切换方向时未完整字符转为字节转义。二进制数据建议启用前切换 HEX，HEX模式仍按每次收发事件记录完整字节。

新建或空文件写入一次 UTF-8 BOM，帮助 Windows 编辑器识别编码；追加非空文件不会插入中途 BOM，也不会修改旧内容。带 UTF-16/UTF-32 BOM 的文件会明确拒绝追加，请选择新文件。之前无 BOM 且出现乱码的日志不会自动修复，请更换新文件后记录。

文件在后台持续写入，每条记录立即刷新应用缓冲；暂停显示、清空日志、缓存裁剪、切换标签和断线都不会停止记录。停止时在后台排空已排队记录；关闭会话会等待排空，正常退出应用会在框架退出等待期内排空。写入错误或后台队列满会停止捕获并在工具栏上方提示；修复路径或存储问题后可重新启用。

### 快捷键

| 快捷键 | 操作 |
| --- | --- |
| `Ctrl+T` | 新建会话 |
| `Ctrl+W` | 关闭当前会话 |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | 下一个 / 上一个会话 |
| `Ctrl+1` … `Ctrl+8` | 切换到对应位置的会话 |
| `Ctrl+9` | 切换到最后一个会话 |
| `Alt+1` … `Alt+9` | 发送快捷发送列表中对应位置的命令 |
| `Ctrl+C` | 复制当前编辑器或日志区选中的文字 |
| 参数浮层中的 `Enter` / `Esc` | 确认 / 取消参数修改 |

## 快捷发送

**快捷发送**区块位于右侧栏**脚本**区块下方。每条命令保存 `id`、`name`、可选 `group`、`content`、`hex` 与 `line_ending`（`"none"` / `"crlf"` / `"lf"`，开启 `hex = true` 时忽略）。点击命令会按其自带的 HEX 与换行符设置直接发送，不会改动发送卡输入框和底栏状态；在工作区按 `Alt+1` … `Alt+9` 可发送展示顺序中的前 9 条命令（跨分组计数，不受搜索过滤或折叠影响）。命令在所有会话间共享，并持久化在 `config.toml` 中：

```toml
[[quick_commands]]
id = "qc-1"
name = "AT"
group = "设备初始化"
content = "AT"
hex = false
line_ending = "crlf"

[[quick_commands]]
id = "qc-2"
name = "Ping"
group = "设备初始化"
content = "AA 55 01"
hex = true
```

`config.toml` 中单条格式损坏或 `id` 重复的命令会在加载时被忽略，不影响其余配置，并在快捷发送区块内提示。

## 定时发送

发送卡底部行中，发送按钮左侧常驻一个胶囊 `[⏱ | 1000 ms]`，用于按固定间隔重复发送输入框内容。点击左侧时钟图标只切换“启用”（进入或退出定时发送模式），不会直接发送；右侧为间隔输入（10 ~ 3600000 毫秒的整数），会记住上次使用的值（写入 `config.toml` 的 `timer_interval_ms`）。未连接时胶囊置灰，无法启用。

三种状态：

- **未启用**：胶囊为灰色描边、透明底，发送按钮是普通发送 `↑`。
- **已启用（待机）**：胶囊为强调色实心底，发送按钮变为播放 `▶`；点击播放（或在输入框按 Ctrl+Enter）开始定时发送，先校验间隔草稿与内容，无效则报错且不启动。
- **运行中**：图标带呼吸点，发送按钮变为暂停 `⏸`；点击暂停停止发送但保持启用，按钮回到 `▶`。运行中在输入框按 Ctrl+Enter 会被忽略并提示，不会当作暂停。再次点击时钟图标会取消启用（运行中则同时停止）。

开始后立即发送一次，之后按固定节拍发送；程序卡顿时错过的周期会被跳过，不会补发。若端口写入速度跟不上间隔（如低波特率），待写入的定时发送超过上限时同样跳过周期，避免堆满发送队列。

- 已启用期间（含待机）不能手动发送：输入框的普通发送、`Alt+1` … `Alt+9` 与快捷发送面板都不会发送并给出提示；脚本发送不受影响。
- 输入框内容、HEX 开关、底栏 HEX 发送与换行符可在运行中修改，从下一个周期起生效。间隔输入框中的内容是草稿：按回车或输入框失焦时才提交并从下一个周期起生效（未提交时输入段有淡淡的高亮），回车只提交不会启动；无效草稿会恢复为上一个有效间隔并提示。
- 每个周期发送前都会重新校验：内容为空或 HEX 格式非法时自动停止并提示，胶囊保持启用，可修正后再点播放。
- 点击暂停、写入失败，或发送队列被脚本、快捷发送占满时停止并提示（保持启用）；断线、关闭会话或取消启用会同时取消启用并停止，重新连接不会自动恢复。启用状态按会话独立，不会持久化。

## 偏好设置

从右侧栏打开**设置**。偏好保存在可执行文件同目录的 `config.toml` 中，设置页会显示实际配置路径。

```toml
version = 1
locale = "zh-CN"
log_font_size = 14
stream_output_scope = "future_only" # 或 "current_buffer_and_future"
theme = "dark"                # 内置 id 或 themes/ 中的文件名
```

语言支持 `en-US` 和 `zh-CN`，默认为 `zh-CN`。日志字号预设为 12、13、14、15 逻辑像素。此文件不恢复会话草稿和连接状态。如果 TOML 格式损坏，Sasha 会尝试备份为 `config.toml.bak`，并回退到默认配置。

## 主题

Sasha 内置**深色**（默认）与**浅色**主题。在 **设置 > 视图 > 界面主题** 中选择，立即生效，
并以 `theme = "<id>"` 保存到 `config.toml`。

自定义主题是 `config.toml` 同级 `themes` 目录中的 `*.toml` 文件（首次启动时创建；
**设置 > 视图 > 主题目录 > 打开目录** 可直接打开）。文件名（去掉 `.toml`）即主题 id
（`ocean.toml` 的 id 为 `ocean`；`dark`、`light` 为内置保留 id）。增加、修改或删除文件后，
列表会自动更新；当前主题的文件被修改时会立即重新应用。

完整示例同样见 [`docs/themes/example.toml`](docs/themes/example.toml)：

```toml
name = "Ocean"          # 显示名，缺省为 id
base = "dark"           # "dark"（默认）或 "light"

# 基础色：#RRGGBB 或 #RRGGBBAA
canvas = "#0B1620"      # 窗口 / 日志背景
card = "#12222F"        # 发送卡片等抬升表面
text = "#E3EEF5"        # 主文字
accent = "#14B8A6"      # 发送按钮与高亮
rx = "#4ADE80"          # 接收数据
tx = "#7DD3FC"          # 发送数据（不随强调色联动）
error = "#FB7185"       # 校验错误
border = "#FFFFFF1C"    # 卡片边框

# 可选：按名称覆盖任意派生色
[overrides]
bg_strip = "#08111A"
text_muted = "#6B8394"
```

- **缺失字段**由 `base` 对应的内置主题补齐；未知字段会被忽略。
- **派生色**由基础色计算：悬停与开关背景来自 `text` 和 `accent`，接收条与弹出层来自
  `canvas` 和 `card`，强调色悬停来自 `accent`，次要/弱化文字由 `text` 向 `canvas` 混合得到，
  错误与徽标底色来自 `error` 和 `accent`，已连接状态色来自 `rx`；深色与浅色使用不同的混合强度。
- **`[overrides]`** 可用名称：`bg_canvas`、`bg_card`、`bg_strip`、`bg_popover`、`bg_hover`、
  `bg_toggle_active`、`border_card`、`border_popover`、`accent_blue`、`accent_blue_hover`、
  `rx_green`、`tx_blue`、`text_primary`、`text_secondary`、`text_muted`、`error_text`、
  `error_bg`、`sim_badge_bg`、`sim_badge_text`、`status_disconnected`、`status_connected`，
  优先于派生规则。
- **出错处理：** TOML 无效或颜色非法的文件会被跳过，并在“主题警告”区域给出文件名和字段名，
  其他主题照常加载。若配置的主题缺失或损坏，Sasha 改用内置深色并提示，但保留配置中的 id，
  文件修复后会自动恢复。

## 当前边界

- 可见日志最多保留最近 **2,000 条记录**。导出保存当前可见历史及其显示格式，并非完整的原始数据抓取。
- 暂停只冻结日志显示，不会停止串口连接；恢复显示时追加缓冲记录。
- 真实串口连接支持 1 或 2 个停止位，当前后端会拒绝 1.5 个停止位。
- 模拟会话用于界面预览，不等同于硬件回环测试。

## 开发

```powershell
cargo check --locked
cargo test --locked
```

测试覆盖数据格式、发送校验、会话行为、配置及原生光标几何。硬件通信和桌面交互还需要在目标系统中手动验证。

```text
src/
  main.rs          应用启动
  i18n.rs          英文与简体中文文案
  serial.rs        串口发现与读写
  model/           会话、日志、串口参数及偏好设置
  ui/              工作区、连接页、发送区及设置页
assets/icons/      应用图形资源
vendor/            本地 GPUI 依赖及补丁说明
```

项目通过本地补丁扩展 `gpui-base` 0.9.1，支持按输入框配置原生光标样式，以及发送框聚焦时隐藏提示文字，保留组件原有的光标生命周期。来源、许可与升级方式见[补丁说明](vendor/README.md)。

## 致谢

Sasha 基于以下开源项目构建：

- **[GPUI-KIT](https://github.com/longbridge/gpui-kit)**：Sasha 使用的 UI 工具包与组件库。
- **[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui)**：Zed 的 GPU 加速 UI 框架，源码位于 [Zed 仓库](https://github.com/zed-industries/zed)。

项目开发过程中使用的 AI 工具有：**DeepSeek、Grok、ChatGPT、Gemini**。

## 问题反馈

欢迎通过 [GitHub Issues](https://github.com/yanggx98/Sasha/issues) 报告问题或提出建议。请提供 Windows 版本、应用代码版本、复现步骤及相关串口参数。界面问题建议附上截图和系统显示缩放比例。

## 许可证

Sasha 采用 **GNU 通用公共许可证第 3 版（仅此版本，GPL-3.0-only）**。完整条款见 [LICENSE](LICENSE)。

随项目保存的 GPUI 代码保留其 [Apache-2.0 许可证](vendor/gpui-base/LICENSE-APACHE)，其他依赖遵循各自许可证。
