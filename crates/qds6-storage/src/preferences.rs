//! 功能：带默认值及校验的软件偏好，调用方在后台执行 SQLite；时间：2026-11-11；作者：lq。

use super::{ProfileStore, StoreError};
use qds6_core::TerminalTheme;
use serde::{Deserialize, Serialize};

/// 功能：持久化界面主题，不依赖 UI 类型；时间：2026-11-11；作者：lq。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorTheme {
    #[default]
    Light,
    Dark,
    /// 功能：保存跟随系统的选择，而非当前解析出的浅深色；时间：2026-11-11；作者：lq。
    System,
}

/// 功能：常用偏好允许旧记录缺字段，终端字号限于可用范围；时间：2026-11-11；作者：lq。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppPreferences {
    pub theme: ColorTheme,
    pub terminal_font_size: u8,
    pub terminal_theme: TerminalTheme,
    pub sql_soft_wrap: bool,
}

impl Default for AppPreferences {
    /// 功能：保持现有默认外观、14px 终端及 SQL 不换行行为；时间：2026-11-11；作者：lq。
    fn default() -> Self {
        Self {
            theme: ColorTheme::Light,
            terminal_font_size: 14,
            terminal_theme: TerminalTheme::default(),
            sql_soft_wrap: false,
        }
    }
}

impl ProfileStore {
    /// 功能：无偏好记录时使用默认值，损坏记录明确报错；时间：2026-11-11；作者：lq。
    pub fn preferences(&self) -> Result<AppPreferences, StoreError> {
        use rusqlite::OptionalExtension;
        let body: Option<String> = self
            .0
            .query_row("SELECT body FROM preferences WHERE id = 1", [], |row| {
                row.get(0)
            })
            .optional()?;
        let preferences: AppPreferences = body
            .map(|body| serde_json::from_str(&body))
            .transpose()?
            .unwrap_or_default();
        if !(10..=24).contains(&preferences.terminal_font_size) {
            return Err(StoreError::InvalidPreferences);
        }
        Ok(preferences)
    }

    /// 功能：单条语句原子保存偏好，无效字号不覆盖已有设置；时间：2026-11-11；作者：lq。
    pub fn save_preferences(&self, preferences: &AppPreferences) -> Result<(), StoreError> {
        if !(10..=24).contains(&preferences.terminal_font_size) {
            return Err(StoreError::InvalidPreferences);
        }
        self.0.execute("INSERT INTO preferences(id, body) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET body=excluded.body", [serde_json::to_string(preferences)?])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 功能：并发初始化数据库时连接和自动主题均可读取，防止启动竞争导致设置失效；时间：2026-11-11；作者：lq。
    #[test]
    fn concurrent_startup_reads_preserve_automatic_theme() {
        let path = std::env::temp_dir().join(format!("qds6-startup-{}.sqlite3", uuid_for_test()));
        let expected = AppPreferences {
            theme: ColorTheme::System,
            ..Default::default()
        };
        ProfileStore::open(&path)
            .unwrap()
            .save_preferences(&expected)
            .unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let barrier = barrier.clone();
                let path = &path;
                let expected = &expected;
                scope.spawn(move || {
                    barrier.wait();
                    for _ in 0..8 {
                        let store = ProfileStore::open(path).unwrap();
                        assert_eq!(&store.preferences().unwrap(), expected);
                        assert!(store.list().unwrap().is_empty());
                    }
                });
            }
        });
        std::fs::remove_file(path).unwrap();
    }

    /// 功能：三种主题均可落盘重开，兼容旧浅深色值及缺字段默认值；时间：2026-11-11；作者：lq。
    #[test]
    fn interface_theme_preferences_remain_backward_compatible() {
        let path = std::env::temp_dir().join(format!("qds6-themes-{}.sqlite3", uuid_for_test()));
        for (theme, value) in [
            (ColorTheme::Light, "light"),
            (ColorTheme::Dark, "dark"),
            (ColorTheme::System, "system"),
        ] {
            assert_eq!(
                serde_json::to_string(&theme).unwrap(),
                format!("\"{value}\"")
            );
            let preferences: AppPreferences =
                serde_json::from_str(&format!(r#"{{"theme":"{value}"}}"#)).unwrap();
            assert_eq!(preferences.theme, theme);
            let store = ProfileStore::open(&path).unwrap();
            store.save_preferences(&preferences).unwrap();
            drop(store);
            assert_eq!(
                ProfileStore::open(&path).unwrap().preferences().unwrap(),
                preferences
            );
        }
        assert_eq!(
            serde_json::from_str::<AppPreferences>("{}").unwrap().theme,
            ColorTheme::Light
        );
        std::fs::remove_file(path).unwrap();
    }

    /// 功能：验证旧库默认、持久化重开、非法写入保护及连接隔离；时间：2026-11-11；作者：lq。
    #[test]
    fn preferences_survive_reopen_and_invalid_write() {
        let path =
            std::env::temp_dir().join(format!("qds6-preferences-{}.sqlite3", uuid_for_test()));
        let store = ProfileStore::open(&path).unwrap();
        assert_eq!(store.preferences().unwrap(), AppPreferences::default());
        let preferences = AppPreferences {
            theme: ColorTheme::Dark,
            terminal_font_size: 18,
            terminal_theme: TerminalTheme::WarmPaper,
            sql_soft_wrap: true,
        };
        store.save_preferences(&preferences).unwrap();
        let mut invalid = preferences.clone();
        invalid.terminal_font_size = 255;
        assert!(store.save_preferences(&invalid).is_err());
        drop(store);
        let store = ProfileStore::open(&path).unwrap();
        assert_eq!(store.preferences().unwrap(), preferences);
        assert!(store.list().unwrap().is_empty());
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    /// 功能：生成无需额外依赖的独立测试路径；时间：2026-11-11；作者：lq。
    fn uuid_for_test() -> String {
        format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

    /// 功能：升级前的设置缺少终端主题时保持原深色，并覆盖四个选择的序列化；时间：2026-11-11；作者：lq。
    #[test]
    fn terminal_theme_defaults_for_existing_preferences() {
        let old: AppPreferences = serde_json::from_str(
            r#"{"theme":"dark","terminal_font_size":16,"sql_soft_wrap":true}"#,
        )
        .unwrap();
        assert_eq!(old.terminal_theme, TerminalTheme::DeepSea);
        for theme in TerminalTheme::ALL {
            let preferences = AppPreferences {
                terminal_theme: theme,
                ..old.clone()
            };
            assert_eq!(
                serde_json::from_str::<AppPreferences>(
                    &serde_json::to_string(&preferences).unwrap()
                )
                .unwrap(),
                preferences
            );
        }
    }
}
