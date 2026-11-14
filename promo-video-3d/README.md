# QDS6 3D 发布片

功能：独立 Three.js 产品发布片及可复现渲染工程。日期：2026-11-14；作者：lq。

55.6 秒 / 1920 × 1080 / 30 fps / H.264 + AAC。一个连续三维世界里，Q 标识建成、终端键帽接连浮起，文件在本地与服务器目录之间飞行，数据库分层展开，服务器/文件/数据库实体沿轨道向品牌图形合流，最后把实体 Q 标识交给应用 Logo 收束。镜头由真实 3D 坐标、目标点、视场变化及平滑轨道驱动；标题字形随镜头汇聚、掠过与散开。连接光路使用四层可加光材质、移动脉冲长尾、星芒和局部照明，保留真实物体遮挡。产品信息与节奏跟随用户提供的完整 55.59 秒 MP3。配色以钛灰、深青黛和冰银构成立体层次，薄荷绿、信号青与少量暖琥珀标示品牌和状态。

成片与预览在 `../output/qds6-launch-3d/`。双击其中 `index.html` 可离线播放；MP4 可直接分享。

## 重现

需要 Node.js 22+ 与本机 Chrome。以下命令在本目录运行。

```sh
# 功能：安装锁定依赖、准备品牌/截图和原创配乐；时间：2026-11-14；作者：lq。
npm ci
npm run assets
npm run typecheck

# 功能：打开可拖动时间轴的本地视频预览；时间：2026-11-14；作者：lq。
npm run studio

# 功能：渲染六个章节截图和完整视频，输出到项目 output 目录；时间：2026-11-14；作者：lq。
npm run render

# 功能：只渲染关键帧；时间：2026-11-14；作者：lq。
node scripts/render.mjs --stills
```

默认使用 macOS Chrome 的 ANGLE Metal 后端；可通过 `QDS6_CHROME` 指定 Chrome 路径。其他平台需相应修改 `scripts/render.mjs` 中的 ANGLE 后端选项。这里只验证 Mac 视频生产，不代表 QDS6 Windows 客户端验收。

## 修改

- `src/timeline.ts`：章节时长、文案、颜色、连续相机路径。
- `src/Scene.tsx`：真实 Three.js 几何、独立相机/目标轨道、文件飞行动画、数据对象和界面纹理。
- `src/LaunchFilm.tsx`：中文排版、章节指示和音频组合。
- `scripts/prepare-assets.mjs`：品牌及隔离截图复制、用户提供配乐接入。
- `preview.html`：离线 MP4 播放器和章节跳转。

所有运动由 Remotion 当前帧驱动，跳帧、逐帧导出与预览使用同一时间轴。没有依赖屏幕录制、真实服务或用户凭据。

## 素材与边界

产品宣传语与色彩来自本项目产品方案和品牌规范。品牌模型由几何体构建，品牌 PNG 来自 `assets/brand/png/qds6-512.png`。SSH 场景主屏采用用户提供的服务器工作区截图，后方副屏采用用户提供的深色数据库对象页；截图完整保存在 public/qds6-real-server.png 与 public/qds6-real-database.png。工作台原生截图来自 `output/sidebar-files/linked-dark.png` 的本地隔离验证。截图固定复制进 `public`，重新导出视频不要求本机仍保留测试环境。

内容以 QDS6 产品说明为依据，强调 Rust 原生构建、GPUI 渲染、服务器与数据库连接、SSH、SFTP 和统一工作台。速度用「轻快流畅」表达，没有添加无法验证的性能数字。片尾标明 `macOS · 内测版 / v0.1.0` 和「非人类程序员出品」。外部参考帖只作运镜及物件衔接参考，没有使用其画面、音乐或品牌资产。

音乐 `public/qds6-launch-music.mp3` 由用户指定，原声以 `0.88` 音量完整贯穿全片。第三方运行库仍遵循各自许可：随 npm 包提供的声明复制到 `third-party/`，其余依赖的来源见该目录清单；不把第三方代码改署为 lq。源码包不分发第三方运行库，安装依赖时保留 npm 包的原始许可。外部参考视频及其配乐未复制到本片。

## Q 字母引路角色

薄荷绿 Q 字母角色带眼睛、短手脚，依次跳过品牌标识、原生芯片、SSH 键帽、服务器、文件夹、飞行文件、数据库、工作台，穿插沿连接光束落脚。数据库盘面没有方块装饰。`src/mascotMotion.ts` 统一接触点与音效时刻；`src/QMascot.tsx` 负责外形、体积补偿的压缩回弹、眨眼和落地光圈；`src/ConnectionBeam.tsx` 与角色共用光束曲线。

原创落地音由 `python3 scripts/make-bounce.py` 合成到 `public/q-bounce.wav`，每次落地同步叠加到用户指定音乐上。音效无需外部音源。新增镜头截图用 `node scripts/render.mjs --stills --frames=189,399,546,756` 重现。
