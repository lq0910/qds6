# 终端内置主题与验证

日期：2026-11-11；作者：lq。

## 内置配色

| 名称 | 类型 | 默认背景 | 默认文字 | 设计方向 |
| --- | --- | --- | --- | --- |
| 晴空 | 浅色 | `#F4F7FB` | `#26364A` | 冷白底、蓝色目录、沉稳绿提示符 |
| 暖纸 | 浅色 | `#FAF5E9` | `#433B31` | 米色底、暖棕文字、橄榄绿与青蓝点缀 |
| 深海 | 深色（默认） | `#101722` | `#DCE3EC` | 深蓝灰底，延续原终端 ANSI 配色 |
| 墨林 | 深色 | `#142320` | `#DCE9DF` | 深绿灰底、柔和薄荷绿与雾蓝点缀 |

设置页提供四个可点击原生预览按钮。选择独立于界面主题，并后台保存到软件偏好；旧设置没有终端主题字段时继续使用默认深海。已有终端通过网格颜色重新映射立即更新，不清屏、不重连、不重置本次临时字号。新连接从已保存偏好继承配色。

颜色语义位于 `qds6-core::TerminalTheme`。终端状态机映射默认前景/背景与 ANSI16，保留显式 RGB 真彩色与扩展256色；反显、隐藏文字继续在映射后处理。原生画布同步使用主题的光标、搜索高亮及输入法组合文本色。

## 验证记录

- `cargo build --offline -p qds6-app`：编译通过，日志 `output/terminal-themes-build.log`。
- `cargo clippy --offline -p qds6-app -p qds6-core -p qds6-storage -p qds6-services --features ui-tests --lib -- -D warnings`：通过，日志 `output/terminal-themes-clippy.log`。
- `cargo test --offline -p qds6-core -p qds6-storage -p qds6-services --lib`：本轮 17 项通过。日志 `output/terminal-themes-unit-tests.log`。包含主题数量及文字/光标/搜索对比检查、已有网格重新配色、RGB 真彩色保留、反显/隐藏语义、旧偏好缺字段兼容和序列化往返。
- 原生 Metal 布局与交互：1440×900、960×640 的设置页及原有标签回归通过；在真实 VT 网格上依次点击四套主题，核对已保存选择、已有输出不变及临时 19px 字号不被重置；重建工作台后读取保存的主题。日志 `output/terminal-themes-layout-smoke.log`。
- 截图：`output/ui-layout/terminal-theme-clear-sky.png`、`terminal-theme-warm-paper.png`、`terminal-theme-deep-sea.png`、`terminal-theme-ink-forest.png`。已目视检查。`terminal-themes-overview.png` 是原生截图裁切拼接的效果对照，不是替代终端渲染的模拟图。

终端截图使用仅在 `test-support` 下启用的离线会话和真实 Alacritty VT 网格，不连接 SSH 服务；此记录不等同于真实服务、Windows、安装包或真人输入法专项验收。
