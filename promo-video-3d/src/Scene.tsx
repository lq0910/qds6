// 功能：用实体变形、空间穿行和物件合流制作一镜到底的 QDS6 发布片；时间：2026-11-14；作者：lq。
import React, {useLayoutEffect, useMemo} from 'react';
import {useLoader, useThree} from '@react-three/fiber';
import {staticFile} from 'remotion';
import * as THREE from 'three';
import {RoomEnvironment} from 'three/examples/jsm/environments/RoomEnvironment.js';
import {Slab, type Vec} from './materials';
import {ConnectionBeam} from './ConnectionBeam';
import {QMascot} from './QMascot';
import {cameraPath, clamp, smooth, mix, MINT, PAPER, SIGNAL, AMBER} from './timeline';

// 功能：生成自有字面纹理，让字样贴在真正的三维实体上；时间：2026-11-14；作者：lq。
function Label({text, width=2, size=0.25, color='#DEDEDE', position=[0,0,0], rotation=[0,0,0], mono=false}: {text:string;width?:number;size?:number;color?:string;position?:Vec;rotation?:Vec;mono?:boolean}) {
  const texture=useMemo(()=>{
    const canvas=document.createElement('canvas');canvas.width=1024;canvas.height=Math.max(64,Math.min(1024,Math.round(1024*size/width)));
    const c=canvas.getContext('2d')!;
    c.fillStyle=color;c.font=`${mono?'500':'600'} ${Math.round(canvas.height*0.76)}px ${mono?'Menlo, monospace':'-apple-system, "PingFang SC", sans-serif'}`;
    c.textAlign='center';c.textBaseline='middle';c.fillText(text,512,canvas.height/2,1000);
    const map=new THREE.CanvasTexture(canvas);map.colorSpace=THREE.SRGBColorSpace;return map;
  },[text,color,mono,width,size]);
  useLayoutEffect(()=>()=>texture.dispose(),[texture]);
  return <mesh position={position} rotation={rotation}><planeGeometry args={[width,size]}/><meshBasicMaterial map={texture} transparent depthWrite={false} toneMapped={false}/></mesh>;
}

function Rig({time}:{time:number}) {
  const {gl,scene,camera}=useThree();
  const path=cameraPath(time);
  useLayoutEffect(()=>{
    const generator=new THREE.PMREMGenerator(gl),room=new RoomEnvironment();
    const env=generator.fromScene(room,0.04);scene.environment=env.texture;
    room.dispose();generator.dispose();return()=>{scene.environment=null;env.dispose();};
  },[gl,scene]);
  useLayoutEffect(()=>{
    camera.position.set(...path.eye);camera.lookAt(...path.target);camera.rotateZ(path.roll);
    const perspective=camera as THREE.PerspectiveCamera;perspective.fov=path.fov;perspective.updateProjectionMatrix();camera.updateMatrixWorld();
  },[camera,...path.eye,...path.target,path.fov,path.roll]);
  return <>
    <ambientLight intensity={0.65}/>
    <hemisphereLight args={['#D7F5F0','#26353F',1.25]}/>
    <directionalLight position={[path.target[0]-4,8,7]} intensity={3.1} color="#DDF7F4"/>
    <directionalLight position={[path.target[0]+5,3,-4]} intensity={3.0} color="#A9D9E8"/>
    <pointLight position={[path.target[0],-2,3]} intensity={7} distance={16} color="#F3C997"/>
  </>;
}

// 功能：品牌圆环从线条长成实体，连接尾笔保留薄荷绿；时间：2026-11-14；作者：lq。
function BrandCore({time,final=false,scale=1}:{time:number;final?:boolean;scale?:number}) {
  const build=final?1:smooth((time+0.15)/1.8);
  const spread=final?0:0.75*(1-smooth(time/3));
  const angle=final?Math.sin((time-46)*0.18)*0.06:-0.15+Math.sin(time*0.3)*0.06;
  return <group scale={scale} rotation={[0.05,angle,-0.04]}>
    <Slab size={[3.8,3.8,0.38]} color="#15232D" metalness={0.62}/>
    {[1,2].map(i=><group key={i} position={[0,0,-0.3-i*spread]}>
      <Slab size={[3.64,3.64,0.08]} color={i===1?'#3C6872':'#1C3038'} metalness={0.8}/>
    </group>)}
    <mesh position={[-0.08,0.09,0.3]} rotation={[0,0,Math.PI/2]}>
      <torusGeometry args={[1.06,0.21,24,96,Math.max(0.001,Math.PI*2*build)]}/>
      <meshStandardMaterial color={PAPER} metalness={0.3} roughness={0.19}/>
    </mesh>
    <group position={[0.76,-0.76,0.39]} rotation={[0,0,Math.PI/4]} scale={Math.max(0.001,smooth((build-0.45)/0.55))}>
      <mesh><capsuleGeometry args={[0.21,0.9,8,24]}/><meshStandardMaterial color={MINT} metalness={0.26} roughness={0.19}/></mesh>
      <mesh position={[0,-0.45,0.19]}><sphereGeometry args={[0.065,16,12]}/><meshStandardMaterial color="#171717" roughness={0.3}/></mesh>
    </group>
  </group>;
}

