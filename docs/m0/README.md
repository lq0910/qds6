# M0 首轮实施与验证

> 时间：2026-11-10｜作者：lq｜状态：部分完成，不能进入发布验收。

本轮建立实际 Rust 原生工程，交付可构建的本地连接管理与 Kit SQL 草稿工作区。没有实现 SSH、SFTP、数据库查询、凭据库、导入/导出或备份。本地配置保存不会发起网络连接。

## 版本与环境

- 基线提交 `1575f7a`，本轮新增代码位于未提交工作树；不能把基线提交当作本轮成果提交。
- macOS 27.0.1（26A434），Apple M4 Pro，24 GB，aarch64，完整 Xcode。
- Rust / Cargo 1.99.1；`rust-toolchain.toml` 固定工具链。1.95 编译 GPUI macOS 时在 `Option::map_or_default` 失败，升级到官方稳定版后通过。
- GPUI Kit 0.9.1，官方提交 `ab7bd640021e062094cb4fd1e15fc6b465ec6a47`；GPUI 默认后端 `gpui-pre` 0.3.12。
- `Cargo.lock` SHA-256：`57511051b818c17fd7e788863481518e9beba37c17dd3ba61f4f7d23dc372e82`。
- 宏、Kit、Base、Component、Assets 统一从 Kit 固定提交获取。实际 macOS 依赖树只有一套 `gpui-pre`，未启用 Fast。锁文件可能记录上游可选 Fast 包，不代表它进入构建。
- 开启测试支持时，Cargo 需要整体重新求解上游可选依赖约束；当前锁文件选定 `taffy 0.15.0`、`unicode-properties 0.1.3`。后续使用 `--locked`。

## 已交付

| 层 | 实现 |
| --- | --- |
| core | SSH/SFTP/MySQL/PostgreSQL 协议、默认端口、稳定 UUID、凭据引用；普通配置拒绝未知密码字段 |
| storage | SQLite schema v1、未来 schema 拒绝、参数化事务写入、读取校验、稳定 ID 更新 |
| app | 连接新建与编辑、分组显示、名称/主机/用户/协议搜索、收藏筛选、生产标记、持久化和重读 |
| UI | Kit 原生标题栏、按钮、输入、标签；浅深主题；1440×900 默认、960×640 最小窗口 |
| editor | Kit Editor SQL 高亮、行号、内置查找入口、编辑与复制、标签切换保留会话草稿 |
| lifecycle | I/O 放后台；保存期间冻结表单；切换前丢弃确认；关闭窗口/应用菜单/快捷键保护修改 |
| CI | macOS / Windows 构建、领域存储回归、格式与 Clippy 工作流，尚未在远端执行 |

领域模型只含 `CredentialRef`，不含密码；凭据库尚未实现。SQLite 不存 SQL 草稿。Unix 应用目录设置 0700；Windows 私有目录 ACL 尚待验证。

## 验证结果

| 检查 | 结果与口径 |
| --- | --- |
| `cargo build -p qds6-app --locked` | macOS debug 成功，二进制 `target/debug/qds6`；约 115 MiB 是调试二进制，不能作为下载包或 release 安装体积 |
| `cargo test -p qds6-core -p qds6-storage --locked` | 5 项通过：地址与 IPv6、未知密码字段、文本边界、SQLite 往返/无效写入保护、未来 schema 保留 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 本地通过；开启全部应用功能仅添加 UI 测试支持，未开启 GPUI Fast |
| 隔离 UI 流程 | Kit 公共测试 API 触发中文输入、空表单拒绝、保存/重读、收藏、SQL 编辑与复制、切换页面保留草稿、主题切换，全部通过 |
| GPU 绘制 | 真实 macOS 字体与 Metal 离屏绘制，1440×900 逻辑尺寸、2880×1800 图片；浅色表单、SQL、深色表单已目视检查 |
| 原生桌面启动 | 普通二进制进程启动、QDS6 标题窗口存在、隔离 SQLite 成功创建、无启动错误日志；窗口列表只返回系统缩略图，未确认前台完整窗口尺寸或交互，不据此完成 T003 |
| Windows | 已配置工作流，没有本轮编译、窗口或安装证据 |

UI 验证代码：`crates/qds6-app/src/smoke.rs`，使用临时 SQLite，完成后删除。系统剪贴板在受限沙箱内不可用，UI 验证是在可访问原生桌面服务的环境完成。事件订阅在 UI 更新结束后派发，断言在队列处理后进行；截图推进模拟时钟以等待标签动画。

截图与日志位于被 Git 忽略的 `output/m0-ui/`。保留的截图可直接打开：

- [连接配置 / 浅色](../../output/m0-ui/connection-light.png)
- [SQL 草稿](../../output/m0-ui/sql-light.png)
- [连接配置 / 深色](../../output/m0-ui/connection-dark.png)

`native-window.png` 仅为系统缩略图，不作为完整桌面验收截图。普通桌面启动使用 `QDS6_DATA_DIR=.../output/native-data` 隔离配置；正常启动不设置该变量。

## 任务对应与下一步

计划调整（2026-11-10，lq）：当前优先推进 Mac M0–M3，Mac 主要功能基本落地后启动 W0 Windows 适配。首轮记录中的 Windows 工作流配置不代表已执行；当前 CI 仅运行 Mac，Windows 矩阵延期启用。以下 Windows 待办保留，但不阻塞 Mac 阶段退出判断。

- T001、T002：完成，证据为 workspace、锁文件、成功构建与实际依赖树。
- T003：仅 macOS 进程启动部分通过；仍需正常前台窗口操作、两端最小安装包和 Windows 实际运行。
- T004：Mac 树、十万行二十列虚拟表、弹出菜单、标签切换/重排/跨组拖动、分隔线与面板放大恢复已验证，见 [组件验证记录](kit-components.md)；Windows 延至 W001。该结果属于组件能力验证，未将合成数据接入正式客户端业务。
- T005：中文文本注入、SQL 高亮与编辑/复制已验证；不等同于真实 IME 组合输入。撤销、替换、5 MiB 性能、超长行和 CRLF 编辑保存尚未完成。
- T011：已记录主要版本和机器可读依赖许可声明；发行资源、字体、完整 NOTICE 和 Windows 依赖审计仍待执行。
- T006–T010、T012–T014：尚未验收；按原清单继续，不把 UI 空壳算作网络服务能力。

`TextLimits` 的 5 MiB / 100000 行 / 单行 64 KiB 校验有单测，目前用于复制边界，尚无远程文件打开流程；不能据此声称编辑器对任意持续输入始终有内存硬上限。SQL 草稿当前仅会话保存；不能当作已实现加密草稿恢复。系统强制退出/掉电与应用菜单主动关闭分开处理，尚无崩溃恢复。

## 依赖许可

[dependencies.json](dependencies.json) 从本轮 `cargo metadata --filter-platform aarch64-apple-darwin` 导出，列出 474 项当前目标解析依赖的版本、来源和声明许可证。全部有声明，但 metadata 声明不等同于完整发行许可审查。

直接依赖：GPUI Kit / GPUI 为 Apache-2.0；rusqlite 为 MIT；serde、serde_json、uuid、directories、thiserror 为 MIT OR Apache-2.0。发行前仍需收集源许可证、资源许可、系统链接库与必要 NOTICE；T011 保持未勾选。
