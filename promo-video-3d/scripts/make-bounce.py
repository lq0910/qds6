# 功能：合成原创短促弹性落地音效，避免外部音效依赖；时间：2026-11-14；作者：lq。
import math, struct, wave
from pathlib import Path
rate=48000
phase=0.0
samples=[]
for i in range(int(rate*0.42)):
    t=i/rate
    frequency=150+430*math.exp(-t*24)+65*math.sin(t*65)*math.exp(-t*11)
    phase+=2*math.pi*frequency/rate
    envelope=(1-math.exp(-t*850))*math.exp(-t*13)*(1-t/0.42)
    value=(math.sin(phase)+0.2*math.sin(phase*2))*envelope*0.68
    samples.append(struct.pack('<h',int(max(-1,min(1,value))*32767)))
with wave.open(str(Path(__file__).resolve().parent.parent/'public/q-bounce.wav'),'wb') as output:
    output.setnchannels(1);output.setsampwidth(2);output.setframerate(rate);output.writeframes(b''.join(samples))
