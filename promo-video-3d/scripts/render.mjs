// 功能：以同一确定性场景渲染关键帧及 1080p H.264 成片；时间：2026-11-14；作者：lq。
import {bundle} from '@remotion/bundler';
import {openBrowser, renderMedia, renderStill, selectComposition} from '@remotion/renderer';
import {copyFileSync, mkdirSync, writeFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import path from 'node:path';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const output = path.resolve(root, '../output/qds6-launch-3d');
mkdirSync(output, {recursive: true});
copyFileSync(path.join(root, 'preview.html'), path.join(output, 'index.html'));
const stillOnly = process.argv.includes('--stills');
const browserExecutable = process.env.QDS6_CHROME || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const chromiumOptions = {gl: 'angle', flags: {'use-angle': 'metal'}};
const serveUrl = await bundle({entryPoint: path.join(root, 'src/index.ts'), outDir: path.join(root, 'build'), publicDir: path.join(root, 'public')});
console.log('Bundle ready:', serveUrl);
const browser = await openBrowser('chrome', {browserExecutable, chromiumOptions});
try {
  const composition = await selectComposition({serveUrl, id: 'QDS6Launch3D', puppeteerInstance: browser});
  // 功能：支持指定角色接触帧，逐个验收落点和变形；时间：2026-11-14；作者：lq。
  const requestedFrames = process.argv.find(arg=>arg.startsWith('--frames='));
  const frames = requestedFrames ? requestedFrames.slice(9).split(',').map(Number) : process.argv.includes('--first') ? [120] : [72,234,414,558,702,840,1113,1284,1605];
  for (const frame of frames) {
    await renderStill({serveUrl, composition, frame, output: path.join(output, `frame-${String(frame).padStart(4, '0')}.png`), puppeteerInstance: browser, imageFormat: 'png', logLevel: 'warn', timeoutInMilliseconds: 120000});
    console.log(`Still ${frame} ready.`);
  }
  if (!stillOnly && !process.argv.includes('--first')) {
    let lastBucket = -1;
    const started = Date.now();
    await renderMedia({serveUrl, composition, puppeteerInstance: browser, outputLocation: path.join(output, 'QDS6-Launch-3D-1080p.mp4'), codec: 'h264', audioCodec: 'aac', audioBitrate: '256k', pixelFormat: 'yuv420p', colorSpace: 'bt709', crf: 18, concurrency: 2, imageFormat: 'png', timeoutInMilliseconds: 120000, metadata: {title: 'QDS6 — Connections, simplified.', artist: 'lq', comment: 'Three.js original continuous product film · 2026-11-14'}, onProgress: ({renderedFrames, encodedFrames, progress}) => {
      const bucket = Math.floor(progress * 100 / 5);
      if (bucket !== lastBucket) {lastBucket = bucket; console.log(`${Math.round(progress * 100)}% · rendered ${renderedFrames}/${composition.durationInFrames} · encoded ${encodedFrames} · ${Math.round((Date.now() - started) / 1000)}s`);}
    }});
    writeFileSync(path.join(output, 'render.json'), JSON.stringify({date: '2026-11-14', author: 'lq', composition, renderer: 'Three.js / React Three Fiber / Remotion / ANGLE Metal', elapsedSeconds: (Date.now() - started) / 1000}, null, 2));
    console.log('Film complete:', path.join(output, 'QDS6-Launch-3D-1080p.mp4'));
  }
} finally {
  await browser.close({silent: true});
}