// 功能：机械键帽依次下压，再沿连续飞行轨迹离开终端区域；时间：2026-11-14；作者：lq。
function Keycap({text,position,index,time,wide=1}:{text:string;position:Vec;index:number;time:number;wide?:number}) {
  const strike=6.3+index*0.65;
  const press=Math.exp(-Math.pow((time-strike)/0.12,2))*0.13;
  const lift=smooth((time-10.5-index*0.11)/3.0);
  return <group position={[position[0]+lift*1.8,position[1]-press+lift*(1.7+index*0.08),position[2]-lift*3]} rotation={[-Math.PI/2+lift*0.35,lift*0.15,lift*0.1]}>
    <Slab size={[0.85*wide,0.83,0.34]} color={index%4===0?'#1D1D1D':'#DCDCDC'} metalness={0.28}/>
    <Slab size={[0.69*wide,0.67,0.06]} position={[0,0,0.19]} color={index%4===0?'#292929':'#F3F3F3'} metalness={0.18}/>
    <Label text={text} width={0.45*wide} size={0.26} color={index%4===0?'#F4F4F4':'#252525'} position={[0,0,0.23]} mono/>
  </group>;
}

// 功能：使用用户提供的真实截图作为三维窗口纹理，按原图比例保留完整界面；时间：2026-11-14；作者：lq。
function Window({time,kind='terminal',width=7.7}:{time:number;kind?:'terminal'|'database';width?:number}) {
  const map=useLoader(THREE.TextureLoader,staticFile(kind==='database'?'qds6-real-database.png':'qds6-real-server.png'));
  map.colorSpace=THREE.SRGBColorSpace;map.anisotropy=8;
  const height=width*map.image.height/map.image.width;
  const unfold=smooth((time-5.5)/3);
  return <group rotation={[-0.14*(1-unfold),-0.18,0.015]}>
    <Slab size={[width+0.12,height+0.12,0.18]} color="#91AAB0" metalness={0.78}/>
    <Slab size={[width+0.035,height+0.035,0.2]} color="#101820" metalness={0.35}/>
    <mesh position={[0,0,0.108]}><planeGeometry args={[width,height]}/><meshBasicMaterial map={map} transparent toneMapped={false}/></mesh>
  </group>;
}

// 功能：终端窗口拆成两个有真实空间间隔的会话，键帽和命令字符在镜头前景移动；时间：2026-11-14；作者：lq。
function TerminalWorld({time}:{time:number}) {
  const split=smooth((time-9)/2.5),depart=smooth((time-12.6)/2);
  return <group position={[18,0,0]}>
    <group position={[-split*0.9,0.95, -0.9-depart*0.5]} rotation={[0,-0.08-split*0.06,split*0.025]}>
      <Window time={time} width={6.7}/>
    </group>
    <group position={[2.1+split*0.3,0.45, -1.9+split*0.75]} rotation={[0,-0.25,0.025]} scale={0.72*split}>
      <Window time={time} width={4.7} kind="database"/>
    </group>
    {['S','S','H','↵','>','_'].map((s,i)=><Keycap key={i} text={s} index={i} time={time} position={[-2.65+i*1.02,-1.58,1.65+(i%2)*0.06]} wide={s==='↵'?1.22:1}/>)}
    <group position={[-3.8,0.65,1.4]} rotation={[0,0.1,-0.03]}>
      <Slab size={[1.2,1.12,0.3]} color="#EEEEEE" metalness={0.5}/>
      <Label text=">_" width={0.8} size={0.51} color="#151515" position={[0,0,0.17]} mono/>
    </group>
  </group>;
}

