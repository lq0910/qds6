//! 功能：基础表/视图的 SQL 结构和数据转储，保留约束、索引、备注与身份序列；时间：2026-11-12；作者：lq。
use super::*;

impl DatabaseSession {
    /// 功能：MySQL 使用服务端原始 DDL，PostgreSQL 组合可信系统目录定义；时间：2026-11-12；作者：lq。
    pub(super) fn dump_structure(&self, table: &Table) -> Result<(String, String), String> {
        let p = self.profile.protocol;
        let target = format!(
            "{}.{}",
            identifier(p, &table.schema)?,
            identifier(p, &table.name)?
        );
        let mut prefix = String::from(
            "-- QDS6 SQL dump · 2026-11-12 · lq\n-- Restore into a database with the required schemas, types and referenced tables.\n",
        );
        let mut suffix = String::new();
        if p == Protocol::MySql {
            prefix.push_str("SET NAMES utf8mb4;\nSET @QDS6_OLD_SQL_MODE=@@SQL_MODE;\nSET SQL_MODE='NO_AUTO_VALUE_ON_ZERO';\n");
            let r = self.query(&format!(
                "SHOW CREATE {} {target}",
                if table.kind == "VIEW" {
                    "VIEW"
                } else {
                    "TABLE"
                }
            ))?;
            let row = r
                .sets
                .iter()
                .find_map(|s| s.rows.first())
                .ok_or("无法读取对象定义")?;
            prefix.push_str(&text(&row[1]));
            prefix.push_str(";\n");
            suffix.push_str("SET SQL_MODE=@QDS6_OLD_SQL_MODE;\n");
            return Ok((prefix, suffix));
        }
        prefix.push_str("SET standard_conforming_strings=on;\n");
        let schema = pg_literal(&table.schema);
        let name = pg_literal(&table.name);
        if table.kind == "VIEW" {
            let r=self.query(&format!("SELECT pg_get_viewdef(c.oid,true) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname={schema} AND c.relname={name}"))?;
            let row = r
                .sets
                .iter()
                .find_map(|s| s.rows.first())
                .ok_or("无法读取视图定义")?;
            prefix.push_str(&format!(
                "CREATE VIEW {target} AS {};\n",
                text(&row[0]).trim_end_matches(';')
            ));
            return Ok((prefix, suffix));
        }
        let r=self.query(&format!("SELECT c.relkind::text,c.relispartition::text,c.relrowsecurity::text,EXISTS(SELECT 1 FROM pg_inherits i WHERE i.inhrelid=c.oid)::text,COALESCE(obj_description(c.oid,'pg_class'),'') FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname={schema} AND c.relname={name}"))?;
        let row = r
            .sets
            .iter()
            .find_map(|s| s.rows.first())
            .ok_or("表已不存在")?;
        if text(&row[0]) != "r" || row[1..4].iter().any(|c| text(c) == "true") {
            return Err("基础转储暂不支持分区、继承或行级安全表，请使用 pg_dump".into());
        }
        let comment = text(&row[4]);
        let sql = format!(
            "SELECT a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull::text,COALESCE(pg_get_expr(d.adbin,d.adrelid),''),a.attidentity::text,a.attgenerated::text,COALESCE(col_description(c.oid,a.attnum),''),COALESCE(pg_get_serial_sequence(format('%I.%I',n.nspname,c.relname),a.attname),''),CASE WHEN a.attcollation<>0 AND a.attcollation<>t.typcollation THEN quote_ident(cn.nspname)||'.'||quote_ident(co.collname) ELSE '' END FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_attribute a ON a.attrelid=c.oid JOIN pg_type t ON t.oid=a.atttypid LEFT JOIN pg_attrdef d ON d.adrelid=c.oid AND d.adnum=a.attnum LEFT JOIN pg_collation co ON co.oid=a.attcollation LEFT JOIN pg_namespace cn ON cn.oid=co.collnamespace WHERE n.nspname={schema} AND c.relname={name} AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum"
        );
        let rows = self
            .query(&sql)?
            .sets
            .into_iter()
            .flat_map(|s| s.rows)
            .collect::<Vec<_>>();
        let mut definitions = Vec::new();
        for row in rows {
            let col = identifier(p, &text(&row[0]))?;
            let sequence = text(&row[7]);
            let identity = text(&row[4]);
            let generated = text(&row[5]);
            let default = text(&row[3]);
            // 功能：serial 按实际序列定义创建；身份列保留自增配置，恢复后校准 last_value；时间：2026-11-12；作者：lq。
            let mut seq_options = String::new();
            if !sequence.is_empty() {
                let seq=self.query(&format!("SELECT seqstart::text,seqincrement::text,seqmin::text,seqmax::text,seqcache::text,seqcycle::text FROM pg_sequence WHERE seqrelid={}::regclass",pg_literal(&sequence)))?;
                let v = seq
                    .sets
                    .iter()
                    .find_map(|s| s.rows.first())
                    .ok_or("无法读取序列配置")?;
                seq_options = format!(
                    "START WITH {} INCREMENT BY {} MINVALUE {} MAXVALUE {} CACHE {} {}",
                    text(&v[0]),
                    text(&v[1]),
                    text(&v[2]),
                    text(&v[3]),
                    text(&v[4]),
                    if text(&v[5]) == "true" {
                        "CYCLE"
                    } else {
                        "NO CYCLE"
                    }
                );
                if identity.is_empty() {
                    prefix.push_str(&format!(
                        "CREATE SEQUENCE {sequence} AS {} {seq_options};\n",
                        text(&row[1])
                    ));
                    suffix.push_str(&format!(
                        "ALTER SEQUENCE {sequence} OWNED BY {target}.{col};\n"
                    ));
                }
                let state = self.query(&format!(
                    "SELECT last_value::text,is_called::text FROM {sequence}"
                ))?;
                let v = state
                    .sets
                    .iter()
                    .find_map(|s| s.rows.first())
                    .ok_or("无法读取序列状态")?;
                suffix.push_str(&format!(
                    "SELECT pg_catalog.setval({}, {}, {});\n",
                    pg_literal(&sequence),
                    text(&v[0]),
                    text(&v[1])
                ));
            }
            let mut def = format!("{col} {}", text(&row[1]));
            if !text(&row[8]).is_empty() {
                def.push_str(&format!(" COLLATE {}", text(&row[8])));
            }
            if !identity.is_empty() {
                def.push_str(&format!(
                    " GENERATED {} AS IDENTITY (SEQUENCE NAME {sequence} {seq_options})",
                    if identity == "a" {
                        "ALWAYS"
                    } else {
                        "BY DEFAULT"
                    }
                ));
            } else if !generated.is_empty() {
                def.push_str(&format!(
                    " GENERATED ALWAYS AS ({default}) {}",
                    if generated == "v" {
                        "VIRTUAL"
                    } else {
                        "STORED"
                    }
                ));
            } else if !default.is_empty() {
                def.push_str(&format!(" DEFAULT {default}"));
            }
            if text(&row[2]) == "true" {
                def.push_str(" NOT NULL");
            }
            definitions.push(def);
            if !text(&row[6]).is_empty() {
                suffix.push_str(&format!(
                    "COMMENT ON COLUMN {target}.{col} IS {};\n",
                    pg_literal(&text(&row[6]))
                ));
            }
        }
        prefix.push_str(&format!(
            "CREATE TABLE {target} (\n  {}\n);\n",
            definitions.join(",\n  ")
        ));
        let constraints=self.query(&format!("SELECT con.conname,pg_get_constraintdef(con.oid,true) FROM pg_constraint con JOIN pg_class c ON c.oid=con.conrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname={schema} AND c.relname={name} AND con.contype IN ('p','u','c','f','x') ORDER BY CASE WHEN con.contype='f' THEN 1 ELSE 0 END,con.conname"))?;
        for r in constraints.sets.into_iter().flat_map(|s| s.rows) {
            suffix.push_str(&format!(
                "ALTER TABLE {target} ADD CONSTRAINT {} {};\n",
                identifier(p, &text(&r[0]))?,
                text(&r[1])
            ));
        }
        let indexes=self.query(&format!("SELECT pg_get_indexdef(i.indexrelid) FROM pg_index i JOIN pg_class c ON c.oid=i.indrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname={schema} AND c.relname={name} AND NOT EXISTS(SELECT 1 FROM pg_constraint co WHERE co.conindid=i.indexrelid) ORDER BY i.indexrelid"))?;
        for r in indexes.sets.into_iter().flat_map(|s| s.rows) {
            suffix.push_str(&format!("{};\n", text(&r[0])));
        }
        if !comment.is_empty() {
            suffix.push_str(&format!(
                "COMMENT ON TABLE {target} IS {};\n",
                pg_literal(&comment)
            ));
        }
        Ok((prefix, suffix))
    }

