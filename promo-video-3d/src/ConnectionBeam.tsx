// 功能：以多层光纤、流动脉冲与星芒构造贯穿产品世界的连接光束；时间：2026-11-14；作者：lq。
import React, {useMemo, useLayoutEffect} from 'react';
import * as THREE from 'three';
import {beamCurve} from './mascotMotion';

// 功能：沿管线 UV 推进能量，亮芯、长尾和宽光晕共享同一确定性时间；时间：2026-11-14；作者：lq。
const vertex = `varying vec2 vUv; void main(){vUv=uv;gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.0);}`;
const fragment = `
  varying vec2 vUv;
  uniform float time;
  uniform float strength;
  uniform vec3 tint;
  void main(){
    float phase=fract(vUv.x*5.0-time*0.55);
    float tail=exp(-phase*22.0);
    float head=exp(-phase*150.0);
    float energy=0.035+tail*0.7+head*1.5;
    gl_FragColor=vec4(mix(tint,vec3(0.88,1.0,0.98),head),energy*strength);
  }
`;

export function ConnectionBeam({time}:{time:number}) {
  const curve=beamCurve;
  const geometries=useMemo(()=>[0.012,0.045,0.12,0.26].map(r=>new THREE.TubeGeometry(curve,600,r,8,false)),[curve]);
  const uniforms=useMemo(()=>[1,0.32,0.09,0.025].map(strength=>({time:{value:0},strength:{value:strength},tint:{value:new THREE.Color('#40DBCA')}})),[]);
  uniforms.forEach(u=>{u.time.value=time;});
  const flare=useMemo(()=>{
    const canvas=document.createElement('canvas');canvas.width=256;canvas.height=256;
    const c=canvas.getContext('2d')!;
    const g=c.createRadialGradient(128,128,0,128,128,128);
    g.addColorStop(0,'rgba(240,255,255,1)');g.addColorStop(0.035,'rgba(196,255,245,1)');
    g.addColorStop(0.12,'rgba(75,233,210,0.5)');g.addColorStop(0.4,'rgba(42,179,199,0.08)');g.addColorStop(1,'rgba(20,140,180,0)');
    c.fillStyle=g;c.fillRect(0,0,256,256);
    const map=new THREE.CanvasTexture(canvas);map.colorSpace=THREE.SRGBColorSpace;return map;
  },[]);
  useLayoutEffect(()=>()=>{geometries.forEach(g=>g.dispose());flare.dispose();},[geometries,flare]);
  return <group>
    {geometries.map((geometry,i)=><mesh key={i} geometry={geometry}>
      <shaderMaterial vertexShader={vertex} fragmentShader={fragment} uniforms={uniforms[i]} transparent depthWrite={false} blending={THREE.AdditiveBlending} toneMapped={false}/>
    </mesh>)}
    {Array.from({length:5},(_,i)=>{
      const p=((time*0.55+i)/5)%1;
      const pos=curve.getPoint(p);
      return <group key={i} position={pos}>
        <sprite scale={[1.35,1.35,1]}><spriteMaterial map={flare} transparent depthWrite={false} blending={THREE.AdditiveBlending} toneMapped={false}/></sprite>
        <sprite scale={[2.4,0.13,1]}><spriteMaterial map={flare} opacity={0.7} transparent depthWrite={false} blending={THREE.AdditiveBlending} toneMapped={false}/></sprite>
        <pointLight color="#71F5DA" intensity={2.5} distance={3.5}/>
      </group>;
    })}
  </group>;
}
