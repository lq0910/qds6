# 第三方来源

功能：记录视频工程依赖来源，保留上游作者与许可。日期：2026-11-14；作者：lq。

源码包只包含自有源文件及 QDS6 素材，不分发 `node_modules` 或 Webpack 产物。已从本机安装包复制提供的 LICENSE / THIRD_PARTY_LICENSES 原文，不改署名。未单独携带 LICENSE 文件的 npm 包仍应按上游许可使用，完整依赖由 `package-lock.json` 固定。

| 依赖 | 版本 | 上游 | 许可 |
| --- | --- | --- | --- |
| Three.js | 0.170.0 | https://github.com/mrdoob/three.js | MIT，已附原文 |
| React / React DOM | 18.3.1 | https://github.com/facebook/react | MIT，已附原文 |
| React Three Fiber | 8.18.0 | https://github.com/pmndrs/react-three-fiber | MIT，上游声明 |
| Remotion / CLI / Media | 4.0.543 | https://github.com/remotion-dev/remotion | 按 Remotion 上游 LICENSE.md，已附主包原文 |
| @remotion/three | 4.0.543 | https://github.com/remotion-dev/remotion | npm 元数据标注 MIT，上游声明 |

Three.js 的 RoomEnvironment 及 ExtrudeGeometry 由包导入，不复制上游实现。字体使用本机系统字体渲染为视频，不分发字体文件。音乐来自用户指定的 `audio_7d8b154412.mp3`，外部参考视频及其音频未纳入素材。