    /// 功能：列名显式引用，计算列省略，身份列保留实际值，二进制使用方言十六进制；时间：2026-11-12；作者：lq。
    pub(super) fn dump_insert(
        &self,
        table: &Table,
        columns: &[GridColumn],
        values: &[Cell],
    ) -> Result<String, String> {
        if table.kind != "BASE TABLE" {
            return Ok(String::new());
        }
        let p = self.profile.protocol;
        let fields: Vec<_> = columns
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                !c.extra.contains("GENERATED") || c.extra.contains("DEFAULT_GENERATED")
            })
            .collect();
        let names = fields
            .iter()
            .map(|(_, c)| identifier(p, &c.name))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let data = fields
            .iter()
            .map(|(i, _)| match &values[*i] {
                Cell::Null => "NULL".into(),
                Cell::Binary(bytes) => {
                    if p == Protocol::PostgreSql {
                        format!("decode('{}','hex')", hex(bytes))
                    } else {
                        format!("X'{}'", hex(bytes))
                    }
                }
                Cell::Text(s) => {
                    if p == Protocol::PostgreSql {
                        format!("E'{}'", s.replace('\\', "\\\\").replace('\'', "''"))
                    } else {
                        literal(s)
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let target = format!(
            "{}.{}",
            identifier(p, &table.schema)?,
            identifier(p, &table.name)?
        );
        Ok(format!(
            "INSERT INTO {target} ({names}){} VALUES ({data});\n",
            if p == Protocol::PostgreSql && columns.iter().any(|c| c.extra == "IDENTITY") {
                " OVERRIDING SYSTEM VALUE"
            } else {
                ""
            }
        ))
    }
}

/// 功能：PostgreSQL 备注及对象文字显式使用 E 字符串，保留反斜线；时间：2026-11-12；作者：lq。
fn pg_literal(value: &str) -> String {
    format!("E{}", literal(value))
}
