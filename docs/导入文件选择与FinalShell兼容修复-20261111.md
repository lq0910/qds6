# 导入文件选择与 FinalShell 兼容修复

日期：2026-11-11；作者：lq。

## 修复内容

- 「从其他工具导入」和「恢复与回滚」的路径旁增加「选择文件…」，调用 GPUI 原生单文件选择接口，禁止选择目录；仍支持粘贴路径。选择新文件后旧预览失效，取消选择保留路径和预览，后台任务忙碌时禁用选择入口。
- FinalShell 文件原先只要含 `proxy_id` 就被跳过。用户指定的三个连接配置均使用 `proxy_id: "0"` 表示无代理，现接受该值、数字 `0`、空字符串及 `null`；有效代理引用继续拒绝，避免静默变成直连。
- 识别连接配置的 `conection_type: 100`，保留原有 `type: 1001` 兼容入口；存在不受支持的类型字段时跳过。
- `authentication_type: 2` 或非空 `secret_key_id` 会提示重新选择私钥，不读取来源引用的密钥文件，不复制密文密码。其他未映射字段仍报告。
- 没有可导入连接时，错误信息同时显示前五条跳过原因，避免提示用户查看一个实际不可见的兼容报告。

## 验证记录

| 范围 | 结果与证据 |
| --- | --- |
| 普通开发构建 | `cargo build -p qds6-app --locked --offline` 通过 |
| 解析与写入回归 | 三项迁移测试通过，包含四类来源、无代理标记、有效代理和非 SSH 拒绝、秘密排除、隔离 SQLite 正式恢复服务写入、恢复点和重复导入幂等。日志 `output/backup-import-migration.log` |
| 用户指定样例 | 三个 FinalShell JSON 分别通过只读预览，各识别一个 SSH 连接；未打印源密码、主机或用户名。日志 `output/finalshell-selected-samples.log`。样例不复制到仓库 |
| 对话框回调 | 平台模拟响应验证单文件参数、中文路径、预览失效、取消保留和忙碌保护通过。日志 `output/backup-picker-test.log`；该结果不是人工操作 Finder 对话框的验收 |
| 原生渲染与交互 | 真实 Metal 离屏流程通过，1440×900 / 960×640 下文件按钮可见且不越界，FinalShell 解析后出现一条预览，现有连接数不变。日志 `output/backup-import-layout.log`，截图 `output/ui-layout/finalshell-preview-*.png`、`restore-file-*.png`，已目视检查 |
| 领域、存储及服务测试 | 20 项通过，显式样例测试默认忽略并另行对三个文件执行。日志 `output/backup-import-tests.log`。原沙箱内 localhost 绑定被拒，允许本机临时端口后全量通过 |
| 静态检查 | 首轮全工作区全 feature 检查通过；补充回归后服务层与默认 feature 检查另记 `output/backup-import-services-clippy.log`、`output/backup-import-default-clippy.log`。最终全 feature 检查受并行设置改动引用尚未创建的 `settings_ui/tests.rs` 阻塞，诊断保留在 `output/backup-import-clippy.log`；未修改该任务的源码或伪称最终全 feature 通过 |
| 发行构建 | `cargo build -p qds6-app --release --locked --offline` 通过，无测试 feature；日志 `output/backup-import-release.log` |

## 内部测试包

构建号 `2026111103`，Apple Silicon macOS，产物目录 `output/release/0.1.0-macos-arm64-import-fix-20261111-03/`。打包使用已有脚本，APP、ZIP 已签名检查，DMG 在允许访问系统磁盘镜像服务后创建并校验。记录见 `output/backup-import-package.log`、`output/backup-import-dmg.log`，原沙箱内打包日志保留了创建 DMG 失败的诊断。

包内 `build-manifest.json` 记录发行二进制与源码快照指纹。打包后的其他设置和窗口修改不视为本包内容；工作树已有修改保留，不据此声称已提交或推送。

新 DMG 挂载、复制到独立目录并使用隔离数据启动通过，约 1.89 秒出现原生 QDS6 窗口，隔离数据库为零连接；只停止本次验证创建的进程。签名、ZIP/DMG 的 SHA-256 校验通过。记录见 `output/backup-import-install-check.log` 和包内 `installation-verification.json`、`installation-check/desktop-window.png`。该结果是当前 Mac 的复制安装和启动验证，不代表人工操作文件选择器或真实 SSH 服务验收。

未替换 `/Applications/QDS6.app`。未使用来源凭据连接真实 SSH 服务；导入后仍须补全密码或私钥。Windows、干净机器、Developer ID 签名与公证均不在本轮验收范围。
