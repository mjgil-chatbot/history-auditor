use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::Serialize;
use std::collections::HashSet;

use super::paths::{memory_corpus_path, WorkspaceAuditorPaths};
use super::schema::{
    ActionEvent, ActionKind, AuditRecord, ClaimEvent, ErrorEvent, ErrorKind, IntentEvent,
    MemoryRecord, MemoryStatus,
};
use super::util::atomic_write;

pub fn persist_intent_tx(
    tx: &rusqlite::Transaction<'_>,
    intent: &IntentEvent,
    session_id: &str,
) -> Result<()> {
    tx.execute(
        "INSERT OR REPLACE INTO intent_events (
            intent_id, session_id, kind, confidence, evidence_event_ids_json
        ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            intent.intent_id,
            session_id,
            serde_label(&intent.kind),
            intent.confidence,
            serde_json::to_string(&intent.evidence_event_ids)?,
        ],
    )?;
    Ok(())
}

pub fn persist_action_tx(
    tx: &rusqlite::Transaction<'_>,
    action: &ActionEvent,
    session_id: &str,
) -> Result<()> {
    tx.execute(
        "INSERT OR REPLACE INTO action_events (
            action_id, session_id, kind, status, command, exit_code, stdout_excerpt,
            stderr_excerpt, commit_hash, evidence_event_ids_json, evidence_tier
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            action.action_id,
            session_id,
            serde_label(&action.kind),
            serde_label(&action.status),
            action.command,
            action.exit_code,
            action.stdout_excerpt,
            action.stderr_excerpt,
            action.commit_hash,
            serde_json::to_string(&action.evidence_event_ids)?,
            i64::from(action.evidence_tier),
        ],
    )?;
    Ok(())
}

pub fn persist_error_tx(
    tx: &rusqlite::Transaction<'_>,
    error: &ErrorEvent,
    session_id: &str,
) -> Result<()> {
    tx.execute(
        "INSERT OR REPLACE INTO error_events (
            error_id, session_id, kind, message, evidence_event_ids_json, evidence_tier
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            error.error_id,
            session_id,
            serde_label(&error.kind),
            error.message,
            serde_json::to_string(&error.evidence_event_ids)?,
            i64::from(error.evidence_tier),
        ],
    )?;
    Ok(())
}

pub fn persist_claim_tx(
    tx: &rusqlite::Transaction<'_>,
    claim: &ClaimEvent,
    session_id: &str,
) -> Result<()> {
    tx.execute(
        "INSERT OR REPLACE INTO claim_events (
            claim_id, session_id, kind, summary, confidence, evidence_event_ids_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            claim.claim_id,
            session_id,
            serde_label(&claim.kind),
            claim.summary,
            claim.confidence,
            serde_json::to_string(&claim.evidence_event_ids)?,
        ],
    )?;
    Ok(())
}

pub fn persist_memory_tx(
    tx: &rusqlite::Transaction<'_>,
    paths: &WorkspaceAuditorPaths,
    memory: &MemoryRecord,
) -> Result<()> {
    tx.execute(
        "INSERT OR REPLACE INTO memory_records (
            memory_id, workspace_id, session_id, status, kind, confidence, text, repo_path, branch,
            evidence_event_ids_json, supporting_task_ids_json, created_at, updated_at, status_reason
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            memory.memory_id,
            paths.workspace_id,
            memory.session_id,
            serde_label(&memory.status),
            serde_label(&memory.kind),
            memory.confidence,
            memory.text,
            memory.repo_path.clone(),
            memory.branch.clone(),
            serde_json::to_string(&memory.evidence_event_ids)?,
            serde_json::to_string(&memory.supporting_task_ids)?,
            memory.created_at.to_rfc3339(),
            memory.updated_at.to_rfc3339(),
            memory.status_reason.clone(),
        ],
    )?;
    Ok(())
}

