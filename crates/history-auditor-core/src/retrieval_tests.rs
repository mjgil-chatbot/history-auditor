use chrono::{TimeZone, Utc};
use rusqlite::{params, Connection};
use std::fs;
use super::paths;
fn set_audited_at(workspace: &std::path::Path, session_id: &str, audited_at: &str) {
    let paths = paths::ensure_workspace_layout(workspace).unwrap();
    let conn = Connection::open(&paths.db_path).unwrap();
    conn.execute(
        "UPDATE audit_sessions SET audited_at = ?2 WHERE session_id = ?1",
        params![session_id, audited_at],
    )
    .unwrap();
}
