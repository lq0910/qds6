// 功能：有眼睛与短四肢的 Q 字母角色，沿各模型和光束跳跃并压扁回弹；时间：2026-11-14；作者：lq。
import React from 'react';
import {mascotPose} from './mascotMotion';
import {cameraPath,smooth,MINT} from './timeline';

export function QMascot({time}:{time:number}) {
  const pose=mascotPose(time),eye=cameraPath(time).eye;
  const facing=Math.atan2(eye[0]-pose.position[0],eye[2]-pose.position[2]);
  const appear=smooth((time-0.05)/0.3)*(1-smooth((time-49.1)/0.6));
  const introScale=0.72+0.28*smooth(time/3);
  const blink=Math.pow(Math.max(0,Math.cos(time*2.3)),32);
  if(time<0.1||time>49.7)return null;
  return <>
    <group position={pose.position} rotation={[0,facing,Math.sin(pose.u*Math.PI*2)*0.12]} scale={[pose.scaleXZ*appear*introScale,pose.scaleY*appear*introScale,pose.scaleXZ*appear*introScale]}>
      <QBody blink={blink} air={pose.u} wave={time<1.4?smooth((time-0.3)/0.2)*(0.7+0.3*Math.sin(time*14)):0}/>
    </group>
    {pose.index>0&&pose.elapsed>=0&&pose.elapsed<0.5&&<mesh position={[pose.contact[0],pose.contact[1]+0.012,pose.contact[2]]} rotation={[-Math.PI/2,0,0]} scale={0.25+pose.elapsed*2}>
      <ringGeometry args={[0.42,0.46,64]}/><meshBasicMaterial color="#9BFFE0" transparent opacity={(1-pose.elapsed/0.5)*0.75} depthWrite={false} toneMapped={false}/>
    </mesh>}
  </>;
}

// 功能：复用同一个 Q 角色外形，支持片尾单眼眨眼和挥手；时间：2026-11-14；作者：lq。
export function QBody({blink=0,air=0,wink=0,wave=0}:{blink?:number;air?:number;wink?:number;wave?:number}) {
  return <group>
      <mesh position={[0,0.48,0]}><torusGeometry args={[0.28,0.105,24,64]}/><meshPhysicalMaterial color={MINT} roughness={0.2} metalness={0.12} clearcoat={1} clearcoatRoughness={0.15}/></mesh>
      <mesh position={[0.24,0.25,0.03]} rotation={[0,0,Math.PI/4]}><capsuleGeometry args={[0.085,0.24,8,16]}/><meshPhysicalMaterial color={MINT} roughness={0.22} clearcoat={1}/></mesh>
      {[-1,1].map(side=><React.Fragment key={side}>
        <mesh position={[side*0.16,0.65,0.095]} scale={[1,1-(side===1?Math.max(blink,wink):blink)*0.88,1]}><sphereGeometry args={[0.068,20,16]}/><meshStandardMaterial color="#F7FFFD" roughness={0.3}/></mesh>
        <mesh position={[side*0.16+0.012,0.65,0.153]} scale={[1,1-(side===1?Math.max(blink,wink):blink)*0.88,1]}><sphereGeometry args={[0.032,16,12]}/><meshStandardMaterial color="#102F38" roughness={0.2}/></mesh>
        <mesh position={[side*0.15,0.075,0.02]} scale={[1,0.58,1.6]}><sphereGeometry args={[0.11,20,16]}/><meshPhysicalMaterial color="#B5F8E2" roughness={0.25} clearcoat={1}/></mesh>
        <mesh position={[side*0.39,0.4+Math.sin(air*Math.PI)*0.13+(side===1?wave*0.2:0),0]} rotation={[0,0,side*(0.3+Math.sin(air*Math.PI)*0.65)+(side===1?wave*1.5:0)]}><capsuleGeometry args={[0.055,0.17,8,12]}/><meshPhysicalMaterial color={MINT} roughness={0.25}/></mesh>
      </React.Fragment>)}
  </group>;
}
