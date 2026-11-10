//! 功能：统一 QDS6 品牌资源与 Lucide 界面图标，按需静态嵌入；时间：2026-11-10；作者：lq。

use gpui_kit::{AssetSource, Result, SharedString, assets::Assets};
use std::borrow::Cow;

/// 功能：统一使用 Kit 完整目录的图标名，避免旧兼容枚举缺少业务图标；时间：2026-11-10；作者：lq。
pub use gpui_kit::assets::IconName as UiIcon;

/// 功能：界面品牌路径，彩色图像使用 img，单色标识使用 Icon；时间：2026-11-10；作者：lq。
pub const BRAND_IMAGE: &str = "brand/qds6.png";
/// 功能：单色品牌路径随当前主题前景色绘制；时间：2026-11-10；作者：lq。
pub const BRAND_MARK: &str = "brand/qds6-mark.svg";

// 功能：选择工作台所需 Lucide 图标；保留 Kit 上游资源及许可，不复制或改署作者；时间：2026-11-10；作者：lq。
gpui_kit::assets::icon_assets!(
    WorkbenchIcons,
    [
        Server,
        Terminal,
        Folder,
        Database,
        Search,
        Plus,
        Save,
        Star,
        Sun,
        Moon,
        Copy,
        RefreshCw,
        Settings,
        Download,
        Upload,
        KeyRound,
        Lock,
        Plug,
        Unplug,
        Table2,
        FileCode,
        Trash,
        Check,
        TriangleAlert,
        ChevronRight,
        ChevronDown,
        Ellipsis,
        PanelLeft,
        SquareTerminal,
        Network,
        Play,
        LoaderCircle,
        Circle,
    ]
);

/// 功能：组合品牌、业务图标与组件默认资源，运行时无需网络或外部文件；时间：2026-11-10；作者：lq。
pub struct AppAssets;

impl AssetSource for AppAssets {
    /// 功能：优先命中自有品牌和选中图标，最后回落到 Kit 默认资源；时间：2026-11-10；作者：lq。
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match path {
            BRAND_IMAGE => Ok(Some(Cow::Borrowed(include_bytes!(
                "../../../assets/brand/png/qds6-256.png"
            )))),
            BRAND_MARK => Ok(Some(Cow::Borrowed(include_bytes!(
                "../../../assets/brand/qds6-mark.svg"
            )))),
            _ => match WorkbenchIcons.load(path)? {
                Some(bytes) => Ok(Some(bytes)),
                None => Assets.load(path),
            },
        }
    }

    /// 功能：列举资源供 GPUI 查询，去重且遵守路径前缀；时间：2026-11-10；作者：lq。
    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(WorkbenchIcons.list(path)?);
        paths.extend(
            [BRAND_IMAGE, BRAND_MARK]
                .into_iter()
                .filter(|name| name.starts_with(path))
                .map(SharedString::from),
        );
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
