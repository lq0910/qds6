// 功能：制作 QDS6 产品宣传片章节与画面动画；时间：2026-11-14；作者：lq。
import React from "react";
import {
  AbsoluteFill, Img, interpolate, spring,
  useCurrentFrame, useVideoConfig, staticFile,
} from "remotion";

// ══════════════════════════════════════════════════
// HELPERS
// ══════════════════════════════════════════════════

function calcTextAnim(localF: number, fps: number, delay = 0) {
  const s = spring({ frame: localF - delay, fps,
    config: { damping: 18, stiffness: 85, mass: 1 } });
  return {
    opacity: interpolate(s, [0, 1], [0, 1]),
    y: interpolate(s, [0, 1], [44, 0]),
  };
}

// ══════════════════════════════════════════════════
// PARTICLES  (deterministic, frame-driven)
// ══════════════════════════════════════════════════

const P_DATA = Array.from({ length: 75 }, (_, i) => ({
  x: (i * 137.508) % 100,
  startY: (i * 71.3) % 110,
  size: 0.9 + ((i * 11) % 3) * 0.65,
  speed: 0.14 + ((i * 7) % 5) * 0.07,
  base: 0.08 + ((i * 13) % 5) * 0.07,
  phase: (i * 2.3) % (Math.PI * 2),
}));

function Particles({ frame, fps }: { frame: number; fps: number }) {
  return (
    <AbsoluteFill style={{ pointerEvents: "none" }}>
      {P_DATA.map((p, i) => {
        const y = ((p.startY - frame * p.speed * 0.4) % 110 + 110) % 110;
        const tw = 0.5 + 0.5 * Math.sin((frame / fps) * p.speed * 2.5 * Math.PI + p.phase);
        const a = p.base * tw;
        return (
          <div key={i} style={{
            position: "absolute", left: `${p.x}%`, top: `${y}%`,
            width: p.size, height: p.size, borderRadius: "50%",
            background: `rgba(90,170,255,${a})`,
            boxShadow: `0 0 ${p.size * 2}px rgba(90,170,255,${a * 0.4})`,
          }} />
        );
      })}
    </AbsoluteFill>
  );
}

// ══════════════════════════════════════════════════
// SHARED UI ATOMS
// ══════════════════════════════════════════════════

const FONT = '-apple-system,"SF Pro Display","PingFang SC",sans-serif';

function Vignette() {
  return (
    <AbsoluteFill style={{
      background: "radial-gradient(ellipse at center,transparent 42%,rgba(0,0,0,0.78) 100%)",
      pointerEvents: "none",
    }} />
  );
}

function Scanlines() {
  return (
    <AbsoluteFill style={{
      backgroundImage: "repeating-linear-gradient(0deg,transparent,transparent 2px,rgba(0,0,0,0.035) 2px,rgba(0,0,0,0.035) 4px)",
      pointerEvents: "none",
    }} />
  );
}

function Letterbox() {
  const bar: React.CSSProperties = {
    position: "absolute", left: 0, right: 0, height: 72,
    background: "#000", zIndex: 999, pointerEvents: "none",
  };
  return (
    <>
      <div style={{ ...bar, top: 0 }} />
      <div style={{ ...bar, bottom: 0 }} />
    </>
  );
}

function AccentLine({ localF, fps, delay = 0 }: { localF: number; fps: number; delay?: number }) {
  const s = spring({ frame: localF - delay, fps, config: { damping: 22, stiffness: 50 } });
  const w = interpolate(s, [0, 1], [0, 440]);
  return (
    <div style={{
      height: 2, width: w,
      background: "linear-gradient(90deg,transparent,#38bdf8,transparent)",
      margin: "26px 0 0 0",
      boxShadow: "0 0 14px rgba(56,189,248,0.55)",
    }} />
  );
}

// ══════════════════════════════════════════════════
// SCENE 1 — INTRO
// ══════════════════════════════════════════════════

