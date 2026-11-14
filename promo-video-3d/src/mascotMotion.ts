// 功能：统一 Q 小人的模型落点、光束路径和音效时刻；时间：2026-11-14；作者：lq。
import * as THREE from 'three';
import {smooth,clamp,cameraPath} from './timeline';
export const beamCurve=new THREE.CatmullRomCurve3(Array.from({length:101},(_,i)=>{
  const x=-8+i*1.12;
  return new THREE.Vector3(x,0.3+Math.sin(x*0.31)*1.2,-3.1+Math.sin(x*0.35)*0.85);
}));
type Point=[number,number,number];
const transform=(p:Point,translation:Point,rotation:Point):Point=>new THREE.Vector3(...p).applyEuler(new THREE.Euler(...rotation)).add(new THREE.Vector3(...translation)).toArray() as Point;
export const landings=[
  {at:0.3,kind:'intro',x:0},{at:2.7,kind:'brand',x:0},{at:3.8,kind:'chip',x:8.8},
  ...Array.from({length:6},(_,i)=>({at:6.3+i*0.65,kind:'key',x:i})),
  {at:11.3,kind:'beam',x:23},{at:13.3,kind:'server',x:27.5},
  {at:15.5,kind:'folder-left',x:32.75},{at:18.2,kind:'file',x:4},
  {at:20,kind:'folder-right',x:39.25},{at:22.3,kind:'beam',x:46},
  {at:25.2,kind:'database',x:54},{at:28,kind:'database-edge',x:54},
  {at:30.7,kind:'beam',x:61.5},{at:34.7,kind:'workspace',x:70},
  {at:38.5,kind:'beam',x:74},{at:42.3,kind:'merge',x:79},
  {at:45.1,kind:'beam',x:85},{at:48,kind:'closing',x:90},
];
// 功能：跟随正在运动的实体表面计算落脚点，光束落点复用同一条曲线；时间：2026-11-14；作者：lq。
export function surface(node:typeof landings[number],t:number):Point {
  switch(node.kind){
    // 功能：开场角色先在镜头左下方打招呼，再从固定起跳点登上品牌模型；时间：2026-11-14；作者：lq。
    case 'intro':{
      const path=cameraPath(Math.min(t,1.4));
      const camera=new THREE.PerspectiveCamera(path.fov,1920/1080,0.1,140);
      camera.position.set(...path.eye);camera.lookAt(...path.target);camera.rotateZ(path.roll);camera.updateMatrixWorld();
      const ray=new THREE.Vector3(-0.74,-0.82,0.5).unproject(camera).sub(camera.position).normalize();
      return ray.multiplyScalar(6).add(camera.position).toArray() as Point;
    }
    case 'brand':return transform([0,1.9,0],[0,0.7,0],[0.05,-0.15+Math.sin(t*0.3)*0.06,-0.04]);
    case 'chip':return transform([0,1.125,0],[8.8,0.6,-0.4],[0.08,-0.15+(t-4)*0.09,-0.12]);
    case 'key':return [18-2.65+node.x*1.02,-1.58+0.25-Math.exp(-Math.pow((t-node.at)/0.12,2))*0.13,1.65+(node.x%2)*0.06];
    case 'server':return transform([0,0.96,0.25],[27.5,0.4,-0.4],[0,-0.18,0.03]);
    case 'folder-left':return transform([-0.6,1.07,0],[32.75,-0.4,0],[0,0.25,-0.06]);
    case 'folder-right':return transform([-0.6,1.07,0],[39.25,-0.3,-0.65],[0,-0.22,0.03]);
    case 'file':{
      const p=clamp((t-14.7-node.x*0.53)/4.7);
      const size=smooth(p/0.12)*(1-smooth((p-0.92)/0.13))*0.88;
      return transform([0,0.75*size,0],[36-3.2+6.5*p,-0.3+Math.sin(p*Math.PI)*2,0.45+Math.sin(p*Math.PI)*1.1],[0.08*Math.sin(p*6),-0.25+p*0.5,-0.3+p*0.6]);
    }
    case 'database':case 'database-edge':{
      const spread=smooth((t-24)/2)*(1-smooth((t-29.1)/2.2));
      return [node.kind==='database'?53.25:54.8,0.6+1.5*(0.56+spread*0.4)+0.19,0.3];
    }
    case 'workspace':return transform([0,2.57,0],[70,0.65,0],[0,-0.11,0.02]);
    case 'merge':return transform([0,1.9*0.62,0],[79,0.6,0],[0.05,Math.sin((t-46)*0.18)*0.06,-0.04]);
    case 'closing':return transform([0,1.9*0.74,0],[90,1.6,0],[0.05,Math.sin((t-46)*0.18)*0.06,-0.04]);
    default:return beamCurve.getPoint(clamp((node.x+8)/112)).toArray() as Point;
  }
}
export function mascotPose(t:number){
  let i=0;while(i<landings.length-1&&t>=landings[i+1].at)i++;
  const a=landings[i],b=landings[Math.min(i+1,landings.length-1)];
  const elapsed=t-a.at,duration=b.at-a.at;
  const hold=a.kind==='intro'?1.1:0.12;
  const u=duration?clamp((elapsed-hold)/(duration-hold)):0;
  const from=surface(a,t),to=surface(b,t),p=smooth(u);
  const arch=4*u*(1-u)*(a.kind==='key'&&b.kind==='key'?0.8:a.kind==='intro'&&b.kind==='brand'?0.55:1.65);
  const position=from.map((v,j)=>v+(to[j]-v)*p+(j===1?arch:0)) as Point;
  const squash=Math.exp(-Math.pow(elapsed/0.08,2))*0.36;
  const stretch=Math.sin(Math.PI*u)*0.13;
  return {position,scaleY:1-squash+stretch,scaleXZ:1/Math.sqrt(1-squash+stretch),u,elapsed,contact:from,index:i};
}
