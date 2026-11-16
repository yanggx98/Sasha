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

### 快捷键

| 快捷键 | 操作 |
| --- | --- |
| `Ctrl+T` | 新建会话 |
| `Ctrl+W` | 关闭当前会话 |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | 下一个 / 上一个会话 |
| `Ctrl+1` … `Ctrl+8` | 切换到对应位置的会话 |
| `Ctrl+9` | 切换到最后一个会话 |
| `Ctrl+C` | 复制当前编辑器或日志区选中的文字 |
| 参数浮层中的 `Enter` / `Esc` | 确认 / 取消参数修改 |

## 偏好设置

从右侧栏打开**设置**。偏好保存在可执行文件同目录的 `config.toml` 中，设置页会显示实际配置路径。

```toml
version = 1
locale = "zh-CN"
log_font_size = 14
```

语言支持 `en-US` 和 `zh-CN`，默认为 `zh-CN`。日志字号预设为 12、13、14、15 逻辑像素。此文件不恢复会话草稿和连接状态。如果 TOML 格式损坏，Sasha 会尝试备份为 `config.toml.bak`，并回退到默认配置。

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
