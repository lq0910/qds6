// 功能：定义 55.6 秒、1080p、30 fps 的一镜到底产品发布视频；时间：2026-11-14；作者：lq。
import React from 'react';
import {Composition} from 'remotion';
import {LaunchFilm} from './LaunchFilm';

export const Root: React.FC = () => (
  <Composition id="QDS6Launch3D" component={LaunchFilm} durationInFrames={1668} fps={30} width={1920} height={1080} defaultProps={{}}/>
);