// 功能：文件实体包含折角、代码行及薄边，在服务器目录间按弧线传递；时间：2026-11-14；作者：lq。
function Document({position,rotation=[0,0,0],scale=1,name='config.toml',dark=false}:{position:Vec;rotation?:Vec;scale?:number;name?:string;dark?:boolean}) {
  return <group position={position} rotation={rotation} scale={scale}>
    <Slab size={[1.18,1.5,0.065]} color={dark?'#242424':'#F1F1F1'} metalness={0.14}/>
    <mesh position={[0.32,0.5,0.054]} rotation={[0,0,-Math.PI/4]}><planeGeometry args={[0.24,0.24]}/><meshBasicMaterial color={dark?'#555555':'#AFAFAF'} side={THREE.DoubleSide}/></mesh>
    {[0,1,2,3].map(i=><mesh key={i} position={[-0.04,0.16-i*0.16,0.045]}><boxGeometry args={[i===3?0.46:0.78,0.027,0.008]}/><meshBasicMaterial color={dark?'#AFAFAF':'#666666'}/></mesh>)}
    <Label text={name} width={0.95} size={0.12} color={dark?'#DDDDDD':'#333333'} position={[0,-0.58,0.049]} mono/>
  </group>;
}

function Folder({position,rotation=[0,0,0],dark=false}:{position:Vec;rotation?:Vec;dark?:boolean}) {
  return <group position={position} rotation={rotation}>
    <Slab size={[2.3,1.65,0.18]} color={dark?'#252525':'#ECECEC'} metalness={0.35}/>
    <Slab size={[1.0,0.38,0.18]} color={dark?'#252525':'#ECECEC'} position={[-0.6,0.88,0]}/>
    <Slab size={[2.43,1.48,0.15]} color={dark?'#3B3B3B':'#BFBFBF'} position={[0,-0.2,0.43]} metalness={0.52}/>
    <Label text={dark?'/workspace':'~/Projects'} width={1.55} size={0.21} color={dark?'#DDDDDD':'#333333'} position={[0,-0.25,0.52]} mono/>
  </group>;
}

function FileWorld({time}:{time:number}) {
  const names=['app.toml','schema.sql','README.md','config.json','deploy.sh'];
  return <group position={[36,0,0]}>
    <Folder position={[-3.25,-0.4,0]} rotation={[0,0.25,-0.06]}/>
    <Folder position={[3.25,-0.3,-0.65]} rotation={[0,-0.22,0.03]} dark/>
    {Array.from({length:8},(_,i)=>{
      const raw=(time-14.7-i*0.53)/4.7;
      const p=clamp(raw);
      const x=mix(-3.2,3.3,p),y=-0.3+Math.sin(p*Math.PI)*2.0,z=0.45+Math.sin(p*Math.PI)*1.1;
      const size=Math.max(0.001,smooth(raw/0.12)*(1-smooth((raw-0.92)/0.13)));
      return <Document key={i} position={[x,y,z]} rotation={[0.08*Math.sin(p*6),-0.25+p*0.5,-0.3+p*0.6]} scale={size*0.88} name={names[i%names.length]} dark={i%3===1}/>;
    })}
    <Label text="LOCAL" width={1.7} size={0.22} position={[-3.2,-1.68,0.7]} color="#AFAFAF"/>
    <Label text="REMOTE" width={1.8} size={0.22} position={[3.2,-1.65,0.1]} color="#AFAFAF"/>
  </group>;
}

// 功能：文件沿螺旋过渡为数据单元，数据库层展开后再聚合；时间：2026-11-14；作者：lq。
function DatabaseWorld({time}:{time:number}) {
  const explode=smooth((time-24)/2)*(1-smooth((time-29.1)/2.2));
  const twist=(time-24)*0.24;
  return <group position={[54,0.6,0]} rotation={[0,-0.12,0]}>
    {[0,1,2,3].map(i=><group key={i} position={[0,(i-1.5)*(0.56+explode*0.4),0]} rotation={[0,twist*(i%2?1:-1)*0.16,0]}>
      <mesh><cylinderGeometry args={[1.75,1.75,0.38,96]}/><meshStandardMaterial color={['#D8E5E4','#37616B','#B9CBD1','#3D5664'][i]} metalness={0.58} roughness={0.24}/></mesh>
      <mesh position={[0,0.193,0]} rotation={[Math.PI/2,0,0]}><torusGeometry args={[1.64,0.016,8,96]}/><meshBasicMaterial color={i===1?SIGNAL:'#F3F6F2'}/></mesh>
      {[0,1,2].map(j=><mesh key={j} position={[-0.36+j*0.18,0,1.75]}><sphereGeometry args={[0.037,10,8]}/><meshBasicMaterial color={j===0&&i===3?MINT:j===1&&i===1?AMBER:'#AFC2C8'}/></mesh>)}
      {/* 功能：数据库盘面保持完整平滑，不添加方块装饰；时间：2026-11-14；作者：lq。 */}
    </group>)}
    <group position={[-2.75,0.2,1.1]} rotation={[0,0.12,-0.07]}>
      <Document position={[0,0,0]} rotation={[0,0.1,0]} name="query.sql" scale={1.18}/>
    </group>
    <Label text="MySQL  /  PostgreSQL" width={3.7} size={0.25} position={[0,-2.22,1]} mono color="#C4C4C4"/>
  </group>;
}

