use chrono::{TimeZone, Utc};
use rusqlite::{params, Connection};
use std::fs;

use super::paths;
use super::schema::ArtifactKind;
use super::service::{AuditArtifactRequest, HistoryAuditorService};
use super::test_support::{
    file_name, init_git_repo_with_branch, run_git, temp_dir, HistoryAuditorTestGuard,
};
use crate::session_mcp::service::SessionMcpService;

fn set_audited_at(workspace: &std::path::Path, session_id: &str, audited_at: &str) {
    let paths = paths::ensure_workspace_layout(workspace).unwrap();
    let conn = Connection::open(&paths.db_path).unwrap();
    conn.execute(
        "UPDATE audit_sessions SET audited_at = ?2 WHERE session_id = ?1",
        params![session_id, audited_at],
    )
    .unwrap();
}

#[test]
fn audit_history_honors_session_and_final_verdict_filters() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("history-session-verdict");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/history-session-verdict");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-history-failure".to_string()),
            source_uri: Some("notes/history-failure.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Push the release branch.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"non-fast-forward\"}"
                    .to_string(),
            ),
        })
        .unwrap();
    set_audited_at(
        &workspace,
        "artifact-history-failure",
        &Utc.with_ymd_and_hms(2026, 5, 22, 10, 0, 0)
            .single()
            .unwrap()
            .to_rfc3339(),
    );
    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-history-success".to_string()),
            source_uri: Some("notes/history-success.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Run the release verification tests.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"cargo test --release\",\"exit_code\":0,\"stdout\":\"release verification passed\"}"
                    .to_string(),
            ),
        })
        .unwrap();
    set_audited_at(
        &workspace,
        "artifact-history-success",
        &Utc.with_ymd_and_hms(2026, 5, 23, 10, 0, 0)
            .single()
            .unwrap()
            .to_rfc3339(),
    );

    let failure_only = service
        .audit_history(
            "release",
            Some(&workspace.display().to_string()),
            None,
            None,
            None,
            Some("task_blocked_with_clear_fix"),
            None,
            None,
            None,
        )
        .unwrap();
    assert!(failure_only.hit_count >= 1);
    assert!(failure_only
        .hits
        .iter()
        .all(|hit| file_name(&hit.file_path) == "artifact-history-failure.md"));

    let exact_session = service
        .audit_history(
            "release",
            Some(&workspace.display().to_string()),
            None,
            None,
            Some("artifact-history-success"),
            Some("task_completed"),
            None,
            None,
            None,
        )
        .unwrap();
    assert!(exact_session.hit_count >= 1);
    assert!(exact_session
        .hits
        .iter()
        .all(|hit| file_name(&hit.file_path) == "artifact-history-success.md"));

    let timestamp_window = service
        .audit_history(
            "release",
            Some(&workspace.display().to_string()),
            None,
            None,
            None,
            None,
            None,
            Some("2026-05-23T00:00:00Z"),
            Some("2026-05-23T23:59:59Z"),
        )
        .unwrap();
    assert!(timestamp_window.hit_count >= 1);
    assert!(timestamp_window
        .hits
        .iter()
        .all(|hit| file_name(&hit.file_path) == "artifact-history-success.md"));

    let pattern_window = service
        .audit_history(
            "release",
            Some(&workspace.display().to_string()),
            None,
            Some("git_delivery"),
            None,
            None,
            Some("git_delivery_push_non_fast_forward"),
            None,
            None,
        )
        .unwrap();
    assert!(pattern_window.hit_count >= 1);
    assert!(pattern_window
        .hits
        .iter()
        .all(|hit| { file_name(&hit.file_path) == "artifact-history-failure.md" }));
}