function IntroScene({ localF, fps }: { localF: number; fps: number }) {
  const logoS = spring({ frame: localF - 5, fps,
    config: { damping: 11, stiffness: 60, mass: 0.9 } });
  const logoScale  = interpolate(logoS, [0, 1], [0.3, 1]);
  const logoOpacity = interpolate(logoS, [0, 1], [0, 1]);
  const logoGlow   = interpolate(logoS, [0, 1], [0, 1]);

  const lineS = spring({ frame: localF - 14, fps, config: { damping: 25, stiffness: 48 } });
  const lineW = interpolate(lineS, [0, 1], [0, 210]);

  const tag   = calcTextAnim(localF, fps, 26);
  const sub   = calcTextAnim(localF, fps, 56);
  const badge = calcTextAnim(localF, fps, 82);

  return (
    <AbsoluteFill style={{ background: "#000", alignItems: "center", justifyContent: "center" }}>
      <Particles frame={localF} fps={fps} />
      <div style={{ textAlign: "center", position: "relative", zIndex: 10 }}>
        {/* Logo */}
        <div style={{
          width: 112, height: 112, borderRadius: 26, margin: "0 auto 36px",
          background: "linear-gradient(135deg,#1a2f50,#0d1f3d,#060e22)",
          border: "1.5px solid rgba(56,189,248,0.35)",
          boxShadow: `0 0 ${60*logoGlow}px rgba(56,189,248,0.45),0 0 ${120*logoGlow}px rgba(56,189,248,0.15),inset 0 1px 0 rgba(255,255,255,0.08)`,
          display: "flex", alignItems: "center", justifyContent: "center",
          transform: `scale(${logoScale})`, opacity: logoOpacity,
        }}>
          <span style={{ fontSize: 58, fontWeight: 900, color: "#fff", fontFamily: FONT, letterSpacing: "-0.04em", lineHeight: 1 }}>Q</span>
        </div>
        {/* Thin line */}
        <div style={{ height: 1, width: lineW, background: "linear-gradient(90deg,transparent,rgba(56,189,248,0.5),transparent)", margin: "0 auto 36px" }} />
        {/* Tagline */}
        <div style={{
          fontSize: 62, fontWeight: 700, color: "#fff", fontFamily: FONT,
          letterSpacing: "-0.025em", lineHeight: 1.2,
          textShadow: "0 0 100px rgba(56,189,248,0.3)",
          opacity: tag.opacity, transform: `translateY(${tag.y}px)`,
        }}>数据，是现代世界的脉搏</div>
        {/* Sub */}
        <div style={{
          fontSize: 19, fontWeight: 300, color: "rgba(255,255,255,0.45)", marginTop: 22,
          letterSpacing: "0.22em", fontFamily: FONT,
          opacity: sub.opacity, transform: `translateY(${sub.y}px)`,
        }}>DATABASE WORKBENCH</div>
        {/* Badge */}
        <div style={{
          display: "inline-block", marginTop: 32, padding: "7px 22px", borderRadius: 100,
          border: "1px solid rgba(56,189,248,0.25)", background: "rgba(56,189,248,0.07)",
          color: "rgba(144,210,255,0.8)", fontSize: 13, letterSpacing: "0.12em", fontFamily: FONT,
          opacity: badge.opacity, transform: `translateY(${badge.y}px)`,
        }}>QDS6</div>
      </div>
      <Scanlines />
    </AbsoluteFill>
  );
}

// ══════════════════════════════════════════════════
// SCENE 2 — BRAND REVEAL
// ══════════════════════════════════════════════════

const BRAND_PILLS = ["MySQL", "PostgreSQL", "SSH 终端", "SFTP 传输", "系统监控"];

