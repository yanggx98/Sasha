# 主题

[English](themes.md) · 简体中文

Sasha 内置**深色**（默认）与**浅色**主题。在 **设置 > 视图 > 界面主题** 中选择，立即生效，
并以 `theme = "<id>"` 保存到 `config.toml`。

自定义主题是 `config.toml` 同级 `themes` 目录中的 `*.toml` 文件（首次启动时创建；
**设置 > 视图 > 主题目录 > 打开目录** 可直接打开）。文件名（去掉 `.toml`）即主题 id
（`ocean.toml` 的 id 为 `ocean`；`dark`、`light` 为内置保留 id）。增加、修改或删除文件后，
列表会自动更新；当前主题的文件被修改时会立即重新应用。

完整示例同样见 [`themes/example.toml`](themes/example.toml)：

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


## 在设置中编辑配色

**设置 > 视图 > 界面主题 > 编辑配色** 可编辑 8 个基础色（`canvas`、`card`、`text`、`accent`、
`rx`、`tx`、`error`、`border`）。每行是色块加 `#RRGGBB` / `#RRGGBBAA` 输入框，点击色块可打开色盘（调色板、滑块、透明度）并与输入框同步；有效的值会实时
预览，无效的会标红且不生效。

- 内置主题不能直接修改：输入名称后点 **另存为**，会创建 `themes/<名称>.toml`（文件名为小写
  后的名称；`dark`、`light`、`con` 等保留名和已被占用的名称会被拒绝）。
- 自定义主题用 **保存** 写回它自己的文件，也可用 **删除** 移除（需点两次确认）。删除当前主题
  会切回深色。
- 保存会重写文件：`[overrides]` 会保留，注释不会。
- 退出编辑、选择其他主题或关闭设置窗口时，未保存的改动会被丢弃。
