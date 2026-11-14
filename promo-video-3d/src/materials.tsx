// 功能：彩色产品界面纹理与圆角三维实体构造工具；时间：2026-11-14；作者：lq。
import React, {useMemo, useLayoutEffect} from "react";
import * as THREE from "three";
import {MINT, PAPER, SIGNAL, AMBER} from "./timeline";

export type Vec = [number, number, number];
type PanelKind = 'terminal' | 'files' | 'database';
const MONO = '"SFMono-Regular", Menlo, monospace';
const SANS = '-apple-system, "PingFang SC", sans-serif';

// 功能：绘制只含虚构示例数据的宣传界面纹理，界面演绎不读取用户连接；时间：2026-11-14；作者：lq。
export function panelTexture(kind: PanelKind): THREE.CanvasTexture {
  const canvas = document.createElement('canvas');
  canvas.width = 1600; canvas.height = 1000;
  const c = canvas.getContext('2d')!;
  const rect = (x: number, y: number, w: number, h: number, color: string, r = 0) => {
    c.fillStyle = color; c.beginPath(); c.roundRect(x, y, w, h, r); c.fill();
  };
  const text = (s: string, x: number, y: number, size = 25, color = '#C8DAD7', family = SANS) => {
    c.fillStyle = color; c.font = `${size}px ${family}`; c.fillText(s, x, y);
  };
  const line = (x: number, y: number, x2: number, y2: number) => {
    c.strokeStyle = '#385762'; c.lineWidth = 2; c.beginPath(); c.moveTo(x, y); c.lineTo(x2, y2); c.stroke();
  };
  // 功能：为演绎界面应用深石墨蓝底色，并保留连接、查询与传输状态的品牌强调色；时间：2026-11-14；作者：lq。
  rect(0, 0, 1600, 1000, '#14242D', 24);
  rect(0, 0, 1600, 78, '#203A45', 24); rect(0, 42, 1600, 36, '#203A45');
  ['#E68D85', '#E3C37B', '#72BD9F'].forEach((color, i) => {c.fillStyle = color; c.beginPath(); c.arc(35 + i * 29, 39, 8, 0, Math.PI * 2); c.fill();});
  text('QDS6', 150, 48, 28, PAPER); text('工作台', 255, 47, 24, '#88A6AC');
  rect(1305, 24, 260, 32, '#284C4C', 16); text('●  工作连接已就绪', 1324, 47, 19, '#8EE9C9');
  rect(0, 78, 252, 922, '#172D36');
  text('连接', 32, 132, 22, '#7B9B9E');
  ['⌕  搜索连接', '⌄  开发环境', '   ◉  app-server', '   ◉  PostgreSQL', '   ◉  MySQL', '', '⌄  常用工具'].forEach((s, i) => {
    if (i === (kind === 'database' ? 3 : 2)) rect(18, 150 + i * 62, 216, 48, '#27564C', 8);
    text(s, 32, 183 + i * 62, 22, i === 0 ? '#65858C' : '#B3CCC9');
  });
  rect(252, 78, 1348, 62, '#192F39');
  const title = {terminal: '  >_   app-server / SSH', files: '  ▱   app-server / SFTP', database: '  ▦   PostgreSQL / workspace'}[kind];
  text(title, 276, 119, 23, '#D7F2E8');
  line(252, 140, 1600, 140);
  if (kind === 'terminal') {
    text('SSH TERMINAL', 297, 208, 18, '#6C9698', MONO);
    text('Welcome to your workspace.', 298, 279, 31, '#E0F1EB', MONO);
    text('A little less switching. A lot more focus.', 298, 327, 23, '#73969C', MONO);
    const lines: [string, string][] = [
      ['dev@app-server  ~', '#7EDDB7'], ['$ ssh app-server', '#E4EEEC'],
      ['Connection established.', '#73969C'], ['', '#73969C'],
      ['$ cd /workspace', '#E4EEEC'], ['$ ls', '#E4EEEC'],
      ['config/     data/     logs/     releases/', '#85C9DD'],
    ];
    lines.forEach(([s, color], i) => text(s, 298, 412 + i * 55, 29, color, MONO));
    text('$', 298, 854, 30, MINT, MONO); rect(339, 827, 17, 34, '#8BE6C6', 2);
    line(1060, 170, 1060, 945); text('SESSION / 02', 1100, 213, 18, '#688F94', MONO);
    text('$ tail -f app.log', 1100, 281, 23, '#D7E9E3', MONO);
    ['[ready]  service online', '[info]   worker started', '[info]   request received', '[done]   task completed'].forEach((s, i) => text(s, 1100, 350 + 58 * i, 20, i === 0 ? MINT : '#7F9E9F', MONO));
    rect(1100, 693, 419, 146, '#1B3840', 12); text('一个连接，多个视角。', 1123, 751, 24, '#C4DDD5'); text('保持同一工作上下文', 1123, 797, 21, SIGNAL);
  } else if (kind === 'files') {
    const cols = [280, 945];
    cols.forEach((x, i) => {
      text(i ? '远程文件' : '本地文件', x + 16, 209, 29, PAPER);
      rect(x, 241, 622, 52, '#1B353E', 8); text(i ? '/workspace/releases' : '~/Projects/qds6', x + 19, 275, 21, '#98B9BA', MONO);
      text('名称', x + 18, 345, 19, '#6C9197'); text('大小', x + 510, 345, 19, '#6C9197');
      ['config', 'assets', 'app.toml', 'schema.sql', 'README.md'].forEach((s, j) => {
        if (j === 2) rect(x, 375 + j * 73, 622, 66, '#21413C', 7);
        text(j < 2 ? '▱' : '▤', x + 18, 418 + j * 73, 27, j < 2 ? '#7FD9BD' : '#6D979E');
        text(s, x + 67, 418 + j * 73, 24, '#CCDFDA', MONO);
        text(j < 2 ? '—' : ['2 KB', '4 KB', '1 KB'][j - 2], x + 500, 418 + j * 73, 20, '#739296', MONO);
      });
    });
    line(918, 167, 918, 819); rect(280, 846, 1283, 92, '#1A4039', 10);
    text('✓  app.toml', 310, 890, 24, '#9FECCF', MONO); text('传输完成', 1331, 889, 24, '#9FECCF');
    rect(310, 911, 1216, 3, '#4DB990', 2);
  } else {
    text('SQL 查询', 291, 203, 25, PAPER); rect(1365, 166, 184, 48, '#295D4D', 8); text('▶  执行查询', 1386, 199, 21, '#B4F4D8');
    const sql = [
      [['SELECT ', SIGNAL], ['name, environment, status', '#E5EEE7']],
      [['FROM ', SIGNAL], ['connections', '#BEE2CA']],
      [['WHERE ', SIGNAL], ['status = ', '#D1E1DA'], ["'connected'", AMBER]],
      [['ORDER BY ', SIGNAL], ['name;', '#D1E1DA']],
    ];
    sql.forEach((tokens, i) => {
      text(String(i + 1), 295, 283 + i * 57, 23, '#4F7881', MONO);
      let x = 350;
      tokens.forEach(([s, color]) => {text(s, x, 283 + i * 57, 30, color, MONO); x += c.measureText(s).width;});
    });
    line(279, 526, 1560, 526); text('查询结果', 294, 577, 22, '#BCD6CF'); text('示例数据', 1438, 577, 19, '#62878B');
    rect(280, 602, 1279, 55, '#1F3B42');
    ['name', 'environment', 'status'].forEach((s, j) => text(s, 310 + j * 420, 638, 23, '#A0BFBD', MONO));
    ['app-server', 'primary-db', 'files-node'].forEach((s, i) => {
      rect(280, 659 + i * 77, 1279, 75, i % 2 === 0 ? '#112B32' : '#142E35');
      text(s, 310, 704 + i * 77, 25, '#D7E9E0', MONO); text('development', 730, 704 + i * 77, 24, '#769A9D', MONO);
      text('●  connected', 1150, 704 + i * 77, 23, '#7EDEB5', MONO);
    });
  }
  rect(0, 965, 1600, 35, '#1B3740'); text('QDS6  /  连接，从此简单。', 24, 989, 16, '#8DC2C3'); text('界面演绎 · 示例数据', 1384, 989, 16, '#8DC2C3');
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace; texture.anisotropy = 8;
  return texture;
}
// 功能：生成可复用圆角实体与真实厚度，配合室内柔光表现材质；时间：2026-11-14；作者：lq。
export function Slab({size, color = '#193943', metalness = 0.25, position = [0, 0, 0]}: {size: Vec; color?: string; metalness?: number; position?: Vec}) {
  // 功能：圆角轮廓与实体厚度独立，保持品牌图标的圆润外形；时间：2026-11-14；作者：lq。
  const geometry = useMemo(() => {
    const [w, h, d] = size;
    const r = Math.min(w, h) * (Math.abs(w - h) < 0.1 && w > 3 ? 0.15 : 0.035);
    const x = -w / 2, y = -h / 2;
    const shape = new THREE.Shape();
    shape.moveTo(x + r, y); shape.lineTo(x + w - r, y); shape.quadraticCurveTo(x + w, y, x + w, y + r);
    shape.lineTo(x + w, y + h - r); shape.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
    shape.lineTo(x + r, y + h); shape.quadraticCurveTo(x, y + h, x, y + h - r);
    shape.lineTo(x, y + r); shape.quadraticCurveTo(x, y, x + r, y);
    const bevel = Math.min(0.025, d * 0.2);
    const geo = new THREE.ExtrudeGeometry(shape, {depth: d - bevel * 2, bevelEnabled: true, bevelThickness: bevel, bevelSize: bevel, bevelSegments: 3, steps: 1, curveSegments: 10});
    geo.translate(0, 0, -d / 2 + bevel);
    return geo;
  }, [size[0], size[1], size[2]]);
  useLayoutEffect(() => () => geometry.dispose(), [geometry]);
  return <mesh geometry={geometry} position={position}><meshStandardMaterial color={color} metalness={metalness} roughness={0.24}/></mesh>;
}