function BrandScene({ localF, fps }: { localF: number; fps: number }) {
  const qs = spring({ frame: localF - 5, fps,
    config: { damping: 13, stiffness: 68, mass: 1.1 } });
  const qScale   = interpolate(qs, [0, 1], [0.52, 1]);
  const qOpacity = interpolate(qs, [0, 1], [0, 1]);
  const qGlow    = interpolate(qs, [0, 1], [0, 1]);
  const sub      = calcTextAnim(localF, fps, 38);

  return (
    <AbsoluteFill style={{
      background: "radial-gradient(ellipse at 50% 55%,#040c1e,#000 72%)",
      alignItems: "center", justifyContent: "center",
    }}>
      <Particles frame={localF} fps={fps} />
      {/* Ambient glow */}
      <div style={{
        position: "absolute", width: 800, height: 800, borderRadius: "50%",
        background: "radial-gradient(circle,rgba(29,78,216,0.13) 0%,transparent 70%)",
        top: "50%", left: "50%", transform: "translate(-50%,-50%)", pointerEvents: "none",
      }} />
      <div style={{ textAlign: "center", position: "relative", zIndex: 10 }}>
        {/* QDS6 */}
        <div style={{
          fontSize: 180, fontWeight: 900, fontFamily: FONT,
          letterSpacing: "-0.045em", lineHeight: 0.88,
          background: `linear-gradient(180deg,#fff 20%,rgba(56,189,248,${0.5+qGlow*0.5}) 100%)`,
          WebkitBackgroundClip: "text", WebkitTextFillColor: "transparent",
          transform: `scale(${qScale})`, opacity: qOpacity,
          filter: `drop-shadow(0 0 ${80*qGlow}px rgba(56,189,248,0.5))`,
        }}>QDS6</div>
        {/* Sub */}
        <div style={{
          fontSize: 24, fontWeight: 300, color: "rgba(255,255,255,0.6)", marginTop: 28,
          letterSpacing: "0.1em", fontFamily: FONT,
          opacity: sub.opacity, transform: `translateY(${sub.y}px)`,
        }}>专为开发者打造的数据库工作区</div>
        <AccentLine localF={localF} fps={fps} delay={48} />
        {/* Pills */}
        <div style={{ display: "flex", gap: 12, justifyContent: "center", marginTop: 36, flexWrap: "wrap" }}>
          {BRAND_PILLS.map((pill, i) => {
            const ps = spring({ frame: localF - 72 - i * 12, fps, config: { damping: 20, stiffness: 100 } });
            return (
              <div key={pill} style={{
                padding: "9px 22px", borderRadius: 100,
                border: "1px solid rgba(56,189,248,0.3)", background: "rgba(56,189,248,0.07)",
                color: "rgba(186,230,255,0.9)", fontSize: 14, letterSpacing: "0.07em", fontFamily: FONT,
                opacity: interpolate(ps, [0, 1], [0, 1]),
                transform: `translateY(${interpolate(ps, [0, 1], [20, 0])}px)`,
              }}>{pill}</div>
            );
          })}
        </div>
      </div>
      <Scanlines />
    </AbsoluteFill>
  );
}

// ══════════════════════════════════════════════════
// SCREEN SCENE  (scenes 3–6, reusable)
// ══════════════════════════════════════════════════

interface ScreenProps {
  localF: number; fps: number; imgSrc: string;
  eyebrow: string; titleLines: string[]; subtitle: string;
  pills?: string[];
}

