//! 功能：SQL 语法关键字补全与已连接数据库的表名智能联想；时间：2026-11-13；作者：lq。

// 功能：沿用 Kit 重导出的 GPUI 类型；时间：2026-11-13；作者：lq。
use gpui_kit::component::input::{CompletionProvider, RopeExt};
use gpui_kit::gpui::{App, Task, Window};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, TextEdit,
};
use ropey::Rope;
use std::sync::{Arc, Mutex};

/// 功能：补全提供者持有的共享表名列表，连接成功或刷新后原地更新；时间：2026-11-13；作者：lq。
#[derive(Clone, Default)]
pub(crate) struct TableNames(pub Arc<Mutex<Vec<String>>>);

impl TableNames {
    /// 功能：替换全部表名，每次 load_tables 完成后调用一次；时间：2026-11-13；作者：lq。
    pub fn set(&self, names: impl IntoIterator<Item = String>) {
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        *guard = names.into_iter().collect();
    }

    /// 功能：断开连接后清空，避免残留旧库表名；时间：2026-11-13；作者：lq。
    pub fn clear(&self) {
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        guard.clear();
    }
}

/// 功能：PostgreSQL / MySQL 常用关键字，大写以匹配惯例；时间：2026-11-13；作者：lq。
static SQL_KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "AND",
    "OR",
    "NOT",
    "IN",
    "IS",
    "NULL",
    "AS",
    "JOIN",
    "LEFT",
    "RIGHT",
    "INNER",
    "OUTER",
    "FULL",
    "CROSS",
    "ON",
    "GROUP",
    "BY",
    "ORDER",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "DISTINCT",
    "ALL",
    "UNION",
    "EXCEPT",
    "INTERSECT",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "CREATE",
    "TABLE",
    "VIEW",
    "INDEX",
    "DROP",
    "ALTER",
    "ADD",
    "COLUMN",
    "RENAME",
    "TRUNCATE",
    "PRIMARY",
    "KEY",
    "FOREIGN",
    "REFERENCES",
    "UNIQUE",
    "DEFAULT",
    "NOT NULL",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "TRANSACTION",
    "WITH",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "CAST",
    "COALESCE",
    "NULLIF",
    "EXISTS",
    "BETWEEN",
    "LIKE",
    "ILIKE",
    "RETURNING",
    "EXPLAIN",
    "ANALYZE",
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "NOW",
    "CURRENT_TIMESTAMP",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "EXTRACT",
    "DATE_TRUNC",
    "INTERVAL",
    "TRUE",
    "FALSE",
];

/// 功能：SQL 补全提供者，合并关键字与当前连接的表名；时间：2026-11-13；作者：lq。
pub(crate) struct SqlCompletionProvider {
    table_names: TableNames,
}

impl SqlCompletionProvider {
    pub fn new(table_names: TableNames) -> Self {
        Self { table_names }
    }
}

impl CompletionProvider for SqlCompletionProvider {
    /// 功能：从光标反查当前词前缀，匹配关键字与表名后返回补全列表；时间：2026-11-13；作者：lq。
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<anyhow::Result<CompletionResponse>> {
        // 功能：从光标向左取连续标识符字符作为前缀，区分大小写不敏感匹配；时间：2026-11-13；作者：lq。
        let prefix_len = text
            .chars_at(offset)
            .reversed()
            .take_while(|&ch| ch.is_alphanumeric() || ch == '_' || ch == '.')
            .map(char::len_utf8)
            .sum::<usize>();
        let prefix_start = offset - prefix_len;
        let prefix = text.slice(prefix_start..offset).to_string().to_lowercase();

        // 功能：text_edit 范围覆盖整个前缀，编辑器用它替换已输入部分，不会追加；时间：2026-11-13；作者：lq。
        let replace_range = lsp_types::Range {
            start: text.offset_to_position(prefix_start),
            end: text.offset_to_position(offset),
        };

        let make_edit = |label: &str| {
            lsp_types::CompletionTextEdit::Edit(TextEdit {
                range: replace_range,
                new_text: label.to_string(),
            })
        };

        let mut items: Vec<CompletionItem> = Vec::new();

        // 功能：表名补全：schema.name 整体匹配，优先于关键字排序；时间：2026-11-13；作者：lq。
        {
            let guard = self.table_names.0.lock().unwrap_or_else(|e| e.into_inner());
            for name in guard.iter() {
                if name.to_lowercase().contains(&prefix) {
                    items.push(CompletionItem {
                        label: name.clone(),
                        kind: Some(CompletionItemKind::CLASS),
                        sort_text: Some(format!("0_{name}")),
                        text_edit: Some(make_edit(name)),
                        ..Default::default()
                    });
                }
            }
        }

        // 功能：SQL 关键字补全，前缀为空时不展示以避免噪声；时间：2026-11-13；作者：lq。
        if !prefix.is_empty() {
            for &kw in SQL_KEYWORDS {
                if kw.to_lowercase().starts_with(&prefix) {
                    items.push(CompletionItem {
                        label: kw.to_string(),
                        kind: Some(CompletionItemKind::KEYWORD),
                        sort_text: Some(format!("1_{kw}")),
                        text_edit: Some(make_edit(kw)),
                        ..Default::default()
                    });
                }
            }
        }

        Task::ready(Ok(CompletionResponse::Array(items)))
    }

    /// 功能：字母、下划线和点触发补全，空格或符号不触发；时间：2026-11-13；作者：lq。
    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        new_text
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '.')
    }
}
