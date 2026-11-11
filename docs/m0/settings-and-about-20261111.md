# 设置与关于功能验证

日期：2026-11-11；作者：lq。

## 实现范围

- 活动栏底部新增「设置」，打开原生设置页面；设置页收起连接侧栏，为内容留出宽度，仍保留工作区标签。返回服务器/数据库不销毁现有连接及草稿。
- 常用设置包括浅色/深色主题、终端字号（10–24 px）、SQL 自动换行。独立后台任务读取和原子保存 `preferences` 表，保存成功后生效，失败明确提示且不覆盖运行中的偏好。标题栏主题按钮也使用同一持久化路径。
- 版本号来自 Cargo 构建版本。用户主动检查 GitHub Releases，使用 TLS、5 秒连接超时、15 秒总超时、1 MiB 响应上限和语义版本比较；区分未找到公开发布、限流、网络失败、响应格式错误、无须更新及发现新版。仅打开固定仓库发布页下载，不自动安装。
- 关于页包含 QDS6 简介、“非人类程序员出品”、可复制 QQ：960661824、用户提供的公众号二维码和 `https://github.com/lq0910/qds6` 入口。二维码保留原始字节并离线嵌入，深色主题保持白底。

## 已完成验证

- `cargo build --offline -p qds6-app`：通过。日志 `output/settings-build.log`。
- `cargo clippy --offline -p qds6-app -p qds6-storage -p qds6-services --features ui-tests --lib -- -D warnings`：通过。日志 `output/settings-clippy.log`。
- `cargo test --offline -p qds6-storage -p qds6-services --lib`：9 项通过。覆盖设置默认值、重开数据库后的持久化、无效字号写入保护、语义版本比较及回环 HTTP 的 404/429 处理。日志 `output/settings-unit-tests.log`。
- 原生 Metal 布局与交互回归：1440×900、960×640 均通过。覆盖底部设置入口、主题与字号/换行保存、QQ 剪贴板、滚动访问二维码、返回工作区保留草稿、重建工作台后读取保存的偏好与主题，以及原有标签切换、关闭和多标签溢出。日志 `output/settings-layout-smoke.log`。
- 显式启用 `QDS6_LIVE_UPDATE_CHECK=1`，在原生 UI 中真实点击「检查更新」。请求完成，结果为“未找到公开的正式发布，暂时无法比较更新”。这不是成功发现新版或下载验收。
- 已检查 `output/ui-layout/settings-light-1440x900.png`、`settings-dark-1440x900.png`、`settings-about-960x640.png` 的布局和二维码显示。
- 使用隔离数据目录启动本轮 debug 桌面程序，原生窗口可见、截图成功、无意外退出；没有写入用户连接。日志 `output/settings-desktop-smoke.log`。这不等同于安装包验收。

## 边界

本次没有重新制作安装包、验证 Windows、执行新版下载安装或实机微信扫码验收。终端字号应用路径已接入现有/新建终端；本轮布局场景使用无网络占位工作区，不能替代真实 SSH 终端字体及 PTY 调整专项验收。