function ScreenScene({ localF, fps, imgSrc, eyebrow, titleLines, subtitle, pills }: ScreenProps) {
  // Ken Burns
  const kbF = Math.min(localF, 260);
  const imgScale = interpolate(kbF, [0, 260], [1.04, 1.14]);
  const imgTX    = interpolate(kbF, [0, 260], [0, -1.5]);
  const imgTY    = interpolate(kbF, [0, 260], [0, -0.8]);

  // Screen preview card
  const preS = spring({ frame: localF - 10, fps, config: { damping: 20, stiffness: 52, mass: 1 } });
  const preY  = interpolate(preS, [0, 1], [110, 0]);
  const preO  = interpolate(preS, [0, 1], [0, 1]);
  const preSc = interpolate(preS, [0, 1], [0.92, 1]);

  // Text
  const eye = calcTextAnim(localF, fps, 20);
  const t0  = calcTextAnim(localF, fps, 34);
  const t1  = calcTextAnim(localF, fps, 48);
  const sub = calcTextAnim(localF, fps, 62);

  return (
    <AbsoluteFill style={{ background: "#000" }}>
      {/* BG with Ken Burns */}
      <AbsoluteFill style={{ overflow: "hidden" }}>
        <div style={{
          position: "absolute", inset: 0,
          transform: `scale(${imgScale}) translate(${imgTX}%,${imgTY}%)`,
          transformOrigin: "center center",
        }}>
          <Img src={imgSrc} style={{
            width: "100%", height: "100%", objectFit: "cover", objectPosition: "top center",
            filter: "brightness(0.26) saturate(1.4)",
          }} />
        </div>
        <AbsoluteFill style={{ background: "linear-gradient(105deg,rgba(0,4,15,0.85),rgba(0,6,22,0.4) 55%,rgba(0,0,0,0.75))" }} />
      </AbsoluteFill>
      <Vignette />

      {/* Screenshot preview card — bottom right */}
      <div style={{
        position: "absolute", bottom: 110, right: 120,
        width: 570, height: 356,
        borderRadius: 14, overflow: "hidden",
        boxShadow: "0 32px 100px rgba(0,0,0,0.85),0 0 0 1px rgba(255,255,255,0.09)",
        transform: `translateY(${preY}px) scale(${preSc})`,
        opacity: preO,
      }}>
        <Img src={imgSrc} style={{ width: "100%", height: "100%", objectFit: "cover", objectPosition: "top left" }} />
        {/* Glare */}
        <div style={{
          position: "absolute", inset: 0,
          background: "linear-gradient(135deg,rgba(255,255,255,0.07) 0%,transparent 45%)",
        }} />
      </div>

      {/* Text — left side */}
      <div style={{
        position: "absolute", left: 130, top: "50%",
        transform: "translateY(-50%)", maxWidth: 710, zIndex: 10,
      }}>
        {/* Eyebrow */}
        <div style={{
          fontSize: 13, letterSpacing: "0.42em", textTransform: "uppercase",
          color: "rgba(56,189,248,0.8)", marginBottom: 20, fontFamily: FONT,
          opacity: eye.opacity, transform: `translateY(${eye.y}px)`,
        }}>{eyebrow}</div>

        {/* Title lines */}
        {titleLines.map((line, i) => {
          const anim = i === 0 ? t0 : t1;
          return (
            <div key={i} style={{
              fontSize: 80, fontWeight: 800, color: "#fff",
              lineHeight: 1.08, letterSpacing: "-0.03em", fontFamily: FONT,
              textShadow: "0 0 80px rgba(56,189,248,0.2)",
              opacity: anim.opacity, transform: `translateY(${anim.y}px)`,
            }}>{line}</div>
          );
        })}

        {/* Subtitle */}
        <div style={{
          fontSize: 20, fontWeight: 300, color: "rgba(255,255,255,0.58)", marginTop: 22,
          lineHeight: 1.75, letterSpacing: "0.03em", fontFamily: FONT,
          opacity: sub.opacity, transform: `translateY(${sub.y}px)`,
        }}>{subtitle}</div>

        <AccentLine localF={localF} fps={fps} delay={72} />

        {/* Pills */}
        {pills && (
          <div style={{ display: "flex", gap: 10, marginTop: 28, flexWrap: "wrap" }}>
            {pills.map((pill, i) => {
              const ps = spring({ frame: localF - 96 - i * 12, fps, config: { damping: 20, stiffness: 100 } });
              return (
                <div key={pill} style={{
                  padding: "7px 18px", borderRadius: 100,
                  border: "1px solid rgba(56,189,248,0.25)", background: "rgba(56,189,248,0.07)",
                  backdropFilter: "blur(8px)", color: "rgba(186,230,255,0.85)",
                  fontSize: 13, letterSpacing: "0.06em", fontFamily: FONT,
                  opacity: interpolate(ps, [0, 1], [0, 1]),
                  transform: `translateY(${interpolate(ps, [0, 1], [15, 0])}px)`,
                }}>{pill}</div>
              );
            })}
          </div>
        )}
      </div>
      <Scanlines />
    </AbsoluteFill>
  );
}

