//! 功能：QDS6 后台协议和可移植配置服务，禁止在 UI 线程执行阻塞调用；时间：2026-11-10；作者：lq。

pub mod backup;
pub mod credentials;
pub mod database;
pub mod drafts;
pub mod migration;
pub mod sftp;
pub mod ssh;
pub mod terminal;
// 功能：仅用户主动操作时查询开源仓库正式发布；时间：2026-11-11；作者：lq。
pub mod updates;
