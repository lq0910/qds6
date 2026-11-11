# QDS6 0.1.0 Mac 首包实施与验收

日期：2026-11-11；作者：lq。交付范围为首个 Mac 内部可用版本，未把完整 M0–M4 或正式公开发行声明为已完成。

后续内部构建 `2026111104` 的当前改动及重新验证见 [更新汇总与打包验证](更新汇总与打包验证-20261111-04.md)。下文保留首包实施时的历史记录。

## 版本和环境

- 工作树基线 `1575f7a`，本轮实现未提交，不以基线提交冒充成果提交。准确源文件摘要、锁文件、编译参数见包旁 `build-manifest.json`。
- macOS 27.0.1（26A434）、Apple M4 Pro、24 GB、arm64，完整 Xcode，Rust/Cargo 1.99.1。
- Cargo.lock SHA-256：`1a8897959f07ad4aabc907e297e7810af1a1f388a3f49865016d14ace44af9e6`。
- GPUI Kit `ab7bd640021e062094cb4fd1e15fc6b465ec6a47` / 0.9.1；唯一默认后端 gpui-pre 0.3.12。无 Zed editor、WebView 或 Fast 后端。
- 包仅提供 Apple Silicon；Info.plist 安装下限设 macOS 14，实际验收仅上述环境。Windows/Intel/其他系统版本未验收。

## 已落地的基本工作链路

| 领域 | 当前行为 |
| --- | --- |
| 连接 | 新建、保存、编辑、删除前确认与依赖检查、分组、搜索、收藏、生产标记；SQLite 只存配置及凭据引用 |
| 凭据与 SSH | Keychain 密码/私钥口令；密码、私钥、Agent；SHA-256 首次确认及变化阻断、独立单跳隧道 |
| 终端 | Alacritty 状态机，GPUI 原生网格与 IME 提交接口；Vim/tmux、宽字符、应用光标模式、resize、标签/分屏、粘贴、5000 行回滚 |
| SFTP | 本地/远程双栏、有界虚拟目录、隐藏文件与名称排序、上传/下载进度和取消、创建/重命名/删除空目录/权限 |
| 编辑 | Kit Editor，UTF-8 有界文本、CRLF、常用语法、撤销和查找替换；RemoteDocument 基线、加密草稿及恢复 |
| 保存 | 内容/权限/属主比较、同目录排他临时文件、OpenSSH 原子替换能力协商；失败保留草稿、不主动截断原件 |
| 数据库 | SQLx 独立 Pg/MySQL，直连严格 TLS、SSH 隧道、同连接事务、服务端取消、网络停用；目录/表/视图、字段索引、基础表浏览 |
| 数据 | Kit 虚拟结果、多结果集、总 10000 行/16 MiB、显示文字排序、CSV、单列主键参数化更新/删除及确认预览、基础 DDL 生成 |
| 迁移 | 四类格式保守解析和警告、稳定来源 ID、事务合并/替换；无凭据 JSON、可选凭据加密备份、预览和本机快照回滚 |
| 发行 | 可重现 APP/ZIP/DMG 脚本、系统动态依赖检查、ad-hoc 签名、第三方清单/原文/必要源码、独立复制安装验收脚本 |

所有文件/SQLite/网络/加解密放后台。草稿使用代次核对，过期 worker 不复活已丢弃的旧草稿；撤销到基线也刷新加密缓存。Keychain 不可用不重新生成已有密钥。配置、SQL、远程草稿和活动会话的关闭有提示；等待后台写入完成再允许关闭。

## 验证证据

输出位于仓库 `output/`，均为专用合成数据及回环测试服务，无业务服务或真实用户数据。测试源码位于 examples / scripts，日志不作为发布包中的凭据来源。