// ══════════════════════════════════════════════════
// SCENE 7 — OUTRO
// ══════════════════════════════════════════════════

function OutroScene({ localF, fps }: { localF: number; fps: number }) {
  const title = calcTextAnim(localF, fps, 20);
  const sub   = calcTextAnim(localF, fps, 55);
  const ctaS  = spring({ frame: localF - 88, fps, config: { damping: 14, stiffness: 72, mass: 1 } });
  const ctaO  = interpolate(ctaS, [0, 1], [0, 1]);
  const ctaY  = interpolate(ctaS, [0, 1], [30, 0]);
  const ctaG  = interpolate(ctaS, [0, 1], [0, 1]);

  const rings = [0.42, 0.78, 1.14, 1.55].map((scale, i) => {
    const phase = ((localF / fps) * 0.5 + i * 0.65) % 1;
    return { scale, opacity: 0.07 + 0.22 * Math.sin(phase * Math.PI) };
  });

  return (
    <AbsoluteFill style={{
      background: "radial-gradient(ellipse at 50% 60%,#030a1e,#000 65%)",
      alignItems: "center", justifyContent: "center",
    }}>
      <Particles frame={localF} fps={fps} />

      {/* Glow rings */}
      {rings.map((r, i) => (
        <div key={i} style={{
          position: "absolute", width: 700, height: 700, borderRadius: "50%",
          border: "1px solid rgba(56,189,248,0.5)", opacity: r.opacity,
          top: "50%", left: "50%",
          transform: `translate(-50%,-50%) scale(${r.scale})`,
          pointerEvents: "none",
        }} />
      ))}

      {/* Center glow */}
      <div style={{
        position: "absolute", width: 500, height: 500, borderRadius: "50%",
        background: "radial-gradient(circle,rgba(29,78,216,0.2) 0%,transparent 65%)",
        top: "50%", left: "50%", transform: "translate(-50%,-50%)", pointerEvents: "none",
      }} />

      <div style={{ textAlign: "center", position: "relative", zIndex: 10 }}>
        {/* Headline */}
        <div style={{
          fontSize: 96, fontWeight: 800, lineHeight: 1.1,
          letterSpacing: "-0.035em", fontFamily: FONT,
          background: "linear-gradient(180deg,#fff 25%,rgba(56,189,248,0.95) 100%)",
          WebkitBackgroundClip: "text", WebkitTextFillColor: "transparent",
          filter: "drop-shadow(0 0 50px rgba(56,189,248,0.35))",
          opacity: title.opacity, transform: `translateY(${title.y}px)`,
        }}>
          让数据工作<br />更简单，更优雅
        </div>

        {/* Product label */}
        <div style={{
          fontSize: 17, letterSpacing: "0.32em", color: "rgba(255,255,255,0.35)",
          marginTop: 28, textTransform: "uppercase", fontFamily: FONT,
          opacity: sub.opacity, transform: `translateY(${sub.y}px)`,
        }}>QDS6 · Database Workbench</div>

        <AccentLine localF={localF} fps={fps} delay={58} />

        {/* CTA button */}
        <div style={{
          display: "inline-block", marginTop: 52, padding: "17px 62px",
          borderRadius: 100,
          background: "linear-gradient(135deg,#1d4ed8,#3b82f6)",
          color: "#fff", fontSize: 18, fontWeight: 600, letterSpacing: "0.09em", fontFamily: FONT,
          border: "1px solid rgba(255,255,255,0.18)",
          boxShadow: `0 0 ${40*ctaG}px rgba(59,130,246,0.65),0 0 ${80*ctaG}px rgba(59,130,246,0.25),0 8px 32px rgba(0,0,0,0.5)`,
          opacity: ctaO, transform: `translateY(${ctaY}px)`,
        }}>立即体验 QDS6</div>
      </div>
      <Scanlines />
    </AbsoluteFill>
  );
}

