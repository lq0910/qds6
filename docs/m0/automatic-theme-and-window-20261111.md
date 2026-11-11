# 自动主题与启动窗口验证

日期：2026-11-11；作者：lq。

## 实现

- 设置页界面主题提供「浅色 / 深色 / 自动」，自动跟随原生窗口的系统外观，保留模式本身的选中状态。选择经后台 SQLite 保存后生效，重启后仍可解析当前系统外观。已有浅色、深色偏好和缺字段默认值兼容。
- 标题栏快捷按钮按浅色 → 深色 → 自动 → 浅色循环，按钮文字表示下次切换目标，提示文字说明当前模式。终端配色继续独立保存。
- 系统外观订阅由设置状态独立持有，替换连接表单不会取消订阅。手动主题不受系统外观通知覆盖。
- 启动窗口由 1440×900 调整为居中的 1200×780，最小尺寸仍为 960×640。
- 实际启动发现连接与偏好并发初始化 SQLite 时，延迟事务可能升级互锁。初始化改为立即事务，使用已有 busy timeout 等待写锁，保证偏好加载能够完成。未调整连接、凭据或业务表内容。

## 验证

| 项目 | 结果与证据 |
| --- | --- |
| 普通构建 | `cargo build --offline -p qds6-app` 通过；`output/auto-theme-build.log` |
| 存储单测 | 9 项通过，包括三种主题保存重开、旧值兼容、4 线程并发初始化读取自动主题；`output/auto-theme-storage-tests.log` |
| 静态检查 | `cargo clippy --offline -p qds6-app -p qds6-storage --features ui-tests --lib --example appearance-smoke -- -D warnings` 通过；`output/auto-theme-clippy.log` |
| 原生布局和交互 | 1200×780、960×640 下实际点击自动选项和标题栏三态循环，核对外观、SQLite 保存与重建工作台后的读取；保留原有表单、备份、标签和终端主题回归。日志 `output/auto-theme-layout-smoke.log` |
| 原生外观通知 | `cargo run --offline -p qds6-app --features ui-tests --example appearance-smoke` 通过 5 个场景：自动深色/浅色、手动浅色遇深色、手动深色遇浅色、返回自动深色。每轮重建连接表单，核对订阅仍有效；`output/auto-theme-native-appearance.log` |
| 普通桌面启动 | 隔离 `QDS6_DATA_DIR` 启动 debug 构建，实际 QDS6 窗口宽度 1200，CoreGraphics 外框高度 781（含原生边框）；截图成功，随后退出自建进程。`output/auto-theme/desktop-check/verification.json` 和 `desktop-window.png` |

已目视检查 `output/ui-layout/settings-auto-1200x780.png`、`settings-auto-960x640.png` 和普通桌面启动截图。原生通知专项仅覆盖当前 Mac，通过改变测试进程自身的原生外观产生通知，没有修改系统全局外观。未重新打包、替换已安装 APP 或进行 Windows 验收。