#[test]
fn find_similar_failures_honors_branch_session_and_failure_filters() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("similar-failure-filters");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/similar-failure");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-similar-feature".to_string()),
            source_uri: Some("notes/similar-feature.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Push the feature branch.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"non-fast-forward\"}"
                    .to_string(),
            ),
        })
        .unwrap();
    set_audited_at(
        &workspace,
        "artifact-similar-feature",
        &Utc.with_ymd_and_hms(2026, 5, 22, 9, 0, 0)
            .single()
            .unwrap()
            .to_rfc3339(),
    );

    run_git(
        &workspace,
        &["git", "checkout", "-B", "release/similar-failure"],
    );
    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-similar-release".to_string()),
            source_uri: Some("notes/similar-release.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Push the release branch.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"non-fast-forward\"}"
                    .to_string(),
            ),
        })
        .unwrap();
    set_audited_at(
        &workspace,
        "artifact-similar-release",
        &Utc.with_ymd_and_hms(2026, 5, 24, 9, 0, 0)
            .single()
            .unwrap()
            .to_rfc3339(),
    );
    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-similar-complete".to_string()),
            source_uri: Some("notes/similar-complete.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Investigate the old non-fast-forward incident and rerun tests.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"cargo test --release\",\"exit_code\":0,\"stdout\":\"tests passed\"}"
                    .to_string(),
            ),
        })
        .unwrap();
    set_audited_at(
        &workspace,
        "artifact-similar-complete",
        &Utc.with_ymd_and_hms(2026, 5, 25, 9, 0, 0)
            .single()
            .unwrap()
            .to_rfc3339(),
    );

    let branch_hits = service
        .find_similar_failures(
            "non-fast-forward push failure",
            Some(&workspace.display().to_string()),
            Some("feature/similar-failure"),
            Some("git_delivery"),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
    assert!(branch_hits.hit_count >= 1);
    assert!(branch_hits
        .hits
        .iter()
        .all(|hit| file_name(&hit.file_path) == "artifact-similar-feature.md"));

    let exact_session = service
        .find_similar_failures(
            "non-fast-forward push failure",
            Some(&workspace.display().to_string()),
            None,
            Some("git_delivery"),
            Some("artifact-similar-release"),
            Some("task_blocked_with_clear_fix"),
            None,
            None,
            None,
        )
        .unwrap();
    assert!(exact_session.hit_count >= 1);
    assert!(exact_session
        .hits
        .iter()
        .all(|hit| file_name(&hit.file_path) == "artifact-similar-release.md"));

    let completed_only = service
        .find_similar_failures(
            "non-fast-forward push failure",
            Some(&workspace.display().to_string()),
            None,
            None,
            None,
            Some("task_completed"),
            None,
            None,
            None,
        )
        .unwrap();
    assert_eq!(completed_only.hit_count, 0);

    let timestamp_window = service
        .find_similar_failures(
            "non-fast-forward push failure",
            Some(&workspace.display().to_string()),
            None,
            Some("git_delivery"),
            None,
            None,
            None,
            Some("2026-05-24T00:00:00Z"),
            Some("2026-05-24T23:59:59Z"),
        )
        .unwrap();
    assert!(timestamp_window.hit_count >= 1);
    assert!(timestamp_window
        .hits
        .iter()
        .all(|hit| file_name(&hit.file_path) == "artifact-similar-release.md"));

    let explicit_pattern = service
        .find_similar_failures(
            "push failure",
            Some(&workspace.display().to_string()),
            Some("release/similar-failure"),
            Some("git_delivery"),
            None,
            None,
            Some("git_delivery_push_non_fast_forward"),
            None,
            None,
        )
        .unwrap();
    assert!(explicit_pattern.hit_count >= 1);
    assert!(explicit_pattern
        .hits
        .iter()
        .all(|hit| file_name(&hit.file_path) == "artifact-similar-release.md"));
}

#[test]
fn find_similar_failures_infers_nothing_to_commit_incident_pattern() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("similar-nothing-to-commit");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/similar-nothing-to-commit");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-similar-nothing-delivery".to_string()),
            source_uri: Some("notes/similar-nothing-delivery.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Commit the release metadata changes.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git commit -m \\\"Release metadata\\\"\",\"exit_code\":1,\"stderr\":\"nothing to commit, working tree clean\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-similar-nothing-unknown".to_string()),
            source_uri: Some("notes/similar-nothing-unknown.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Investigate the last failed release note.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"exit_code\":1,\"stderr\":\"nothing to commit, working tree clean\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    let response = service
        .find_similar_failures(
            "nothing to commit during the commit step",
            Some(&workspace.display().to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

    assert_eq!(
        response.incident_pattern.as_deref(),
        Some("git_delivery_nothing_to_commit")
    );
    assert!(response.hit_count >= 1);
    assert!(response
        .hits
        .iter()
        .all(|hit| { file_name(&hit.file_path) == "artifact-similar-nothing-delivery.md" }));
}

#[test]
fn find_similar_failures_infers_dirty_worktree_incident_pattern() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("similar-dirty-worktree");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/similar-dirty-worktree");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-similar-dirty-delivery".to_string()),
            source_uri: Some("notes/similar-dirty-delivery.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Push the release branch after checking the worktree.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin HEAD\",\"exit_code\":1,\"stderr\":\"cannot push: dirty worktree\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-similar-dirty-generic".to_string()),
            source_uri: Some("notes/similar-dirty-generic.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Investigate why the release commit did not produce a delta.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git commit -m \\\"release notes\\\"\",\"exit_code\":1,\"stderr\":\"nothing to commit, working tree clean\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    let response = service
        .find_similar_failures(
            "working tree is dirty before the push step",
            Some(&workspace.display().to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

    assert_eq!(
        response.incident_pattern.as_deref(),
        Some("git_delivery_dirty_worktree")
    );
    assert!(response.hit_count >= 1);
    assert!(response
        .hits
        .iter()
        .all(|hit| file_name(&hit.file_path) == "artifact-similar-dirty-delivery.md"));
}
