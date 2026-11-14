// 功能：音乐驱动的三维物件叙事与连续空间镜头；时间：2026-11-14；作者：lq。
// 功能：统一产品片的午夜石墨、冰银、薄荷绿、信号青与暖琥珀主题；时间：2026-11-14；作者：lq。
export const PAPER = '#E9F1F2';
export const INK = '#101820';
export const MINT = '#58DDB5';
export const SIGNAL = '#55C6DB';
export const AMBER = '#F0B36D';
export const FONT = '-apple-system, BlinkMacSystemFont, "PingFang SC", sans-serif';
export const clamp = (v: number) => Math.min(1, Math.max(0, v));
export const smooth = (v: number) => {const p = clamp(v); return p*p*(3-2*p);};
export const mix = (a: number, b: number, p: number) => a+(b-a)*p;
export const reveal = (t: number, a: number, b: number) => smooth((t-a)/0.65)*(1-smooth((t-b+0.65)/0.65));
export const chapters = [
  {start:0,end:5,label:'QDS6'}, {start:5,end:15,label:'SSH'},
  {start:15,end:24,label:'SFTP'}, {start:24,end:33,label:'DATABASE'},
  {start:33,end:45,label:'WORKSPACE'}, {start:45,end:55.6,label:'QDS6'},
];

// 功能：相机和目标分别沿 Hermite 轨道运动，所有物件共处一个三维世界；时间：2026-11-14；作者：lq。
const cameraKeys = [
  // 时间、相机 xyz、观察目标 xyz、视场角、滚转。
  [0,1.0,1.6,5.6,0,0.65,0,40,-0.045],
  [2.8,4.8,3.1,8.7,0,0.4,0,38,0.015],
  [5.2,11.8,2.7,8.0,16.5,0.5,0,41,0.04],
  [8.0,17.0,4.0,10.0,18,0.55,0,39,-0.025],
  [10.8,20.2,2.4,9.2,18,0.7,0,37,-0.045],
  [13.3,24.8,1.9,7.1,25.6,0.5,-0.8,40,0.025],
  [16.0,32.3,3.6,10.5,35.0,0.55,0,40,0.025],
  [19.0,36.0,3.0,8.2,37.5,0.7,0,39,-0.035],
  [22.0,42.0,2.3,7.4,43,0.7,-0.4,41,-0.025],
  [25.3,49.7,3.3,9.5,53.8,0.6,0,40,0.04],
  [28.3,55.5,6.0,7.3,54,0.7,0,38,-0.035],
  [31.1,58.0,2.2,6.8,59.6,0.7,-0.8,40,-0.04],
  [34.3,65.4,2.2,10.5,69,0.7,0,40,0.02],
  [37.5,69.7,1.9,10.6,70,0.65,0,37,-0.015],
  [40.6,73.6,3.5,11.2,77,0.65,0,40,-0.025],
  [44.0,79.5,4.4,10.7,79,0.5,0,40,0.025],
  [47.0,86.6,2.7,10.8,90,0.65,0,39,0.025],
  [50.3,90.2,1.9,10.8,90,0.5,0,37,0],
  [53.3,90.9,1.6,12.0,90,0.4,0,37,0],
  [55.6,91.0,1.5,12.5,90,0.4,0,37,0],
];

export function cameraPath(t: number) {
  let i=0;
  while(i<cameraKeys.length-2&&t>cameraKeys[i+1][0]) i++;
  const a=cameraKeys[i],b=cameraKeys[i+1];
  const prev=cameraKeys[Math.max(0,i-1)],next=cameraKeys[Math.min(cameraKeys.length-1,i+2)];
  const dt=b[0]-a[0],p=clamp((t-a[0])/dt),p2=p*p,p3=p2*p;
  const v=a.map((value,j)=>{
    const m0=(b[j]-prev[j])/(b[0]-prev[0])*dt,m1=(next[j]-a[j])/(next[0]-a[0])*dt;
    return (2*p3-3*p2+1)*value+(p3-2*p2+p)*m0+(-2*p3+3*p2)*b[j]+(p3-p2)*m1;
  });
  return {eye:v.slice(1,4) as [number,number,number],target:v.slice(4,7) as [number,number,number],fov:v[7],roll:v[8]};
}