| 层次 | 执行与结果 | 证据 |
| --- | --- | --- |
| 单测 | core 3、storage 2、services 3，共 8 通过 | `output/unit-tests.log` |
| 静态 | fmt、workspace/all-targets/all-features Clippy -D warnings 通过；release --locked 编译通过 | `output/clippy.log`、`output/release-build.log` |
| 原有 UI 回归 | 原生 Metal，中文文本、表单失败、SQLite 重读、收藏、SQL 草稿/主题 | `output/ui-smoke.log`、`output/m0-ui/` |
| 联合 UI | 真实 SSH 输入、中文终端渲染，SFTP 修改/CRLF/真实保存/草稿，Pg 严格 TLS 查询和虚拟网格，备份预览 | `output/service-ui-smoke.log`、`output/service-ui/*.png` |
| Kit 压力 | 5 MiB/40960 行真实缓冲+Metal、选区替换+完整撤销链、64 KiB 单行；组合步骤最近一次约 0.7–2.5 秒，不是纯打开耗时 | 联合 UI 日志；超限文本由 TextLimits 拒绝 |
| 真实数据库 | MySQL 8.4 / PostgreSQL 17；精确数值/大整数/NULL/空文本/二进制、回滚、错误、元数据、参数化修改、错误主键拒绝、CA/主机名校验、查询取消后复用 | `output/service-smoke.log`、`output/service-smoke-final.log` |
| 断连及规模 | 两种数据库十万行源均截到一万行；只终止专用目标会话，旧连接停用；普通 SQL 错误保留回滚能力 | 最终服务日志 |
| SSH | 独立临时 Agent、私钥/密码；真实 PTY resize、Vim 和 tmux；两种数据库隧道及第二通道取消 | `output/service-smoke.log`，Agent 不读取用户常用 Agent |
| SFTP | 上传/下载/拒绝覆盖、真实 OpenSSH posix-rename、内容冲突、权限、取消、终端关闭后 SFTP 仍可用；最新 UI 保存同时保留 uid/gid | 服务/联合 UI 日志 |
| 规模组合 | 10000 目录项，187 ms；1 GiB 上传/下载 SHA-256 相等；上传中终端模型 91 ms 响应；下载+双方哈希 109622 ms | `output/pressure-smoke.log`；不是 UI 帧时间/输入法验收 |
| 密码/备份 | 真实 Keychain 独立项存取删除、独立草稿密钥引用、加密恢复/篡改拒绝、可选凭据恢复；本机快照回滚由单测验证 | 服务日志和 backup 单测 |
| 拒绝访问 | 重编译工具访问此前草稿密钥被系统拒绝，返回明确“仅本会话”错误且不覆盖原密钥；后续真实 Keychain 验收使用独立引用 | 中间失败已修订测试隔离；最终服务日志通过不代表系统会自动授权所有构建 |
| 导入 | 四格式合成样例、稳定 ID、未知字段报告、DTD/外部实体拒绝、无法安全映射配置拒绝 | `migration.rs` 单测；无源工具真实版本导出验收 |
| 包验证 | APP 签名校验、DMG 完整性、挂载复制后的隔离数据实际原生启动及窗口探测、体积/当前进程采样 | 包目录 `installation-verification.json` 与 `installation-check/desktop.log`，当前机器结果另见下文 |

服务端固定容器 `qds6-fixture-pg`、`qds6-fixture-mysql`、`qds6-fixture-ssh`，仅绑定 127.0.0.1:25432/23306/22222。公开测试密码仅用于这些容器。验收结束删除上述三个专用容器，不操作其他容器。

## 发布包与重现

`output/release/0.1.0-macos-arm64-internal/` 内含 APP（staging）、ZIP、DMG、SHA256SUMS、build-manifest.json、dynamic-libraries.txt。使用 `scripts/package-macos.py`，发行参数 `cargo build -p qds6-app --release --locked`，不带测试 feature。`ssh2` 使用 vendored-openssl，`otool -L` 仅系统库；无需 Homebrew。

安装验证使用 `scripts/verify-macos-package.py`：挂载只读 DMG → 复制 APP 到独立目录 → 卸载 DMG → 移除 DYLD 环境变量及 Homebrew PATH → 指定新 QDS6_DATA_DIR → 实际启动 → 探测仅该 PID 原生窗口 → 读取新 SQLite → 终止自建进程。不会替换用户 Applications 或修改已有配置。该记录是当前机器的复制安装与启动，不能代替干净机器/Gatekeeper 网络隔离下载验收。

