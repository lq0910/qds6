//! 功能：四套原创终端配色，统一默认色、ANSI、光标与搜索高亮；时间：2026-11-11；作者：lq。

use serde::{Deserialize, Serialize};

/// 功能：两浅两深，默认深海延续原终端外观，独立于软件主题；时间：2026-11-11；作者：lq。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalTheme {
    ClearSky,
    WarmPaper,
    #[default]
    DeepSea,
    InkForest,
}

/// 功能：完整绘制色表，RGB 不依赖 GPUI 或网络；时间：2026-11-11；作者：lq。
#[derive(Clone, Copy, Debug)]
pub struct TerminalPalette {
    pub background: u32,
    pub foreground: u32,
    pub cursor: u32,
    pub cursor_text: u32,
    pub search_background: u32,
    pub search_foreground: u32,
    pub ansi: [u32; 16],
}

impl TerminalTheme {
    /// 功能：设置页固定按浅色、深色顺序列出四套主题；时间：2026-11-11；作者：lq。
    pub const ALL: [Self; 4] = [
        Self::ClearSky,
        Self::WarmPaper,
        Self::DeepSea,
        Self::InkForest,
    ];

    /// 功能：可持久化主题对应稳定的界面测试标识；时间：2026-11-11；作者：lq。
    pub const fn id(self) -> &'static str {
        match self {
            Self::ClearSky => "terminal-theme-clear-sky",
            Self::WarmPaper => "terminal-theme-warm-paper",
            Self::DeepSea => "terminal-theme-deep-sea",
            Self::InkForest => "terminal-theme-ink-forest",
        }
    }

    /// 功能：中文名称说明每套配色的视觉方向；时间：2026-11-11；作者：lq。
    pub const fn label(self) -> &'static str {
        match self {
            Self::ClearSky => "晴空",
            Self::WarmPaper => "暖纸",
            Self::DeepSea => "深海",
            Self::InkForest => "墨林",
        }
    }

    /// 功能：亮度分类用于预览说明和验证，两套浅色、两套深色；时间：2026-11-11；作者：lq。
    pub const fn is_dark(self) -> bool {
        matches!(self, Self::DeepSea | Self::InkForest)
    }

    /// 功能：原创调色板兼顾命令、日志、报错及目录色的可读性；时间：2026-11-11；作者：lq。
    pub const fn palette(self) -> TerminalPalette {
        match self {
            Self::ClearSky => TerminalPalette {
                background: 0xf4f7fb,
                foreground: 0x26364a,
                cursor: 0x3669a8,
                cursor_text: 0xffffff,
                search_background: 0xf4d58d,
                search_foreground: 0x3c310f,
                ansi: [
                    0x26364a, 0xb33c45, 0x357345, 0x89620a, 0x2c62ab, 0x8a4b9f, 0x21747e, 0x546474,
                    0x748496, 0xc04552, 0x3c7d50, 0x956e12, 0x366fba, 0x9856ac, 0x2b7f8b, 0x172b42,
                ],
            },
            Self::WarmPaper => TerminalPalette {
                background: 0xfaf5e9,
                foreground: 0x433b31,
                cursor: 0x8c6540,
                cursor_text: 0xfffcf5,
                search_background: 0xead39c,
                search_foreground: 0x4f3b16,
                ansi: [
                    0x433b31, 0xa23e35, 0x567035, 0x896017, 0x386581, 0x8c5278, 0x2c7470, 0x6b6258,
                    0x8d8172, 0xb44c43, 0x627c40, 0x966e22, 0x44718c, 0x995e84, 0x38817c, 0x302a23,
                ],
            },
            Self::DeepSea => TerminalPalette {
                background: 0x101722,
                foreground: 0xdce3ec,
                cursor: 0x98b9de,
                cursor_text: 0x101722,
                search_background: 0x66502b,
                search_foreground: 0xffe3a3,
                ansi: [
                    0x18212f, 0xee6c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xdce3ec,
                    0x65748b, 0xff8790, 0xb5e890, 0xffdf91, 0x8ac7ff, 0xe8a1ff, 0x82dbe5, 0xffffff,
                ],
            },
            Self::InkForest => TerminalPalette {
                background: 0x142320,
                foreground: 0xdce9df,
                cursor: 0xb3d6b8,
                cursor_text: 0x142320,
                search_background: 0x536044,
                search_foreground: 0xf5edbe,
                ansi: [
                    0x22352f, 0xe78f87, 0xa8cb98, 0xe1c58a, 0x91baca, 0xc0a2ca, 0x8fcab9, 0xdce9df,
                    0x78968d, 0xf4a39c, 0xb9dca8, 0xefdaa2, 0xa5cede, 0xd2b5dc, 0xa2decc, 0xf1f7ee,
                ],
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 功能：验证默认文字、光标和搜索色对比，并固定两浅两深的数量；时间：2026-11-11；作者：lq。
    #[test]
    fn themes_have_readable_text_and_balanced_light_dark_options() {
        assert_eq!(
            TerminalTheme::ALL
                .iter()
                .filter(|theme| theme.is_dark())
                .count(),
            2
        );
        for theme in TerminalTheme::ALL {
            let p = theme.palette();
            assert!(
                contrast(p.foreground, p.background) >= 7.,
                "{} body",
                theme.label()
            );
            assert!(
                contrast(p.cursor_text, p.cursor) >= 4.5,
                "{} cursor",
                theme.label()
            );
            assert!(
                contrast(p.search_foreground, p.search_background) >= 4.5,
                "{} search",
                theme.label()
            );
            for index in [1, 2, 3, 4, 5, 6, 9, 10, 11, 12, 13, 14] {
                assert!(
                    contrast(p.ansi[index], p.background) >= 3.,
                    "{} ANSI {index}",
                    theme.label()
                );
            }
        }
    }

    /// 功能：按线性 sRGB 亮度计算文字与底色对比，用于可读性回归；时间：2026-11-11；作者：lq。
    fn contrast(a: u32, b: u32) -> f64 {
        let luminance = |rgb: u32| {
            let channel = |shift: u32| {
                let c = f64::from((rgb >> shift) & 255u32) / 255.;
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
        };
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
}