// 功能：经过前景实体时，数据卡片组成开放环形通道，镜头继续驶向原生工作台；时间：2026-11-14；作者：lq。
function DataTunnel({time}:{time:number}) {
  return <group position={[61,0.7,-0.6]} rotation={[0,Math.PI/2,0]}>
    {Array.from({length:10},(_,i)=>{
      const a=i/10*Math.PI*2+(time-30)*0.14;
      return <Document key={i} position={[Math.cos(a)*2.65,Math.sin(a)*2.65,0]} rotation={[0,0,a-Math.PI/2]} scale={0.55} name={i%2?'ROW':'SQL'} dark={i%3===0}/>;
    })}
  </group>;
}

function NativeWorkspace({time}:{time:number}) {
  const map=useLoader(THREE.TextureLoader,staticFile('workspace-mono-source.png'));map.colorSpace=THREE.SRGBColorSpace;map.anisotropy=8;
  const unfold=smooth((time-32)/3.4);
  return <group position={[70,0.65,0]} rotation={[0,-0.11,0.02]}>
    <group rotation={[0,0.9*(1-unfold),0]}>
      <Slab size={[8.12,5.14,0.22]} color="#91AAB0" metalness={0.72}/>
      <Slab size={[8.04,5.06,0.235]} color="#15252F"/>
      <mesh position={[0,0,0.13]}><planeGeometry args={[7.96,4.975]}/><meshBasicMaterial map={map} toneMapped={false}/></mesh>
    </group>
    <Label text="QDS6  /  ONE WORKSPACE" width={3.8} size={0.23} position={[0,-3.0,0.1]} color="#A0A0A0"/>
  </group>;
}

// 功能：三个服务实体沿轨道向同一品牌核心合拢，空间运动体现统一工作台；时间：2026-11-14；作者：lq。
function Convergence({time}:{time:number}) {
  const contraction=smooth((time-41.2)/4.5),radius=mix(3.3,0.72,contraction);
  const angle=(time-39)*0.36;
  const positions=[0,1,2].map(i=>{
    const a=i/3*Math.PI*2+angle;return [Math.cos(a)*radius,Math.sin(a)*radius*0.67,Math.sin(a)*0.4] as Vec;
  });
  const shrink=Math.max(0.001,1-smooth((time-44.2)/1.8));
  return <group position={[79,0.6,0]}>
    <BrandCore time={time} final scale={0.62}/>
    <group position={positions[0]} scale={shrink} rotation={[0,0.1,-0.05]}>
      <Slab size={[1.3,1.3,0.22]} color="#E5E5E5"/><Label text=">_" width={0.9} size={0.5} position={[0,0,0.13]} color="#222222" mono/>
    </group>
    <group position={positions[1]} scale={shrink*0.65}><Folder position={[0,0,0]}/></group>
    <group position={positions[2]} scale={shrink*0.75}>
      {[0,1,2].map(i=><mesh key={i} position={[0,i*0.32-0.32,0]}><cylinderGeometry args={[0.72,0.72,0.25,48]}/><meshStandardMaterial color={i%2?'#444444':'#CCCCCC'} metalness={0.65} roughness={0.25}/></mesh>)}
    </group>
  </group>;
}

// 功能：最终品牌字样使用真实挤出几何体，继承同一场景灯光与镜头；时间：2026-11-14；作者：lq。
function Closing({time}:{time:number}) {
  return <group position={[90,0.8,0]}>
    <group position={[0,0.8,0]}><BrandCore time={time} final scale={0.74}/></group>
    <Label text="QDS6" width={2.5} size={0.55} position={[0,-2.12,0.25]} color="#F3F3F3"/>
  </group>;
}

