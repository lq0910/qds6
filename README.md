# QDS6

Connections, simplified.

连接，从此简单。

终端、文件与数据库，一个工作台。使用 Rust + GPUI Kit 开发，先完成 macOS 主要功能，再启动 Windows 适配；最终平台范围为 macOS 和 Windows。

当前处于 **M0 技术验证阶段**。已建立原生窗口、连接配置管理、SQLite 存储、浅深主题与 Kit SQL 草稿编辑器。SSH、SFTP、数据库执行、凭据库、导入与备份仍待实现；保存连接不会联网。

## 开发

安装 Rustup 与平台原生构建工具。macOS 需要完整 Xcode（Metal 编译）；Windows 需要 Visual Studio C++ Build Tools 与 Windows SDK。`rust-toolchain.toml` 固定 Rust 1.99.1；首次构建会下载依赖。

```sh
# 功能：运行原生桌面客户端；时间：2026-11-10；作者：lq。
cargo run -p qds6-app --locked
```

```sh
# 功能：格式、领域存储测试与静态检查；时间：2026-11-10；作者：lq。
cargo fmt --all -- --check
cargo test -p qds6-core -p qds6-storage --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

```sh
# 功能：macOS 使用真实字体与 Metal 验证 UI，生成 output/m0-ui 截图；时间：2026-11-10；作者：lq。
cargo run -p qds6-app --example ui-smoke --features ui-tests --locked
```

```sh
# 功能：打开 Mac M0 树、虚拟表和停靠组件验证窗口，仅使用合成数据；时间：2026-11-10；作者：lq。
cargo run -p qds6-app --example component-lab --features component-lab --locked

# 功能：自动验证上述组件交互，截图及结果写入 output/m0-components；时间：2026-11-10；作者：lq。
cargo run -p qds6-app --example component-smoke --features ui-tests --locked
```

Mac T004 组件验证已通过，详见 [组件验证记录](docs/m0/kit-components.md)。实验窗口独立于正式客户端，验证数据表不代表数据库查询功能已经实现。

UI 验证使用临时数据库，不读取用户连接。实际客户端将非秘密配置存于系统本地应用数据目录下的 `QDS6/connections.sqlite3`；macOS 为 `~/Library/Application Support/QDS6/`，Windows 为 `%LOCALAPPDATA%\QDS6\`。SQL 草稿仅保留本次会话，可复制到外部保存；关闭有修改的窗口会询问是否丢弃。

## 工程结构

- `crates/qds6-core`：协议、连接模型、凭据引用、文本输入边界。
- `crates/qds6-storage`：SQLite schema 与事务存储，不接收密码。
- `crates/qds6-app`：GPUI Kit 原生 UI；文件与数据库 I/O 在后台执行。
- `.github/workflows/ci.yml`：当前运行 macOS 编译、格式与回归检查；Windows 在 Mac 主要功能落地后启用。
- [产品与技术方案](docs/产品与技术实施方案-20261110.md)
- [开发任务](docs/开发任务与验收清单-20261110.md)
- [导入导出与备份规范](docs/连接导入导出与备份规范-20261110.md)
- [M0 首轮实施记录](docs/m0/README.md)
- [图标与品牌规范](docs/图标与品牌规范-20261110.md)

GPUI Kit 固定官方提交，使用其默认 `gpui-pre` 后端，不依赖 Zed editor、WebView 或 GPUI Fast。精确依赖由 `Cargo.lock` 管理。构建通过和 CI 配置不等于 Windows 运行、服务联调或安装验收。
