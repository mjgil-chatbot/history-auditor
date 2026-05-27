use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde_json::json;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::incident::incident_pattern_label;
use super::paths::{memory_corpus_path, WorkspaceAuditorPaths};
use super::schema::{
    AuditRecord, CorpusSearchHit, ErrorKind, MemoryRecord, MemoryStatus, StoredAuditSummary,
    TaskRun,
};
use super::store_support::{
    error_kind_label, persist_action_tx, persist_claim_tx, persist_edges_tx, persist_error_tx,
    persist_intent_tx, persist_memory_tx, read_memories, row_to_memory, serde_label, task_type,
    write_memory_corpus,
};

pub fn ensure_store(paths: &WorkspaceAuditorPaths) -> Result<()> {
    let conn = Connection::open(&paths.db_path)
        .with_context(|| format!("failed to open {}", paths.db_path.display()))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS audit_sessions (
            session_id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL,
            workspace_root TEXT NOT NULL,
            provider TEXT NOT NULL,
            provider_display_name TEXT NOT NULL,
            session_name TEXT NOT NULL,
            session_path TEXT NOT NULL,
            audited_at TEXT NOT NULL,
            branch TEXT,
            task_type TEXT NOT NULL DEFAULT 'unknown',
            primary_error_kind TEXT,
            incident_pattern TEXT,
            final_verdict TEXT NOT NULL DEFAULT 'ambiguous_missing_evidence',
            requested_goal TEXT NOT NULL DEFAULT '',
            commit_verdict TEXT NOT NULL DEFAULT 'commit_not_attempted',
            push_verdict TEXT NOT NULL DEFAULT 'push_not_attempted',
            merge_verdict TEXT NOT NULL DEFAULT 'merge_not_attempted',
            confidence REAL NOT NULL DEFAULT 0.0,
            evidence_tier INTEGER NOT NULL DEFAULT 0,
            task_json TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS raw_events (
            event_id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            ordinal INTEGER NOT NULL,
            actor TEXT NOT NULL,
            kind TEXT NOT NULL,
            timestamp TEXT,
            source_uri TEXT NOT NULL,
            source_span TEXT,
            raw_content TEXT NOT NULL,
            command TEXT,
            stdout TEXT,
            stderr TEXT,
            exit_code INTEGER,
            content_hash TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS intent_events (
            intent_id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            confidence REAL NOT NULL,
            evidence_event_ids_json TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS action_events (
            action_id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            status TEXT NOT NULL,
            command TEXT,
            exit_code INTEGER,
            stdout_excerpt TEXT,
            stderr_excerpt TEXT,
            commit_hash TEXT,
            evidence_event_ids_json TEXT NOT NULL,
            evidence_tier INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS error_events (
            error_id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            message TEXT NOT NULL,
            evidence_event_ids_json TEXT NOT NULL,
            evidence_tier INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS claim_events (
            claim_id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            summary TEXT NOT NULL,
            confidence REAL NOT NULL,
            evidence_event_ids_json TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS audit_edges (
            edge_id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            edge_kind TEXT NOT NULL,
            src_kind TEXT NOT NULL,
            src_id TEXT NOT NULL,
            dst_kind TEXT NOT NULL,
            dst_id TEXT NOT NULL,
            detail TEXT
        );
        CREATE TABLE IF NOT EXISTS memory_records (
            memory_id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            status TEXT NOT NULL,
            kind TEXT NOT NULL,
            confidence REAL NOT NULL,
            text TEXT NOT NULL,
            repo_path TEXT,
            branch TEXT,
            evidence_event_ids_json TEXT NOT NULL,
            supporting_task_ids_json TEXT NOT NULL DEFAULT '[]',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            status_reason TEXT
        );",
    )?;

    ensure_column(&conn, "audit_sessions", "branch", "TEXT")?;
    ensure_column(
        &conn,
        "audit_sessions",
        "task_type",
        "TEXT NOT NULL DEFAULT 'unknown'",
    )?;
    ensure_column(&conn, "audit_sessions", "primary_error_kind", "TEXT")?;
    ensure_column(&conn, "audit_sessions", "incident_pattern", "TEXT")?;
    ensure_column(
        &conn,
        "audit_sessions",
        "final_verdict",
        "TEXT NOT NULL DEFAULT 'ambiguous_missing_evidence'",
    )?;
    ensure_column(
        &conn,
        "audit_sessions",
        "requested_goal",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        &conn,
        "audit_sessions",
        "commit_verdict",
        "TEXT NOT NULL DEFAULT 'commit_not_attempted'",
    )?;
    ensure_column(
        &conn,
        "audit_sessions",
        "push_verdict",
        "TEXT NOT NULL DEFAULT 'push_not_attempted'",
    )?;
    ensure_column(
        &conn,
        "audit_sessions",
        "merge_verdict",
        "TEXT NOT NULL DEFAULT 'merge_not_attempted'",
    )?;
    ensure_column(
        &conn,
        "audit_sessions",
        "confidence",
        "REAL NOT NULL DEFAULT 0.0",
    )?;
    ensure_column(
        &conn,
        "audit_sessions",
        "evidence_tier",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        &conn,
        "memory_records",
        "supporting_task_ids_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;
    backfill_incident_patterns(&conn)?;

    Ok(())
}

pub fn persist_audit(paths: &WorkspaceAuditorPaths, audit: &AuditRecord) -> Result<()> {
    ensure_store(paths)?;
    let mut conn = Connection::open(&paths.db_path)
        .with_context(|| format!("failed to open {}", paths.db_path.display()))?;
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM raw_events WHERE session_id = ?1",
        params![audit.session_id],
    )?;
    tx.execute(
        "DELETE FROM intent_events WHERE session_id = ?1",
        params![audit.session_id],
    )?;
    tx.execute(
        "DELETE FROM action_events WHERE session_id = ?1",
        params![audit.session_id],
    )?;
    tx.execute(
        "DELETE FROM error_events WHERE session_id = ?1",
        params![audit.session_id],
    )?;
    tx.execute(
        "DELETE FROM claim_events WHERE session_id = ?1",
        params![audit.session_id],
    )?;
    tx.execute(
        "DELETE FROM audit_edges WHERE session_id = ?1",
        params![audit.session_id],
    )?;
    tx.execute(
        "DELETE FROM memory_records WHERE session_id = ?1 AND status = 'proposed'",
        params![audit.session_id],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO audit_sessions (
            session_id, workspace_id, workspace_root, provider, provider_display_name, session_name,
            session_path, audited_at, branch, task_type, primary_error_kind, incident_pattern, final_verdict,
            requested_goal, commit_verdict, push_verdict, merge_verdict, confidence,
            evidence_tier, task_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        params![
            audit.session_id,
            audit.workspace_id,
            audit.workspace_root,
            audit.provider,
            audit.provider_display_name,
            audit.session_name,
            audit.session_path,
            audit.audited_at.to_rfc3339(),
            audit.repo_probe.as_ref().and_then(|probe| probe.branch.clone()),
            task_type(audit),
            audit.errors.first().map(|error| error_kind_label(&error.kind)),
            audit.errors.first().and_then(|error| {
                incident_pattern_label(task_type(audit), &error.kind).map(str::to_string)
            }),
            serde_label(&audit.task_run.final_verdict),
            audit.task_run.requested_goal,
            serde_label(&audit.task_run.commit_verdict),
            serde_label(&audit.task_run.push_verdict),
            serde_label(&audit.task_run.merge_verdict),
            audit.task_run.confidence,
            i64::from(audit.task_run.evidence_tier),
            serde_json::to_string(&audit.task_run)?,
        ],
    )?;
    for event in &audit.raw_events {
        tx.execute(
            "INSERT OR REPLACE INTO raw_events (
                event_id, session_id, ordinal, actor, kind, timestamp, source_uri, source_span,
                raw_content, command, stdout, stderr, exit_code, content_hash
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                event.event_id,
                event.session_id,
                event.ordinal as i64,
                serde_label(&event.actor),
                serde_label(&event.kind),
                event.timestamp.map(|value| value.to_rfc3339()),
                event.source_uri,
                event.source_span,
                event.raw_content,
                event.command,
                event.stdout,
                event.stderr,
                event.exit_code,
                event.content_hash,
            ],
        )?;
    }
    for intent in &audit.intents {
        persist_intent_tx(&tx, intent, &audit.session_id)?;
    }
    for action in &audit.actions {
        persist_action_tx(&tx, action, &audit.session_id)?;
    }
    for error in &audit.errors {
        persist_error_tx(&tx, error, &audit.session_id)?;
    }
    for claim in &audit.task_run.assistant_claims {
        persist_claim_tx(&tx, claim, &audit.session_id)?;
    }
    for memory in &audit.proposed_memories {
        persist_memory_tx(&tx, paths, memory)?;
    }
    persist_edges_tx(&tx, audit)?;
    tx.commit()?;
    Ok(())
}

pub fn load_existing_memory_texts(paths: &WorkspaceAuditorPaths) -> Result<Vec<String>> {
    ensure_store(paths)?;
    let conn = Connection::open(&paths.db_path)?;
    let mut stmt = conn.prepare(
        "SELECT text FROM memory_records WHERE status IN ('approved', 'proposed') ORDER BY created_at",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.filter_map(Result::ok).collect())
}

pub fn list_session_memories(
    paths: &WorkspaceAuditorPaths,
    session_id: &str,
) -> Result<Vec<MemoryRecord>> {
    ensure_store(paths)?;
    let conn = Connection::open(&paths.db_path)?;
    let mut stmt = conn.prepare(
        "SELECT memory_id, session_id, status, kind, confidence, text, repo_path, branch,
                evidence_event_ids_json, supporting_task_ids_json, created_at, updated_at, status_reason
         FROM memory_records
         WHERE session_id = ?1
         ORDER BY created_at",
    )?;
    let rows = stmt.query_map(params![session_id], row_to_memory)?;
    let memories = read_memories(rows)?;
    drop(stmt);
    drop(conn);
    Ok(memories)
}

pub fn approve_memory(
    all_workspace_paths: &[WorkspaceAuditorPaths],
    memory_id: &str,
) -> Result<MemoryRecord> {
    mutate_memory_status(all_workspace_paths, memory_id, MemoryStatus::Approved, None)
}

pub fn retract_memory(
    all_workspace_paths: &[WorkspaceAuditorPaths],
    memory_id: &str,
    reason: Option<String>,
) -> Result<MemoryRecord> {
    mutate_memory_status(
        all_workspace_paths,
        memory_id,
        MemoryStatus::Retracted,
        reason,
    )
}

pub fn explain_task(paths: &WorkspaceAuditorPaths, task_id: &str) -> Result<Option<TaskRun>> {
    ensure_store(paths)?;
    let conn = Connection::open(&paths.db_path)?;
    let mut stmt = conn.prepare("SELECT task_json FROM audit_sessions WHERE session_id = ?1")?;
    let mut rows = stmt.query(params![task_id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let json = row.get::<_, String>(0)?;
    Ok(Some(serde_json::from_str(&json)?))
}

pub fn list_audit_summaries(paths: &WorkspaceAuditorPaths) -> Result<Vec<StoredAuditSummary>> {
    ensure_store(paths)?;
    let conn = Connection::open(&paths.db_path)?;
    let mut stmt = conn.prepare(
        "SELECT session_id, audited_at, branch, primary_error_kind, incident_pattern, task_json
         FROM audit_sessions
         ORDER BY audited_at",
    )?;
    let rows = stmt.query_map([], |row| {
        let task_json = row.get::<_, String>(5)?;
        let task_run = serde_json::from_str(&task_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                task_json.len(),
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
        Ok(StoredAuditSummary {
            session_id: row.get(0)?,
            audited_at: parse_time(row.get::<_, String>(1)?),
            branch: row.get(2)?,
            primary_error_kind: row
                .get::<_, Option<String>>(3)?
                .as_deref()
                .map(parse_error_kind_label),
            incident_pattern: row.get(4)?,
            task_run,
        })
    })?;
    let mut summaries = Vec::new();
    for row in rows {
        summaries.push(row?);
    }
    Ok(summaries)
}

pub fn list_report_file_names(
    paths: &WorkspaceAuditorPaths,
    branch: Option<&str>,
    task_type_filter: Option<&str>,
    error_kind: Option<&str>,
    session_id: Option<&str>,
    final_verdict: Option<&str>,
    incident_pattern: Option<&str>,
    audited_after: Option<&str>,
    audited_before: Option<&str>,
    failure_only: bool,
) -> Result<HashSet<String>> {
    ensure_store(paths)?;
    let conn = Connection::open(&paths.db_path)?;
    let mut stmt = conn.prepare(
        "SELECT session_id FROM audit_sessions
         WHERE (?1 IS NULL OR branch = ?1)
           AND (?2 IS NULL OR task_type = ?2)
           AND (?3 IS NULL OR primary_error_kind = ?3)
           AND (?4 IS NULL OR session_id = ?4)
           AND (?5 IS NULL OR final_verdict = ?5)
           AND (?6 IS NULL OR incident_pattern = ?6)
           AND (?7 IS NULL OR audited_at >= ?7)
           AND (?8 IS NULL OR audited_at <= ?8)
           AND (?9 = 0 OR final_verdict IN ('task_blocked_with_clear_fix', 'task_failed'))",
    )?;
    let rows = stmt.query_map(
        params![
            branch,
            task_type_filter,
            error_kind,
            session_id,
            final_verdict,
            incident_pattern,
            audited_after,
            audited_before,
            if failure_only { 1 } else { 0 }
        ],
        |row| row.get::<_, String>(0),
    )?;
    let mut file_names = HashSet::new();
    for row in rows {
        file_names.insert(format!("{}.md", row?));
    }
    Ok(file_names)
}

pub fn list_memory_file_names(
    paths: &WorkspaceAuditorPaths,
    branch: Option<&str>,
) -> Result<HashSet<String>> {
    ensure_store(paths)?;
    let conn = Connection::open(&paths.db_path)?;
    let mut stmt = conn.prepare(
        "SELECT memory_id FROM memory_records
         WHERE status = 'approved'
           AND (?1 IS NULL OR branch = ?1)",
    )?;
    let rows = stmt.query_map(params![branch], |row| row.get::<_, String>(0))?;
    let mut file_names = HashSet::new();
    for row in rows {
        file_names.insert(format!("{}.md", row?));
    }
    Ok(file_names)
}

pub fn write_memory_corpus_file(
    paths: &WorkspaceAuditorPaths,
    memory: &MemoryRecord,
) -> Result<()> {
    write_memory_corpus(paths, memory)
}

pub fn remove_memory_corpus_file(paths: &WorkspaceAuditorPaths, memory_id: &str) -> Result<()> {
    let path = memory_corpus_path(paths, memory_id);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("failed to remove {}", path.display())),
    }
}

pub fn list_workspace_paths(root: &Path) -> Result<Vec<WorkspaceAuditorPaths>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", root.display())),
    };
    let mut workspaces = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        workspaces.push(WorkspaceAuditorPaths {
            workspace_id: name.to_string(),
            workspace_root: PathBuf::new(),
            db_path: path.join(super::paths::AUDIT_DB_FILE_NAME),
            reports_dir: path.join(super::paths::REPORTS_DIR_NAME),
            memory_corpus_dir: path.join(super::paths::MEMORY_CORPUS_DIR_NAME),
            artifacts_dir: path.join(super::paths::ARTIFACTS_DIR_NAME),
            root_dir: path,
        });
    }
    Ok(workspaces)
}

pub fn corpus_hit_to_json(hit: &CorpusSearchHit) -> serde_json::Value {
    json!({
        "workspace_root": hit.workspace_root,
        "workspace_id": hit.workspace_id,
        "file_path": hit.file_path,
        "start_line": hit.start_line,
        "end_line": hit.end_line,
        "score": hit.score,
        "snippet": hit.snippet,
    })
}

fn mutate_memory_status(
    all_workspace_paths: &[WorkspaceAuditorPaths],
    memory_id: &str,
    status: MemoryStatus,
    reason: Option<String>,
) -> Result<MemoryRecord> {
    for paths in all_workspace_paths {
        if !paths.db_path.is_file() {
            continue;
        }
        let mut conn = Connection::open(&paths.db_path)?;
        let tx = conn.transaction()?;
        let mut stmt = tx.prepare(
            "SELECT memory_id, session_id, status, kind, confidence, text, repo_path, branch,
                    evidence_event_ids_json, supporting_task_ids_json, created_at, updated_at, status_reason
             FROM memory_records WHERE memory_id = ?1",
        )?;
        let mut rows = stmt.query(params![memory_id])?;
        let Some(row) = rows.next()? else {
            continue;
        };
        let mut memory = row_to_memory(row)?;
        drop(rows);
        drop(stmt);
        memory.status = status.clone();
        memory.updated_at = Utc::now();
        memory.status_reason = reason.clone();
        tx.execute(
            "UPDATE memory_records SET status = ?2, updated_at = ?3, status_reason = ?4 WHERE memory_id = ?1",
            params![
                memory_id,
                format!("{:?}", memory.status).to_ascii_lowercase(),
                memory.updated_at.to_rfc3339(),
                memory.status_reason,
            ],
        )?;
        match status {
            MemoryStatus::Approved => {
                supersede_weaker_memories(&tx, paths, &memory)?;
                write_memory_corpus_file(paths, &memory)?;
            }
            MemoryStatus::Retracted => remove_memory_corpus_file(paths, memory_id)?,
            MemoryStatus::Proposed => {}
        }
        tx.commit()?;
        return Ok(memory);
    }
    anyhow::bail!("No stored history-auditor memory found for `{memory_id}`")
}

fn supersede_weaker_memories(
    tx: &rusqlite::Transaction<'_>,
    paths: &WorkspaceAuditorPaths,
    approved_memory: &MemoryRecord,
) -> Result<()> {
    let mut stmt = tx.prepare(
        "SELECT memory_id, session_id, status, kind, confidence, text, repo_path, branch,
                evidence_event_ids_json, supporting_task_ids_json, created_at, updated_at, status_reason
         FROM memory_records
         WHERE memory_id != ?1
           AND kind = ?2
           AND ((repo_path = ?3) OR (repo_path IS NULL AND ?3 IS NULL))
           AND status IN ('approved', 'proposed')
         ORDER BY created_at",
    )?;
    let rows = stmt.query_map(
        params![
            approved_memory.memory_id,
            serde_label(&approved_memory.kind),
            approved_memory.repo_path.clone(),
        ],
        row_to_memory,
    )?;
    let siblings = read_memories(rows)?;
    drop(stmt);

    for sibling in siblings {
        if !should_supersede_memory(approved_memory, &sibling) {
            continue;
        }
        let prior_status = sibling.status.clone();
        let reason = format!("superseded by {}", approved_memory.memory_id);
        tx.execute(
            "UPDATE memory_records SET status = 'retracted', updated_at = ?2, status_reason = ?3 WHERE memory_id = ?1",
            params![
                sibling.memory_id,
                approved_memory.updated_at.to_rfc3339(),
                reason,
            ],
        )?;
        if prior_status == MemoryStatus::Approved {
            remove_memory_corpus_file(paths, &sibling.memory_id)?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO audit_edges (
                edge_id, session_id, edge_kind, src_kind, src_id, dst_kind, dst_id, detail
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                format!(
                    "{}-supersedes-{}",
                    approved_memory.memory_id, sibling.memory_id
                ),
                approved_memory.session_id,
                "supersedes",
                "memory",
                approved_memory.memory_id,
                "memory",
                sibling.memory_id,
                Some(serde_label(&approved_memory.kind)),
            ],
        )?;
    }

    Ok(())
}

fn should_supersede_memory(approved_memory: &MemoryRecord, sibling: &MemoryRecord) -> bool {
    if approved_memory.kind != sibling.kind {
        return false;
    }
    if approved_memory.repo_path != sibling.repo_path {
        return false;
    }
    let approved_text = approved_memory.text.trim();
    let sibling_text = sibling.text.trim();
    let stronger_support =
        approved_memory.supporting_task_ids.len() > sibling.supporting_task_ids.len();
    let stronger_confidence = approved_memory.confidence > sibling.confidence;
    approved_text != sibling_text
        && approved_text.contains(sibling_text)
        && (stronger_support || stronger_confidence)
}

fn ensure_column(conn: &Connection, table: &str, column: &str, declaration: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut present = false;
    for row in rows {
        if row? == column {
            present = true;
            break;
        }
    }
    if !present {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {declaration}"),
            [],
        )?;
    }
    Ok(())
}

fn backfill_incident_patterns(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT session_id, task_type, primary_error_kind
         FROM audit_sessions
         WHERE primary_error_kind IS NOT NULL
           AND (incident_pattern IS NULL OR incident_pattern = '')",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut updates = Vec::new();
    for row in rows {
        let (session_id, task_type, error_kind) = row?;
        let Some(pattern) =
            incident_pattern_label(&task_type, &parse_error_kind_label(&error_kind))
        else {
            continue;
        };
        updates.push((session_id, pattern.to_string()));
    }
    drop(stmt);
    for (session_id, pattern) in updates {
        conn.execute(
            "UPDATE audit_sessions SET incident_pattern = ?2 WHERE session_id = ?1",
            params![session_id, pattern],
        )?;
    }
    Ok(())
}

fn parse_time(value: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn parse_error_kind_label(value: &str) -> ErrorKind {
    match value {
        "non_fast_forward" => ErrorKind::NonFastForward,
        "auth_failed" => ErrorKind::AuthFailed,
        "protected_branch" => ErrorKind::ProtectedBranch,
        "pre_receive_hook_declined" => ErrorKind::PreReceiveHookDeclined,
        "remote_not_found" => ErrorKind::RemoteNotFound,
        "no_upstream_branch" => ErrorKind::NoUpstreamBranch,
        "merge_conflict" => ErrorKind::MergeConflict,
        "rebase_conflict" => ErrorKind::RebaseConflict,
        "detached_head" => ErrorKind::DetachedHead,
        "nothing_to_commit" => ErrorKind::NothingToCommit,
        "dirty_worktree" => ErrorKind::DirtyWorktree,
        "test_failure" => ErrorKind::TestFailure,
        "dependency_failure" => ErrorKind::DependencyFailure,
        "permission_denied" => ErrorKind::PermissionDenied,
        _ => ErrorKind::Unknown,
    }
}
