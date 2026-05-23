use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::Serialize;
use std::collections::HashSet;
use super::schema::ActionKind;
use super::schema::AuditRecord;
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
