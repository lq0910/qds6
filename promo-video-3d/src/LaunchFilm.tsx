// 功能：将连续的 Three.js 运镜、产品卖点和完整指定配乐组为 55.6 秒发布片；时间：2026-11-14；作者：lq。
import React from 'react';
import {AbsoluteFill, Img, Sequence, staticFile, useCurrentFrame, useVideoConfig} from 'remotion';
import {Audio} from '@remotion/media';
import {ThreeCanvas} from '@remotion/three';
import {Scene} from './Scene';
import {landings} from './mascotMotion';
import {FinaleMascot,finaleHits} from './FinaleMascot';
import {smooth, FONT, PAPER, INK, MINT, SIGNAL, AMBER} from './timeline';

// 功能：把产品卖点作为随镜头进入的短暂说明，避免停住画面形成标题卡；时间：2026-11-14；作者：lq。
const copy = [
  {at:0.45,until:3.0,label:'QDS6',headline:'连接，从此简单。',detail:'服务器 · SSH · SFTP · 数据库'},
  {at:3.3,until:5.2,label:'BUILT WITH RUST',headline:'Rust 原生构建',detail:'GPUI 原生界面渲染'},
  {at:5.55,until:8.4,label:'SSH TERMINAL',headline:'直达服务器，马上开工。',detail:'SSH 连接 · 多会话 · 分屏'},
  {at:9.1,until:12.1,label:'ONE CONTEXT',headline:'少些切换，多些专注。',detail:'同一连接，延续手上工作'},
  {at:12.35,until:14.7,label:'FLUID DESKTOP',headline:'原生渲染，响应利落。',detail:'Rust 原生构建 · GPUI 渲染'},
  {at:15.15,until:17.4,label:'SFTP',headline:'文件，在连接间流动。',detail:'本地与远程，一目了然'},
  {at:18.15,until:20.5,label:'TRANSFER / EDIT',headline:'传输和编辑，就在身边。',detail:'双栏浏览 · 传输队列 · 远程编辑'},
  {at:21,until:23.5,label:'NO CONTEXT SWITCH',headline:'连接不断，思路不散。',detail:'让下一步工作顺势发生'},
  {at:24.0,until:26.4,label:'MYSQL · POSTGRESQL',headline:'连上数据库，直面数据。',detail:'MySQL · PostgreSQL'},
  {at:27.1,until:29.5,label:'QUERY / DESIGN',headline:'从 SQL，到表格与结构。',detail:'查询 · 表设计 · 数据导入导出'},
  {at:30.1,until:32.6,label:'ONE WORKSPACE',headline:'服务器和数据，在一个窗口。',detail:'终端 · 文件 · 数据库'},
  {at:33.15,until:35.9,label:'FOCUS ON YOUR WORK',headline:'一个工作台，少一些来回。',detail:'SSH 与 SFTP · MySQL 与 PostgreSQL'},
  {at:37.15,until:39.8,label:'NATIVE WORKSPACE',headline:'把连接，收进同一工作台。',detail:'打开一个窗口，继续手上的工作'},
  {at:41,until:43.8,label:'BUILT FOR THE FLOW',headline:'轻快、直接、连贯。',detail:'为日常运维与数据库工作而造'},
  {at:46.0,until:49.0,label:'QDS6',headline:'Connections, simplified.',detail:'macOS 内测版 · v0.1.0'},
];

function Headline({time}:{time:number}) {
  const item=copy.find(c=>time>=c.at-0.18&&time<=c.until+0.25);
  if(!item)return null;
  const itemIndex=copy.indexOf(item);
  const enter=smooth((time-item.at+0.16)/0.58);
  const leave=smooth((time-(item.until-0.42))/0.42);
  const opacity=smooth((time-item.at+0.18)/0.24)*(1-smooth((time-(item.until-0.16))/0.38));
  const side=itemIndex%3;
  const right=side===2;
  const top=['21%','43%','61%'][itemIndex%3];
  const x=(1-enter)*(right?-270:270)+leave*(right?520:-520);
  const y=(1-enter)*(side===1?90:-44)+leave*(side===1?-110:50);
  const z=(1-enter)*-320+leave*480;
  const scale=1.12-enter*0.12+leave*0.18;
  const blur=(1-enter)*9+leave*6;
  const accent=item.label.includes('DATABASE')||item.label.includes('QUERY')?AMBER:item.label.includes('SSH')||item.label.includes('SFTP')?SIGNAL:MINT;
  // 功能：标题字形从不同空间方向汇聚并分散，随三维镜头穿行而改变构图；时间：2026-11-14；作者：lq。
  return <div style={{position:'absolute',left:right?'auto':'6.4%',right:right?'6.4%':'auto',top, maxWidth:'78%',textAlign:right?'right':'left',opacity,transform:`perspective(1300px) translate3d(${x}px,calc(-50% + ${y}px),${z}px) scale(${scale}) rotateY(${(1-enter)*(right?16:-16)+leave*(right?-12:12)}deg)`,transformOrigin:right?'right center':'left center',filter:`blur(${blur}px)`,willChange:'transform,opacity,filter'}}>
    <div style={{fontSize:68,lineHeight:1.15,fontWeight:690,color:PAPER,textShadow:'0 8px 35px rgba(0,0,0,0.36)',whiteSpace:'nowrap'}}>
      {Array.from(item.headline,(ch,index)=>{
        const delay=(index%5)*0.024;
        const arrive=smooth((time-item.at+0.16-delay)/0.48);
        const depart=smooth((time-(item.until-0.42+delay*0.3))/0.36);
        const seed=(index*7+itemIndex*3)%5;
        const dx=(seed%2?1:-1)*((1-arrive)*82+depart*145);
        const dy=((seed%3)-1)*((1-arrive)*58+depart*86);
        const rotate=((seed%2?1:-1)*((1-arrive)*21+depart*28));
        return <span key={index} style={{display:'inline-block',opacity:arrive*(1-depart),transform:`translate3d(${dx}px,${dy}px,0) rotate(${rotate}deg) scale(${0.74+arrive*0.26+depart*0.25})`,filter:`blur(${(1-arrive)*8+depart*5}px)`}}>{ch}</span>;
      })}
    </div>
    <div style={{fontSize:19,color:accent,letterSpacing:0.25,marginTop:18,opacity:enter*(1-leave),transform:`translate3d(${(1-enter)*(right?-95:95)+leave*(right?140:-140)}px,0,0)`}}>{item.detail}</div>
  </div>;
}

