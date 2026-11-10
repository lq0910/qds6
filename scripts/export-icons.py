#!/usr/bin/env python3
"""功能：从矢量母版导出桌面图标与设计预览；时间：2026-11-10；作者：lq。"""

import argparse
import io
from pathlib import Path
import subprocess

from PIL import Image, ImageDraw, ImageFont


def render_svg(source: Path, size: int) -> Image.Image:
    """功能：使用 librsvg 抗锯齿栅格化，保留透明通道；时间：2026-11-10；作者：lq。"""
    data = subprocess.check_output(
        ["rsvg-convert", "-w", str(size), "-h", str(size), str(source)]
    )
    return Image.open(io.BytesIO(data)).convert("RGBA")


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont:
    """功能：使用可用系统字体绘制预览文字；时间：2026-11-10；作者：lq。"""
    filename = "Arial Bold.ttf" if bold else "Arial.ttf"
    candidates = [
        Path("/System/Library/Fonts/Supplemental") / filename,
        Path("/usr/share/fonts/truetype/dejavu")
        / ("DejaVuSans-Bold.ttf" if bold else "DejaVuSans.ttf"),
    ]
    for candidate in candidates:
        if candidate.exists():
            return ImageFont.truetype(str(candidate), size)
    return ImageFont.load_default(size=size)


def preview(brand: Path, kit_icons: Path | None) -> None:
    """功能：输出品牌、大小尺寸、明暗背景和 Lucide 样张；时间：2026-11-10；作者：lq。"""
    board = Image.new("RGBA", (1440, 1060), "#F4F6F3")
    draw = ImageDraw.Draw(board)
    ink, muted = "#173038", "#6B7D80"
    draw.text((70, 46), "QDS6", font=font(38, True), fill=ink)
    draw.text((72, 99), "Connections, simplified.", font=font(20), fill=muted)
    draw.text((1120, 59), "BRAND / 01", font=font(15), fill=muted)
    board.alpha_composite(render_svg(brand / "qds6.svg", 440), (58, 153))
    draw.text((560, 195), "One connection.", font=font(48, True), fill=ink)
    draw.text((560, 254), "One workspace.", font=font(48, True), fill=ink)
    draw.text((562, 328), "A Q. A link. A clear starting point.", font=font(21), fill=muted)
    for i, (color, label) in enumerate([
        ("#122C34", "Midnight"), ("#F2FAF7", "Porcelain"), ("#54D9B1", "Mint"),
    ]):
        x = 563 + i * 200
        draw.rounded_rectangle((x, 397, x + 30, 427), radius=9, fill=color, outline="#D7DFDC")
        draw.text((x + 42, 400), label, font=font(16), fill=ink)
    draw.rounded_rectangle((560, 463, 1328, 568), radius=20, fill="#142C34")
    draw.text((584, 484), "AT EVERY SIZE", font=font(12), fill="#A9BDBD")
    x = 785
    for size in [16, 24, 32, 48, 64]:
        board.alpha_composite(render_svg(brand / "qds6.svg", size), (x, 482 + (64 - size) // 2))
        x += size + 26
    draw.line((72, 618, 1368, 618), fill="#DAE1DD", width=1)
    draw.text((72, 653), "A consistent language for the workbench", font=font(27, True), fill=ink)
    draw.text((73, 696), "LUCIDE  /  24 PX GRID  /  2 PX STROKE  /  THEME AWARE", font=font(13), fill=muted)
    names = [
        "server", "terminal", "folder", "database", "search", "plus", "save", "star",
        "download", "upload", "key-round", "lock", "plug", "unplug", "table-2", "file-code",
        "copy", "refresh-cw", "settings", "trash", "sun", "moon", "network", "play",
        "check", "triangle-alert", "chevron-right", "ellipsis", "panel-left", "square-terminal", "loader-circle", "circle",
    ]
    if kit_icons:
        for i, name in enumerate(names):
            x, y = 90 + i % 8 * 166, 760 + i // 8 * 65
            icon = render_svg(kit_icons / f"{name}.svg", 26)
            # 功能：预览按统一主题色着色，保留上游 SVG 本身；时间：2026-11-10；作者：lq。
            tinted = Image.new("RGBA", icon.size, ink)
            tinted.putalpha(icon.getchannel("A"))
            board.alpha_composite(tinted, (x, y))
            draw.text((x + 37, y + 5), name, font=font(11), fill=muted)
    draw.text((73, 1030), "QDS6 brand: lq / 2026-11-10    |    Lucide: ISC + Feather-derived MIT. See third-party notices.", font=font(12), fill=muted)
    board.convert("RGB").save(brand / "preview.png")


def main() -> None:
    """功能：导出 PNG、Windows ICO、macOS ICNS；时间：2026-11-10；作者：lq。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kit-icons", type=Path, help="Pinned GPUI Kit crates/assets/assets/icons directory")
    args = parser.parse_args()
    brand = Path(__file__).resolve().parents[1] / "assets" / "brand"
    png = brand / "png"
    png.mkdir(parents=True, exist_ok=True)
    sizes = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
    for size in sizes:
        render_svg(brand / "qds6.svg", size).save(png / f"qds6-{size}.png")
    render_svg(brand / "qds6.svg", 256).save(
        brand / "qds6.ico", sizes=[(n, n) for n in [16, 24, 32, 48, 64, 128, 256]]
    )
    # 功能：跨平台生成 ICNS，附加各档矢量直出图，避免依赖 macOS 图形会话；时间：2026-11-10；作者：lq。
    with Image.open(png / "qds6-1024.png") as master:
        master.save(brand / "qds6.icns", append_images=[render_svg(brand / "qds6.svg", n) for n in [16, 32, 64, 128, 256, 512]])
    if args.kit_icons:
        preview(brand, args.kit_icons)
    else:
        print("Preview unchanged; provide --kit-icons to regenerate the Lucide sample sheet.")
    print(f"Exported QDS6 icon assets to {brand}")


if __name__ == "__main__":
    main()
