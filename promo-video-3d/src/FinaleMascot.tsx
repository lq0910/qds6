// 功能：片尾 Q 小人翻滚登场、落到应用图标上沿回弹并眨眼挥手；时间：2026-11-14；作者：lq。
import React from 'react';
import {ThreeCanvas} from '@remotion/three';
import {QBody} from './QMascot';
import {clamp,smooth,mix} from './timeline';
export const finaleHits=[51.9,52.55];
export function FinaleMascot({time,width,height}:{time:number;width:number;height:number}) {
  const entry=clamp((time-50.55)/1.35);
  const rebound=clamp((time-51.9)/0.65);
  const stage=entry<1?4*entry*(1-entry)*0.85:4*rebound*(1-rebound)*0.32;
  const hit=finaleHits.reduce((value,t)=>value+Math.exp(-Math.pow((time-t)/0.085,2)),0);
  const pose=smooth((time-52.75)/0.6);
  const x=mix(-2.3,0.24,smooth(entry));
  const footY=0.8;
  const y=footY+stage;
  const turn=-Math.PI*2*(1-smooth(entry))-pose*0.17;
  const squash=1-hit*0.28;
  const wave=pose*(0.8+0.2*Math.sin(time*12));
  const wink=smooth((time-53.3)/0.15)*(1-smooth((time-54.2)/0.2));
  const visible=smooth((time-50.55)/0.18);
  if(time<50.55)return null;
  return <ThreeCanvas width={width} height={height} dpr={1} camera={{position:[0,0,8],fov:40,near:0.1,far:30}} gl={{alpha:true,antialias:true}} style={{position:'absolute',inset:0,pointerEvents:'none'}}>
    <ambientLight intensity={1.5}/>
    <directionalLight position={[-3,5,6]} intensity={3} color="#E8FFF5"/>
    <directionalLight position={[3,2,-2]} intensity={2} color="#78D9E6"/>
    <group position={[x,y,0]}>
      <group position={[0,0.28,0]} rotation={[0,0,turn]} scale={[0.72/Math.sqrt(squash)*visible,0.72*squash*visible,0.72*visible]}>
        <group position={[0,-0.39,0]}><QBody air={entry<1?entry:rebound} wink={wink} wave={wave}/></group>
      </group>
    </group>
  </ThreeCanvas>;
}