pub fn persist_edges_tx(tx: &rusqlite::Transaction<'_>, audit: &AuditRecord) -> Result<()> {
    let task_node = format!("task:{}", audit.task_run.task_id);
    let mut similar_task_edges = HashSet::new();
    for intent in &audit.intents {
        persist_edge_tx(
            tx,
            &edge_id(
                &audit.session_id,
                "requested",
                "task",
                &task_node,
                "intent",
                &intent.intent_id,
            ),
            &audit.session_id,
            "requested",
            "task",
            &task_node,
            "intent",
            &intent.intent_id,
            Some(serde_label(&intent.kind)),
        )?;
    }

    for action in &audit.actions {
        persist_edge_tx(
            tx,
            &edge_id(
                &audit.session_id,
                "attempted",
                "task",
                &task_node,
                "action",
                &action.action_id,
            ),
            &audit.session_id,
            "attempted",
            "task",
            &task_node,
            "action",
            &action.action_id,
            Some(serde_label(&action.kind)),
        )?;
    }

    for error in &audit.errors {
        persist_edge_tx(
            tx,
            &edge_id(
                &audit.session_id,
                "failed_with",
                "task",
                &task_node,
                "error",
                &error.error_id,
            ),
            &audit.session_id,
            "failed_with",
            "task",
            &task_node,
            "error",
            &error.error_id,
            Some(error_kind_label(&error.kind)),
        )?;
        if let Some(recommended_fix) = &audit.task_run.recommended_fix {
            let fix_id = format!("fix:{}", audit.session_id);
            persist_edge_tx(
                tx,
                &edge_id(
                    &audit.session_id,
                    "fixed_by",
                    "error",
                    &error.error_id,
                    "fix",
                    &fix_id,
                ),
                &audit.session_id,
                "fixed_by",
                "error",
                &error.error_id,
                "fix",
                &fix_id,
                Some(recommended_fix.clone()),
            )?;
        }
    }

    for claim in &audit.task_run.assistant_claims {
        persist_edge_tx(
            tx,
            &edge_id(
                &audit.session_id,
                "claimed",
                "task",
                &task_node,
                "claim",
                &claim.claim_id,
            ),
            &audit.session_id,
            "claimed",
            "task",
            &task_node,
            "claim",
            &claim.claim_id,
            Some(serde_label(&claim.kind)),
        )?;
    }

    for memory in &audit.proposed_memories {
        persist_edge_tx(
            tx,
            &edge_id(
                &audit.session_id,
                "produced_memory",
                "task",
                &task_node,
                "memory",
                &memory.memory_id,
            ),
            &audit.session_id,
            "produced_memory",
            "task",
            &task_node,
            "memory",
            &memory.memory_id,
            Some(serde_label(&memory.kind)),
        )?;
        for supporting_task_id in &memory.supporting_task_ids {
            if !similar_task_edges.insert(supporting_task_id.clone()) {
                continue;
            }
            let supporting_task_node = format!("task:{supporting_task_id}");
            persist_edge_tx(
                tx,
                &edge_id(
                    &audit.session_id,
                    "similar_to",
                    "task",
                    &task_node,
                    "task",
                    &supporting_task_node,
                ),
                &audit.session_id,
                "similar_to",
                "task",
                &task_node,
                "task",
                &supporting_task_node,
                Some(serde_label(&memory.kind)),
            )?;
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn persist_edge_tx(
    tx: &rusqlite::Transaction<'_>,
    edge_id_value: &str,
    session_id: &str,
    edge_kind: &str,
    src_kind: &str,
    src_id: &str,
    dst_kind: &str,
    dst_id: &str,
    detail: Option<String>,
) -> Result<()> {
    tx.execute(
        "INSERT OR REPLACE INTO audit_edges (
            edge_id, session_id, edge_kind, src_kind, src_id, dst_kind, dst_id, detail
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            edge_id_value,
            session_id,
            edge_kind,
            src_kind,
            src_id,
            dst_kind,
            dst_id,
            detail
        ],
    )?;
    Ok(())
}

pub fn row_to_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecord> {
    let status = parse_memory_status(&row.get::<_, String>(2)?);
    let kind = parse_memory_kind(&row.get::<_, String>(3)?);
    let evidence_json = row.get::<_, String>(8)?;
    let supporting_task_ids_json = row.get::<_, String>(9)?;
    let created_at = parse_time(row.get::<_, String>(10)?);
    let updated_at = parse_time(row.get::<_, String>(11)?);
    Ok(MemoryRecord {
        memory_id: row.get(0)?,
        session_id: row.get(1)?,
        task_id: row.get::<_, String>(1)?,
        status,
        kind,
        confidence: row.get(4)?,
        text: row.get(5)?,
        repo_path: row.get(6)?,
        branch: row.get(7)?,
        evidence_event_ids: serde_json::from_str(&evidence_json).unwrap_or_default(),
        supporting_task_ids: serde_json::from_str(&supporting_task_ids_json).unwrap_or_default(),
        created_at,
        updated_at,
        status_reason: row.get(12)?,
    })
}

pub fn read_memories(
    rows: rusqlite::MappedRows<
        '_,
        impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecord>,
    >,
) -> Result<Vec<MemoryRecord>> {
    let mut memories = Vec::new();
    for row in rows {
        memories.push(row?);
    }
    Ok(memories)
}

pub fn write_memory_corpus(paths: &WorkspaceAuditorPaths, memory: &MemoryRecord) -> Result<()> {
    let path = memory_corpus_path(paths, &memory.memory_id);
    let markdown = format!(
        "# History Auditor Memory\n\n- Memory id: `{}`\n- Status: `{:?}`\n- Kind: `{:?}`\n- Confidence: `{:.2}`\n- Session: `{}`\n- Branch: `{}`\n- Supporting tasks: `{}`\n\n{}\n",
        memory.memory_id,
        memory.status,
        memory.kind,
        memory.confidence,
        memory.session_id,
        memory.branch.clone().unwrap_or_default(),
        memory.supporting_task_ids.join("`, `"),
        memory.text
    );
    atomic_write(&path, markdown.as_bytes())
}

pub fn edge_id(
    session_id: &str,
    edge_kind: &str,
    src_kind: &str,
    src_id: &str,
    dst_kind: &str,
    dst_id: &str,
) -> String {
    format!("{session_id}:{edge_kind}:{src_kind}:{src_id}:{dst_kind}:{dst_id}")
}

pub fn task_type(audit: &AuditRecord) -> &'static str {
    let has_git = audit.actions.iter().any(|action| {
        matches!(
            action.kind,
            ActionKind::GitStatus
                | ActionKind::GitAdd
                | ActionKind::GitCommit
                | ActionKind::GitPush
                | ActionKind::GitPull
                | ActionKind::GitFetch
                | ActionKind::GitMerge
                | ActionKind::GitRebase
                | ActionKind::GitCheckout
                | ActionKind::GitLog
        )
    });
    let has_tests = audit
        .actions
        .iter()
        .any(|action| matches!(action.kind, ActionKind::RunTests));
    let has_edits = audit
        .actions
        .iter()
        .any(|action| matches!(action.kind, ActionKind::EditFile));

    if has_git {
        "git_delivery"
    } else if has_tests {
        "test_run"
    } else if has_edits {
        "edit_file"
    } else if audit
        .task_run
        .requested_goal
        .to_ascii_lowercase()
        .contains("investigat")
    {
        "investigation"
    } else {
        "unknown"
    }
}

pub fn error_kind_label(kind: &ErrorKind) -> String {
    match kind {
        ErrorKind::NonFastForward => "non_fast_forward",
        ErrorKind::AuthFailed => "auth_failed",
        ErrorKind::ProtectedBranch => "protected_branch",
        ErrorKind::PreReceiveHookDeclined => "pre_receive_hook_declined",
        ErrorKind::RemoteNotFound => "remote_not_found",
        ErrorKind::NoUpstreamBranch => "no_upstream_branch",
        ErrorKind::MergeConflict => "merge_conflict",
        ErrorKind::RebaseConflict => "rebase_conflict",
        ErrorKind::DetachedHead => "detached_head",
        ErrorKind::NothingToCommit => "nothing_to_commit",
        ErrorKind::DirtyWorktree => "dirty_worktree",
        ErrorKind::TestFailure => "test_failure",
        ErrorKind::DependencyFailure => "dependency_failure",
        ErrorKind::PermissionDenied => "permission_denied",
        ErrorKind::Unknown => "unknown",
    }
    .to_string()
}

pub fn serde_label<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|json| json.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

fn parse_memory_status(value: &str) -> MemoryStatus {
    match value {
        "approved" => MemoryStatus::Approved,
        "retracted" => MemoryStatus::Retracted,
        _ => MemoryStatus::Proposed,
    }
}

fn parse_memory_kind(value: &str) -> super::schema::MemoryKind {
    match value {
        "procedural" => super::schema::MemoryKind::Procedural,
        "semantic_fact" => super::schema::MemoryKind::SemanticFact,
        "anti_pattern" => super::schema::MemoryKind::AntiPattern,
        "repo_fact" => super::schema::MemoryKind::RepoFact,
        "user_preference" => super::schema::MemoryKind::UserPreference,
        _ => super::schema::MemoryKind::Episodic,
    }
}

fn parse_time(value: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
