use anyhow::{Context, Result};

use super::paths::{report_path, WorkspaceAuditorPaths};
use super::schema::{AuditRecord, MemoryRecord};
use super::util::atomic_write;

pub fn write_report(paths: &WorkspaceAuditorPaths, audit: &AuditRecord) -> Result<String> {
    let path = report_path(paths, &audit.session_id);
    let markdown = render_markdown_report(audit);
    atomic_write(&path, markdown.as_bytes())
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path.display().to_string())
}

pub fn render_markdown_report(audit: &AuditRecord) -> String {
    let mut out = String::new();
    out.push_str("# History Audit Report\n\n");
    out.push_str(&format!("- Session: `{}`\n", audit.session_id));
    out.push_str(&format!("- Provider: `{}`\n", audit.provider));
    out.push_str(&format!("- Workspace: `{}`\n", audit.workspace_root));
    out.push_str(&format!(
        "- Audited at: `{}`\n\n",
        audit.audited_at.to_rfc3339()
    ));

    out.push_str("## Verdict\n\n");
    out.push_str(&format!(
        "- Final verdict: `{:?}`\n- Commit verdict: `{:?}`\n- Push verdict: `{:?}`\n- Merge verdict: `{:?}`\n- Confidence: `{:.2}`\n\n",
        audit.task_run.final_verdict,
        audit.task_run.commit_verdict,
        audit.task_run.push_verdict,
        audit.task_run.merge_verdict,
        audit.task_run.confidence
    ));

    out.push_str("## Request\n\n");
    out.push_str(&format!("{}\n\n", audit.task_run.requested_goal));
    if !audit.task_run.requested_goal_evidence_event_ids.is_empty() {
        out.push_str(&format!(
            "Evidence: `{}`\n\n",
            audit
                .task_run
                .requested_goal_evidence_event_ids
                .join("`, `")
        ));
    }

    if !audit.task_run.intent_notes.is_empty() {
        out.push_str("## Intent Notes\n\n");
        for note in &audit.task_run.intent_notes {
            out.push_str(&format!(
                "- `{:.2}` {} (evidence: `{}`)\n",
                note.confidence,
                note.summary,
                note.evidence_event_ids.join("`, `")
            ));
        }
        out.push('\n');
    }

    if !audit.task_run.assistant_claims.is_empty() {
        out.push_str("## Assistant Claims\n\n");
        for claim in &audit.task_run.assistant_claims {
            out.push_str(&format!(
                "- `{}` `{:?}` `{:.2}` {} (evidence: `{}`)\n",
                claim.claim_id,
                claim.kind,
                claim.confidence,
                claim.summary,
                claim.evidence_event_ids.join("`, `")
            ));
        }
        out.push('\n');
    }

    if let Some(root_cause) = &audit.task_run.root_cause {
        out.push_str("## Root Cause\n\n");
        out.push_str(root_cause);
        if !audit.task_run.root_cause_evidence_event_ids.is_empty() {
            out.push_str(&format!(
                "\n\nEvidence: `{}`\n\n",
                audit.task_run.root_cause_evidence_event_ids.join("`, `")
            ));
        } else {
            out.push_str("\n\n");
        }
    }

    if let Some(recommended_fix) = &audit.task_run.recommended_fix {
        out.push_str("## Recommended Fix\n\n");
        out.push_str(recommended_fix);
        if !audit.task_run.recommended_fix_evidence_event_ids.is_empty() {
            out.push_str(&format!(
                "\n\nEvidence: `{}`\n\n",
                audit
                    .task_run
                    .recommended_fix_evidence_event_ids
                    .join("`, `")
            ));
        } else {
            out.push_str("\n\n");
        }
    }

    if !audit.errors.is_empty() {
        out.push_str("## Errors\n\n");
        for error in &audit.errors {
            out.push_str(&format!(
                "- `{}` from `{}`: {}\n",
                error.error_id,
                error.evidence_event_ids.join(", "),
                error.message
            ));
        }
        out.push('\n');
    }

    if !audit.proposed_memories.is_empty() {
        out.push_str("## Proposed Memories\n\n");
        for memory in &audit.proposed_memories {
            append_memory(&mut out, memory);
        }
    }

    out.push_str("## Evidence\n\n");
    for event in &audit.raw_events {
        let snippet = event
            .raw_content
            .trim()
            .lines()
            .take(5)
            .collect::<Vec<_>>()
            .join("\n");
        out.push_str(&format!(
            "- `{}` `{}` `{}` {}\n",
            event.event_id,
            format!("{:?}", event.actor).to_ascii_lowercase(),
            format!("{:?}", event.kind).to_ascii_lowercase(),
            snippet
        ));
    }
    out
}

fn append_memory(out: &mut String, memory: &MemoryRecord) {
    out.push_str(&format!(
        "- `{}` `{:?}` `{:.2}` {} (evidence: `{}`; supporting tasks: `{}`)\n",
        memory.memory_id,
        memory.kind,
        memory.confidence,
        memory.text,
        memory.evidence_event_ids.join("`, `"),
        memory.supporting_task_ids.join("`, `")
    ));
}
