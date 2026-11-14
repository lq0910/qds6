// 功能：注册 QDS6 宣传片的 1080p Remotion 合成；时间：2026-11-14；作者：lq。
import React from "react";
import { Composition } from "remotion";
import { PromoVideo } from "./Promo";

export const RemotionRoot: React.FC = () => {
  return (
    <>
      <Composition
        id="QDS6Promo"
        component={PromoVideo}
        durationInFrames={1500}
        fps={30}
        width={1920}
        height={1080}
        defaultProps={{}}
      />
    </>
  );
};