// 功能：片尾独立展示真实应用图标与指定署名；时间：2026-11-14；作者：lq。
function Endmark({time}:{time:number}) {
  const opacity=smooth((time-50.2)/0.8);
  return <div style={{position:'absolute',inset:0,display:'flex',alignItems:'center',justifyContent:'center',pointerEvents:'none',opacity}}>
    <div style={{display:'flex',flexDirection:'column',alignItems:'center',transform:`translate3d(0,${(1-opacity)*32}px,0) scale(${0.86+0.14*opacity})`,filter:`drop-shadow(0 0 42px ${MINT}48)`}}>
      <Img src={staticFile('qds6.png')} style={{width:246,height:246}}/>
      <div style={{fontSize:25,letterSpacing:3.3,color:'#D2E2E4',marginTop:23}}>非人类程序员出品</div>
    </div>
  </div>;
}

export const LaunchFilm:React.FC=()=>{
  const frame=useCurrentFrame();
  const {fps,width,height}=useVideoConfig();
  const time=frame/fps;
  const finish=smooth((time-49.6)/0.9);
  const pass=Math.max(...[5.0,14.9,23.9,32.9,44.9].map(at=>Math.max(0,1-Math.abs(time-at)/0.11)));
  return <AbsoluteFill style={{background:INK,color:PAPER,fontFamily:FONT,overflow:'hidden'}}>
    <Audio src={staticFile('qds6-launch-music.mp3')} volume={0.88}/>
    {/* 功能：落地音效与小人落点共用帧时间，短音不覆盖背景配乐；时间：2026-11-14；作者：lq。 */}
    {landings.filter(landing=>landing.kind!=='intro').map((landing,i)=><Sequence key={i} from={Math.round(landing.at*fps)} durationInFrames={15} layout="none">
      <Audio src={staticFile('q-bounce.wav')} volume={landing.kind==='key'?0.48:0.36}/>
    </Sequence>)}
    <ThreeCanvas width={width} height={height} camera={{fov:40,near:0.1,far:140}} dpr={1} gl={{antialias:true,alpha:false,powerPreference:'high-performance'}} style={{opacity:1-smooth((time-49.1)/1.1),filter:`contrast(1.06) brightness(${1+pass*0.12})`}}>
      <Scene time={time}/>
    </ThreeCanvas>
    <AbsoluteFill style={{pointerEvents:'none',background:'radial-gradient(ellipse at 50% 46%, transparent 58%, rgba(5,12,18,0.34) 100%), linear-gradient(0deg,rgba(5,12,18,0.62),transparent 35%,transparent 84%,rgba(5,12,18,0.18))'}}/>
    <div style={{position:'absolute',left:120,top:72,display:'flex',alignItems:'center',gap:12,opacity:1-finish}}>
      <Img src={staticFile('qds6.png')} style={{width:40,height:40}}/>
      <span style={{fontSize:21,fontWeight:650}}>QDS6</span>
      <span style={{height:15,width:1,background:'#527078',margin:'0 9px'}}/>
      <span style={{fontSize:11,letterSpacing:2.3,color:'#B7D1D5'}}>BUILT WITH RUST · POWERED BY GPUI</span>
    </div>
    <div style={{position:'absolute',top:83,right:120,fontSize:11,letterSpacing:1.7,color:'#A0A0A0',opacity:1-finish}}>macOS · v0.1.0</div>
    <Headline time={time}/>
    <div style={{position:'absolute',right:120,bottom:76,fontSize:11,letterSpacing:1.2,color:'#999',opacity:1-finish}}>
      {time>33&&time<45?'QDS6 原生工作台 · 隔离环境截图':time>=5&&time<13?'QDS6 真实界面':time<33?'功能概念演绎':''}
    </div>
    <Endmark time={time}/>
    <FinaleMascot time={time} width={width} height={height}/>
    {finaleHits.map(at=><Sequence key={at} from={Math.round(at*fps)} durationInFrames={15} layout="none"><Audio src={staticFile('q-bounce.wav')} volume={0.4}/></Sequence>)}
  </AbsoluteFill>;
};