// 功能：在 SSH 与文件流之间放置真实厚度的服务器模型，承接镜头推进；时间：2026-11-14；作者：lq。
function ServerBridge({time}:{time:number}) {
  return <group position={[27.5,0.4,-0.4]} rotation={[0,-0.18,0.03]}>
    {[0,1,2].map(i=><group key={i} position={[0,(i-1)*0.68,0]}>
      <Slab size={[3.0,0.56,1.25]} color={i===1?'#6A6A6A':'#2D2D2D'} metalness={0.7}/>
      <Slab size={[2.85,0.43,0.035]} color="#161616" position={[0,0,0.655]} metalness={0.4}/>
      {Array.from({length:12},(_,j)=><mesh key={j} position={[-1.03+j*0.14,0,0.681]}><boxGeometry args={[0.035,0.21,0.012]}/><meshStandardMaterial color="#B4B4B4" metalness={0.6} roughness={0.3}/></mesh>)}
      <mesh position={[1.16,0,0.68]}><sphereGeometry args={[0.045,12,8]}/><meshBasicMaterial color={Math.sin(time*4+i)>0?'#FFFFFF':'#5F5F5F'}/></mesh>
    </group>)}
    <Label text="SSH  /  CONNECTED" width={2.0} size={0.19} position={[0,-1.35,0.7]} mono color="#D8D8D8"/>
  </group>;
}

// 功能：以原生核心的立体芯片衔接品牌和终端，承载 Rust 与 GPUI 卖点；时间：2026-11-14；作者：lq。
function NativeCore({time}:{time:number}) {
  return <group position={[8.8,0.6,-0.4]} rotation={[0.08,-0.15+(time-4)*0.09,-0.12]}>
    <Slab size={[2.7,2.25,0.32]} color="#BFBFBF" metalness={0.75}/>
    <Slab size={[2.3,1.9,0.12]} color="#1B1B1B" position={[0,0,0.23]} metalness={0.6}/>
    <Label text="Rust" width={1.7} size={0.47} position={[0,0.25,0.31]} color="#F3F3F3"/>
    <Label text="GPUI" width={1.4} size={0.28} position={[0,-0.36,0.31]} color="#BBBBBB" mono/>
    {Array.from({length:7},(_,i)=><React.Fragment key={i}>
      <mesh position={[-1.49,-0.83+i*0.28,0]}><boxGeometry args={[0.31,0.09,0.1]}/><meshStandardMaterial color="#D0D0D0" metalness={0.8} roughness={0.22}/></mesh>
      <mesh position={[1.49,-0.83+i*0.28,0]}><boxGeometry args={[0.31,0.09,0.1]}/><meshStandardMaterial color="#D0D0D0" metalness={0.8} roughness={0.22}/></mesh>
    </React.Fragment>)}
  </group>;
}

// 功能：文件在场景空隙中转为结构化数据，镜头跟随这条立体螺旋继续前进；时间：2026-11-14；作者：lq。
function FileToData({time}:{time:number}) {
  return <group position={[45.8,0.75,0]}>
    {Array.from({length:7},(_,i)=>{
      const a=i*0.8+(time-21)*0.65;
      const x=(i-3)*0.67,y=Math.sin(a)*0.85,z=Math.cos(a)*0.7;
      return <Document key={i} position={[x,y,z]} rotation={[0.08,0.18,Math.sin(a)*0.12]} scale={0.48} name={i%2?'schema.sql':'ROW'} dark={i%3===0}/>;
    })}
  </group>;
}

export function Scene({time}:{time:number}) {
  const eye=cameraPath(time).eye[0];
  return <>
    <color attach="background" args={['#1C2A32']}/><fog attach="fog" args={['#1C2A32',24,62]}/>
    <Rig time={time}/><ConnectionBeam time={time}/><QMascot time={time}/>
    <group position={[0,0.7,0]} visible={eye<23}><BrandCore time={time}/></group>
    <group visible={Math.abs(eye-8.8)<17}><NativeCore time={time}/></group>
    <group visible={Math.abs(eye-18)<23}><TerminalWorld time={time}/></group>
    <group visible={Math.abs(eye-27.5)<17}><ServerBridge time={time}/></group>
    <group visible={Math.abs(eye-36)<24}><FileWorld time={time}/></group>
    <group visible={Math.abs(eye-45.8)<17}><FileToData time={time}/></group>
    <group visible={Math.abs(eye-54)<24}><DatabaseWorld time={time}/></group>
    <group visible={Math.abs(eye-61)<17}><DataTunnel time={time}/></group>
    <group visible={Math.abs(eye-70)<20}><NativeWorkspace time={time}/></group>
    <group visible={Math.abs(eye-79)<17}><Convergence time={time}/></group>
    <group visible={eye>67&&time<50.15}><Closing time={time}/></group>
  </>;
}
