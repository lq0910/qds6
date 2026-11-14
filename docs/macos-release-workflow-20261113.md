# macOS 双架构签名发布工作流

功能说明：GitHub Actions 在 main 推送后自动构建 Apple Silicon 和 Intel 安装包，默认使用 ad-hoc 测试签名并自动创建 tag 和 Release，可选手动 Developer ID 签名及 Apple 公证。更新日期：2026-11-14；作者：lq。

## 使用方式

将代码提交并推送到 `main`，自动运行 **QDS6 macOS release**：先复用 **QDS6 checks** 完成双架构质量检查，再打包、创建 tag 并发布到仓库 **Releases**。自动发布默认 `adhoc`，无需 Apple 开发者账号、证书或 Secrets。其他分支 push 和 PR 只运行 **QDS6 checks**，不发布；tag 推送不会重复触发检查或发布。

需要重新发布或选择正式签名时，可在 Actions 中选择 **QDS6 macOS release → Run workflow → main**，默认签名仍为 `adhoc`。main 的检查由发布流程调用，不再额外启动独立的 push 检查，避免重复编译。

版本号读取 `Cargo.toml`，无需预先创建 tag。每次运行生成 `v<Cargo版本>-internal.<工作流运行序号>`，例如 `v0.1.0-internal.1`；两架构的 APP 构建号统一使用 GitHub run ID。重跑同一运行使用原 tag；如果该 Release 已经公开则拒绝修改，要重新打包应启动新运行。

只有以下步骤全部成功才公开 Release：

1. 主分支检查；双架构 fmt、领域/存储/服务单测、严格 Clippy、默认客户端编译及发布脚本测试。仅 `developer-id` 模式额外检查 Apple 凭据。
2. 两台对应架构的 Mac 分别收集许可、原生 release 编译、检查二进制架构和非系统动态依赖。
3. 默认 APP 使用 ad-hoc 签名并严格验证签名。可选 `developer-id` 模式使用 Developer ID Application、时间戳和 Hardened Runtime，额外提交 Apple 公证、装订 APP 票据并通过 Gatekeeper 检查。
4. 生成 ZIP 和 DMG，验证 DMG 完整性并计算最终 SHA-256。仅 `developer-id` 模式额外给 DMG 自身签名、公证、装订并通过 Gatekeeper 检查。
5. 汇总验证四个安装包的哈希、版本、源码提交、构建号、干净工作树和所选签名模式；仅 `developer-id` 模式必须提供 Apple Accepted 记录。创建精确指向源码提交的 tag 及草稿 Release，上传完成后公开。

目前发布为预发布版本（Prerelease），因为产品仍处内测阶段；此标记不影响下载。Windows 不在本工作流范围。Mac runner 固定为 `macos-15`（arm64）和 `macos-15-intel`（x86_64），包最低系统版本为 macOS 14。

## 可选：配置 Developer ID 签名

默认 `adhoc` 跳过本节。如以后有开发者账号，在 Run workflow 中选择 `developer-id` 并配置下面的 Secrets；该模式缺凭据或公证失败时停止，不会自动降级。

需要有效的 Apple Developer Program 账号、包含私钥的 **Developer ID Application** 证书，以及具有公证权限的团队 App Store Connect API Key。Apple Development、Mac Distribution 和只有公钥的 `.cer` 不能替代 `.p12` 私钥证书。

在仓库 **Settings → Secrets and variables → Actions → New repository secret** 设置以下值。证书、私钥和密码不要提交到仓库或发到聊天中。

| Secret | 内容 |
| --- | --- |
| `MACOS_CERTIFICATE_P12_BASE64` | 从钥匙串访问导出的 Developer ID Application 证书及私钥 `.p12`，编码为单行 Base64 |
| `MACOS_CERTIFICATE_PASSWORD` | 导出 `.p12` 时设置的密码 |
| `MACOS_SIGNING_IDENTITY` | 证书完整名称，例如 `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_API_KEY_P8` | App Store Connect 团队 API Key 的 `.p8` 文件完整原文（保留换行） |
| `APPLE_API_KEY_ID` | 对应 Key ID |
| `APPLE_API_ISSUER_ID` | 对应 Issuer ID |

Mac 上可使用 `base64 -i /安全目录/DeveloperID.p12 | tr -d '\n' | pbcopy` 将证书编码放入剪贴板，再粘贴到 Secret。`security find-identity -v -p codesigning` 可查看本机证书名称。公证 API Key 使用 Apple 官方支持的团队密钥和权限；流程会验证凭据，缺少或无效时直接失败。

工作流自动使用仓库 `GITHUB_TOKEN`，只有 publish 任务具有 `contents: write`。如果仓库或组织策略禁止工作流写入，需要管理员调整该限制；无需额外配置个人访问令牌。

签名凭据只注入配置步骤，存入 runner 临时独立钥匙串，打包结束后通过 `always()` 清理并恢复原钥匙串搜索列表。安装包、缓存和 Actions artifact 不包含 `.p12`、`.p8` 或钥匙串。

## 下载产物

Release 包含 arm64 / x86_64 各一份 DMG 和 ZIP、合并的 `SHA256SUMS`、两架构的构建清单和动态库记录。仅 `developer-id` 模式包含 APP/DMG 公证报告。M 系列选 arm64，Intel 选 x86_64。下载 DMG 后把 `QDS6.app` 拖入 Applications；ZIP 解压后也可复制 APP。更新前退出旧应用。

默认 ad-hoc 包未通过 Apple 公证，首次打开可能被 macOS 拦截；核对下载来源和校验值后，如系统提供“仍要打开”，可在“系统设置 → 隐私与安全”中确认。不同系统版本和设备策略可能有不同表现，不保证一键打开。发布说明会明确标记 ad-hoc，不声称已通过 Apple 公证。

本地 `python3 scripts/package-macos.py` 同样默认使用 ad-hoc。正式签名本地命令可使用 `--signing-identity`、`--notary-profile` 和可选 `--signing-keychain`，对应钥匙串需事先配置公证凭据。

## 验证记录与边界

2026-11-13 已确认 GitHub 最近一次失败运行 `43374812605` 的失败步骤为 `cargo fmt --all -- --check`。本次修正对应格式，并清理严格 Clippy 发现的两个未使用 Excel 导出字段及嵌套条件；Excel 导出已使用原生保存对话框，无需这些闲置字段。

用户确认暂时没有开发者账号，因此默认改为 ad-hoc。本机没有有效 Developer ID 证书；正式签名和公证需以后配置 Secrets 验证。双架构云端构建及浏览器下载后的真实设备安装，须在推送并执行工作流后分别验收；代码和静态检查通过不能代替这些结果。Apple 官方说明：[Developer ID](https://developer.apple.com/developer-id/)、[公证要求](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)、[自定义公证流程](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow)。Runner 对照见 [GitHub 官方文档](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)。

本次本地验证：actionlint 1.7.12、Python 编译检查、7 项发布集合测试通过，覆盖无 Apple 公证记录的 ad-hoc 发布、正式模式公证记录及模式不匹配拒绝发布。此前本轮 fmt、严格 workspace/all-targets/all-features Clippy、默认客户端编译均通过；领域/存储/服务测试共 32 项通过，2 项按原设计忽略。本次调整仅涉及工作流、Python 发布门禁及文档。未触发远端发布，未创建真实 tag 或 Release。