当前 Mac 验收通过：DMG 挂载和卸载正常，复制与 ZIP 解压得到的 APP 均通过严格签名校验；两份压缩包 SHA-256 与 `SHA256SUMS` 相符。复制版约 1.26 秒出现 QDS6 原生窗口，首次配置为空，截取的原生窗口图见 `installation-check/desktop-window.png`。空闲单次采样约 108 MiB RSS、0.0% CPU；APP 文件合计约 51.7 MiB，DMG 约 23.4 MiB，ZIP 约 18.4 MiB。该采样不代表长期或多场景性能。

签名为内部 ad-hoc；无 Developer ID 或公证，发布渠道、商标/名称可用性待确认。应用标识暂用 `local.qds6.desktop`，不虚构域名所有权。最低系统版本标签不等同兼容性实测。

## 依赖和许可

| 组件 | 精确版本 | 上游声明 |
| --- | --- | --- |
| GPUI Kit / gpui-pre | 0.9.1 固定提交 / 0.3.12 | Apache-2.0 |
| alacritty_terminal | 0.25.1 | Apache-2.0 |
| ssh2 / libssh2-sys | 0.9.7 / 0.3.4 | MIT OR Apache-2.0；原生 libssh2 原文同包保留 |
| sqlx | 0.8.6 | MIT OR Apache-2.0 |
| keyring | 3.6.3 | MIT OR Apache-2.0 |
| rusqlite / libsqlite3-sys | 0.32.1 / 0.30.1 | MIT；SQLite 上游材料同包保留 |
| argon2 / chacha20poly1305 | 0.5.3 / 0.10.1 | MIT OR Apache-2.0 / Apache-2.0 OR MIT |
| openssl-src / OpenSSL | 300.6.1+3.6.3 / 3.6.3 | wrapper MIT/Apache-2.0；OpenSSL Apache-2.0 |
| Lucide | Kit 固定资源 | ISC，保留原版权及 Feather 说明 |

`scripts/collect-licenses.py` 从 Mac 普通+构建依赖闭包整理 560 个第三方包，含构建工具、不声称全部被链接。保存原始许可、NOTICE、精确版本和来源；缺失原文从 Cargo 记录的源提交补齐。`option-ext` 和构建工具 `cbindgen` 含 MPL 声明，未经修改的 crate 源码随包归档。`mac 0.1.1`、`pathfinder_geometry 0.5.1`、`seahash 4.1.0`、`simd_helpers 0.1.0` 原文缺失/旧源不可读，标准许可补充明确标注，并附未修改源包，不冒称找回了上游原文。公开发行仍需最终许可证复核。

## 未完成项和可用性边界

下列项目仍保留任务未勾选；此版本用于内部实际连接和验证，不以缺失项冒充完整正式版：

- 真人中文 IME 候选/组合、多个显示器/缩放、系统快捷键/焦点全路径、无障碍；已验证中文提交与原生绘制不能替代这些验收。
- 真实来源版本导出矩阵（尤其 Navicat 17 加密 `.ncx`、FinalShell 目录），跨平台资源重映射、Windows 双向恢复。
- 传输队列/并发、重试、续传、目录递归；当前单传输有界并可取消。符号链接不跟随编辑；ACL/xattr/硬链接语义未承诺；原子替换仍存在最终远端并发窗口。
- 严格数据库 TLS+SSH 隧道组合明确拒绝，不自动降级。可直连 TLS，或用户显式关闭隧道内数据库 TLS。
- 网格为表单式单行主键更新/删除，新增行/复合键/直接编辑单元格、结果过滤/分页、完整表设计器和查询历史未实现。保守事务关键字识别；动态过程/SET autocommit 后网格需重连。
- 本机偏好/完整布局、跨会话 SQL 草稿、自动备份调度、常驻长期稳定性、所有异常权限/磁盘满/中断恢复组合未验收。
- 干净机器安装升级卸载、公证、Gatekeeper、公开发行许可/品牌/渠道与 Windows；Mac 优先约定不因 Windows 待办阻塞内部包。

完整操作、限制和清理策略见 [使用说明](使用说明.md)。
