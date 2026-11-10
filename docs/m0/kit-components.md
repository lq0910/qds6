# M0 T004：Mac Kit 组件验证

> 日期：2026-11-10｜作者：lq｜结论：Mac 组件最小链路通过；Windows 延至 W001。

本轮实现独立原生组件验证窗口，覆盖对象树、虚拟数据表、弹出菜单、标签和停靠分屏。输入全部是合成数据，不打开正式连接库、不联网、不执行 SQL。正式客户端尚未接入对象树或数据库结果表。

## 版本与环境

- 基线 `1575f7a`；本轮变更仍在未提交工作树，基线提交不代表新增代码。
- macOS 27.0.1（26A434），Apple M4 Pro / 24 GB / arm64；Rust 1.99.1，debug 构建。
- GPUI Kit 固定 `ab7bd640021e062094cb4fd1e15fc6b465ec6a47`，默认 `gpui-pre` 后端不变，未增加第三方依赖。
- `Cargo.lock` SHA-256：`57511051b818c17fd7e788863481518e9beba37c17dd3ba61f4f7d23dc372e82`。
- 自动交互使用 Kit `HeadlessAppContext` 和公开测试 API，字体及 Metal 来自当前 Mac；逻辑窗口 1440×900 / 960×640，截图分别为 2880×1800 / 1920×1280。
- 沙箱内启动遇到 `NSPasteboard` 系统服务不可用；允许访问 Mac 原生服务后运行通过。该环境失败与成功运行分别记录，不当作业务失败或 Windows 证据。

## 复现

```sh
# 功能：打开独立组件实验窗口，供后续人工桌面检查；时间：2026-11-10；作者：lq。
cargo run -p qds6-app --example component-lab --features component-lab --locked

# 功能：运行真实组件交互与离屏 Metal 绘制，生成截图和结果；时间：2026-11-10；作者：lq。
cargo run -p qds6-app --example component-smoke --features ui-tests --locked
```

实现位于 `crates/qds6-app/src/component_lab.rs`，验收脚本位于 `crates/qds6-app/src/component_lab/smoke.rs`。默认客户端构建不包含实验工作台，仅显式 feature 开启。

## 实际结果

两种尺寸均通过以下交互断言：

| 能力 | 操作与结果 |
| --- | --- |
| 树 | 鼠标折叠分组；方向键重新展开、展开子树并选中 `public.users`；折叠时子项不在可见索引中，展开后可定位 |
| 虚拟表 | 合成源 100,000 行 × 20 列，单元格按索引生成；点击单元格选中，方向键移动，首尾行与末列定位均显示正确单元格 |
| 菜单 | 点击实际弹出菜单执行重置布局、首尾定位及主题切换；操作后菜单关闭 |
| 标签 | 点击切换 SQL/说明；拖动重排；将 SQL 拖至结果分组并成为活动面板 |
| 停靠 | 调整侧栏分隔线并断言宽度变化；面板工具栏放大/恢复；原上下分屏可重置 |
| 草稿 | 输入中文 SQL 后，标签切换、重排、跨组拖动、布局重置和面板放大恢复始终保留同一草稿 |

| 逻辑窗口 | 初始可见范围（零起始、右端不含） | 末行可见范围 | 全部交互帧累计绘制单元格次数 |
| --- | --- | --- | --- |
| 1440×900 | 行 `0..16`，列 `0..8` | 行 `99984..100000` | 24,848 |
| 960×640 | 行 `0..8`，列 `0..6` | 行 `99992..100000` | 9,248 |

源共有 2,000,000 个单元格，但不创建全量二维数据缓存。断言要求每个稳定帧的可见行少于 100、可见列少于 20；累计次数包含多次重绘，不能作为常驻单元格数量。定位末行后的四次稳定绘制分别约 79 ms / 46 ms；此值含测试帧推进，不作为交互延迟或 P95 性能结论。尚未测量内存、CPU、持续滚动帧率或真实查询负载。

截图已目视检查，完整生成物位于被 Git 忽略的 `output/m0-components/`：

- [1440 浅色](../../output/m0-components/lab-1440-light.png)
- [1440 深色末行](../../output/m0-components/lab-1440-dark-last.png)
- [960 浅色](../../output/m0-components/lab-960-light.png)
- [960 深色末行](../../output/m0-components/lab-960-dark-last.png)
- [自动结果](../../output/m0-components/results.txt)

## API 接入与缺口

| Kit API | 接入结论 |
| --- | --- |
| `TreeState` / `TreeItem` / `tree` | 提供折叠、键盘导航和可见索引；应用负责行缩进、图标与展开指示。TreeState 当前不实现 `Focusable`，可调用其 `focus`，面板外壳另持焦点句柄 |
| `TableDelegate` / `TableState` / `DataTable` | 委托提供总行列数及单元格内容，`visible_range` 可核验二维虚拟化；默认按索引生成展示文本，不据此证明数据库类型适配或流式结果缓存 |
| 表头主题 | 默认深色表头在截图中对比不足；本项目覆写 `render_th`，使用当前主题前景色改善可读性，未修改第三方源码 |
| `DropdownMenu` / `PopupMenuItem` | 可使用点击回调，无需虚构全局动作；本轮只验证弹出菜单，系统原生菜单栏仍需桌面人工检查 |
| `BasePanel` + `Panel` | 分别实现行为和外观；必须用 `panel_handle` 包装后交给 `DockLayout::panel_view`，保证标题、工具栏等外观可恢复 |
| `DockSkin::dock_area` / `DockLayout` | 原生停靠、标签重排和分屏可用；布局重置复用内容实体，避免丢失草稿；本轮不落盘布局、不实现应用重启恢复 |
| 树行的测试 ID | Tree 自带整数行 ID；自定义 ListItem 使用 `("lab-tree-item", ix)`，避免同名整数 ID 造成自动化定位歧义 |

下一步 T005 仍需真实中文 IME、撤销/替换、CRLF 往返、5 MiB 与超长单行验证。输入文本注入不等于真实输入法验收。屏幕缩放、多显示器、辅助功能、持续拖动手感和原生前台窗口验收均未以本轮离屏截图代替。

## 检查分类

| 检查 | 本轮结果 |
| --- | --- |
| 编译 | `cargo check -p qds6-app --all-targets --all-features --locked`、默认客户端及 `component-lab` 示例构建通过 |
| 单元测试 | core 3 项、storage 2 项通过 |
| 静态检查 | `cargo fmt --all -- --check`、全 workspace / all-targets / all-features Clippy `-D warnings` 通过 |
| 原有 UI 回归 | `ui-smoke` 连接输入、验证、SQLite 保存/重读、收藏、SQL 复制/保留、主题和 Metal 绘制通过 |
| 新组件交互与绘制 | `component-smoke` 在两种尺寸通过，四张截图已检查 |
| 普通桌面运行 | 本轮编译了人工检查入口；未用普通前台窗口进行人工交互验收 |
| 真实服务 / 安装 / Windows | 未执行；Windows 按新计划延期，Mac 其余 M0 任务继续推进 |

T004（Mac）按组件最小链路通过勾选，不代表 T003 安装、T005 输入法或真实服务任务完成。
