// 功能：复制 QDS6 品牌、隔离验收截图和用户指定的完整原声配乐；时间：2026-11-14；作者：lq。
import {copyFileSync, mkdirSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import path from 'node:path';

const base = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const publicDir = path.join(base, 'public');
mkdirSync(publicDir, {recursive: true});
copyFileSync(path.join(base, '../assets/brand/png/qds6-512.png'), path.join(publicDir, 'qds6.png'));
copyFileSync(path.join(base, '../output/sidebar-files/linked-dark.png'), path.join(publicDir, 'workspace-mono-source.png'));
copyFileSync('/Users/liqiang/Downloads/audio_7d8b154412.mp3', path.join(publicDir, 'qds6-launch-music.mp3'));
console.log('Brand, isolated acceptance screenshot and the requested original music are ready.');