// ══════════════════════════════════════════════════
// MAIN COMPOSITION
// Total: 1500 frames @ 30 fps = 50 s
// Scenes overlap 25 frames for crossfade
// ══════════════════════════════════════════════════

const SCENES_DEF = [
  { from: 0,    dur: 190 },  // Intro
  { from: 165,  dur: 215 },  // Brand        ends 380
  { from: 355,  dur: 255 },  // Screen 1     ends 610
  { from: 580,  dur: 265 },  // Screen 2     ends 845
  { from: 820,  dur: 275 },  // Screen 3     ends 1095
  { from: 1070, dur: 240 },  // Screen 4     ends 1310
  { from: 1285, dur: 215 },  // Outro        ends 1500
];

export const PromoVideo: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const FADE = 25;

  const getEl = (idx: number): React.ReactElement => {
    const lf = Math.max(0, frame - SCENES_DEF[idx].from);
    switch (idx) {
      case 0: return <IntroScene localF={lf} fps={fps} />;
      case 1: return <BrandScene localF={lf} fps={fps} />;
      case 2: return <ScreenScene localF={lf} fps={fps} imgSrc={staticFile("screen1.png")}
        eyebrow="Connect · 连接" titleLines={["一键连接", "掌控数据"]}
        subtitle="支持 MySQL 与 PostgreSQL · 从这里开始你的数据之旅" />;
      case 3: return <ScreenScene localF={lf} fps={fps} imgSrc={staticFile("screen2.png")}
        eyebrow="Data Explorer · 数据探索" titleLines={["276 个对象", "尽在掌控"]}
        subtitle="筛选 · 排序 · 编辑 · 导出 · 所见即所得"
        pills={["智能筛选", "多表浏览", "CSV 导出"]} />;
      case 4: return <ScreenScene localF={lf} fps={fps} imgSrc={staticFile("screen3.png")}
        eyebrow="Server Manager · 服务器管理" titleLines={["不只数据库", "掌控服务器"]}
        subtitle="终端操控 · 实时监控 · SFTP 传输 · 一站式解决"
        pills={["SSH 终端", "系统监控", "SFTP 传输", "Docker"]} />;
      case 5: return <ScreenScene localF={lf} fps={fps} imgSrc={staticFile("screen4.png")}
        eyebrow="Theme · 主题" titleLines={["深色或浅色", "随你所好"]}
        subtitle="专注于工作本身 · 让界面服务于你" />;
      case 6: return <OutroScene localF={lf} fps={fps} />;
      default: return <AbsoluteFill />;
    }
  };

  return (
    <AbsoluteFill style={{ background: "#000" }}>
      {SCENES_DEF.map(({ from, dur }, i) => {
        const local = frame - from;
        if (local < -FADE || local >= dur + FADE) return null;
        let opacity = 1;
        if (local < FADE) opacity = Math.max(0, local / FADE);
        else if (local >= dur - FADE) opacity = Math.max(0, (dur - local) / FADE);
        return (
          <div key={i} style={{ position: "absolute", inset: 0, opacity }}>
            {getEl(i)}
          </div>
        );
      })}
      <Letterbox />
    </AbsoluteFill>
  );
};
