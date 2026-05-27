use std::fs;

use super::schema::{ArtifactKind, ClaimKind, CommitVerdict, MergeVerdict, TaskVerdict};
use super::service::{AuditArtifactRequest, HistoryAuditorService};
use super::test_support::{init_git_repo_with_branch, run_git, temp_dir, HistoryAuditorTestGuard};
use crate::session_mcp::service::SessionMcpService;

#[test]
fn failed_action_without_normalized_error_uses_action_root_cause_and_blocks_task() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("structured-failed-action");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/structured-failed-action");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-structured-failed-action".to_string()),
            source_uri: Some("notes/structured-failed-action.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Push the release branch and tell me why it failed.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"remote policy requires signed commits\"}\n{\"actor\":\"assistant\",\"kind\":\"message\",\"content\":\"Next step: sign the commit and retry git push origin main.\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    assert_eq!(
        response.task_run.final_verdict,
        TaskVerdict::TaskBlockedWithClearFix
    );
    assert!(response
        .task_run
        .root_cause
        .as_deref()
        .is_some_and(|root| root.contains("signed commits")));
    assert!(!response.task_run.root_cause_evidence_event_ids.is_empty());
    assert!(response
        .task_run
        .recommended_fix
        .as_deref()
        .is_some_and(|fix| fix.contains("sign the commit")));
    assert!(!response
        .task_run
        .recommended_fix_evidence_event_ids
        .is_empty());
}

#[test]
fn contradictory_uncertainty_claim_keeps_success_shaped_task_ambiguous() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("contradictory-uncertainty");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/contradictory-uncertainty");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-contradictory-uncertainty".to_string()),
            source_uri: Some("notes/contradictory-uncertainty.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Push the branch and confirm whether it worked.\"}\n{\"actor\":\"assistant\",\"kind\":\"message\",\"content\":\"I need more information before I can confirm completion.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":0,\"stdout\":\"main -> main\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    assert!(response
        .task_run
        .assistant_claims
        .iter()
        .any(|claim| claim.kind == ClaimKind::NeedsMoreInformation));
    assert_eq!(
        response.task_run.final_verdict,
        TaskVerdict::AmbiguousMissingEvidence
    );
    assert!(response.task_run.missing_evidence.iter().any(|entry| {
        entry == "Assistant blockage or uncertainty claims conflicted with captured successful push evidence."
    }));
}

#[test]
fn exit_zero_push_without_remote_update_keeps_task_ambiguous() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("ambiguous-push-success");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/ambiguous-push-success");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-ambiguous-push-success".to_string()),
            source_uri: Some("notes/ambiguous-push-success.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Push the branch and confirm whether it worked.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":0,\"stdout\":\"To github.com:mjgil/example.git\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    assert_eq!(
        response.task_run.final_verdict,
        TaskVerdict::AmbiguousMissingEvidence
    );
    assert_eq!(
        response.task_run.push_verdict,
        super::schema::PushVerdict::RemotePushUnknown
    );
    assert!(response.task_run.missing_evidence.iter().any(|entry| {
        entry == "Captured git push activity did not include decisive remote update evidence."
    }));
}

#[test]
fn locally_confirmed_commit_uses_commit_exists_locally_verdict() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("confirmed-local-commit");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/confirmed-local-commit");
    fs::write(
        workspace.join("src/lib.rs"),
        "pub fn fixture() -> u32 { 2 }\n",
    )
    .unwrap();
    run_git(&workspace, &["git", "add", "."]);
    run_git(
        &workspace,
        &["git", "commit", "-m", "Confirmed local commit"],
    );
    let commit_hash = run_git(&workspace, &["git", "rev-parse", "HEAD"]);
    let short_hash = &commit_hash[..7];
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-confirmed-local-commit".to_string()),
            source_uri: Some("notes/confirmed-local-commit.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                format!(
                    "{{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Commit the change.\"}}\n{{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git commit -m \\\"Confirmed local commit\\\"\",\"exit_code\":0,\"stdout\":\"[feature/confirmed-local-commit {short_hash}] Confirmed local commit\"}}"
                ),
            ),
        })
        .unwrap();

    assert_eq!(
        response.task_run.commit_verdict,
        CommitVerdict::CommitExistsLocally
    );
    assert_eq!(response.task_run.final_verdict, TaskVerdict::TaskCompleted);
}

#[test]
fn unconfirmed_commit_output_keeps_commit_only_task_ambiguous() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("unconfirmed-commit");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/unconfirmed-commit");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-unconfirmed-commit".to_string()),
            source_uri: Some("notes/unconfirmed-commit.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Commit the change.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git commit -m \\\"Unconfirmed\\\"\",\"exit_code\":0,\"stdout\":\"[feature/unconfirmed-commit abc1234] Unconfirmed\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    assert_eq!(
        response.task_run.commit_verdict,
        CommitVerdict::CommitCreatedFromLog
    );
    assert_eq!(
        response.task_run.final_verdict,
        TaskVerdict::AmbiguousMissingEvidence
    );
    assert!(response.task_run.missing_evidence.iter().any(|entry| {
        entry == "Captured git commit output was not confirmed by the local repo probe."
    }));
}

#[test]
fn locally_confirmed_merge_uses_local_merge_completed_verdict() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("confirmed-local-merge");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/confirmed-local-merge");
    run_git(&workspace, &["git", "checkout", "-b", "side"]);
    fs::write(
        workspace.join("src/lib.rs"),
        "pub fn fixture() -> u32 { 2 }\n",
    )
    .unwrap();
    run_git(&workspace, &["git", "add", "."]);
    run_git(
        &workspace,
        &["git", "commit", "-m", "Create side branch change"],
    );
    run_git(
        &workspace,
        &["git", "checkout", "feature/confirmed-local-merge"],
    );
    fs::write(
        workspace.join("src/main.rs"),
        "pub fn local_merge_marker() -> u32 { 3 }\n",
    )
    .unwrap();
    run_git(&workspace, &["git", "add", "."]);
    run_git(
        &workspace,
        &["git", "commit", "-m", "Create main branch change"],
    );
    run_git(
        &workspace,
        &["git", "merge", "--no-ff", "side", "-m", "Merge side"],
    );
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-confirmed-local-merge".to_string()),
            source_uri: Some("notes/confirmed-local-merge.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Merge the side branch.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_command\",\"command\":\"git merge side\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git merge side\",\"stdout\":\"Auto-merging src/lib.rs\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    assert_eq!(
        response.task_run.merge_verdict,
        MergeVerdict::LocalMergeCompleted
    );
    assert_eq!(response.task_run.final_verdict, TaskVerdict::TaskCompleted);
    assert!(!response.task_run.missing_evidence.iter().any(|entry| {
        entry == "Captured merge or rebase activity did not include decisive success or conflict evidence."
    }));
}

#[test]
fn ambiguous_merge_activity_keeps_push_followup_task_ambiguous() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("ambiguous-merge-activity");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/ambiguous-merge-activity");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-ambiguous-merge-activity".to_string()),
            source_uri: Some("notes/ambiguous-merge-activity.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Merge the release branch and push the result.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git merge release/main\",\"stdout\":\"Auto-merging src/lib.rs\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":0,\"stdout\":\"main -> main\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    assert_eq!(
        response.task_run.final_verdict,
        TaskVerdict::AmbiguousMissingEvidence
    );
    assert!(response.task_run.missing_evidence.iter().any(|entry| {
        entry == "Captured merge or rebase activity did not include decisive success or conflict evidence."
    }));
}

#[test]
fn rebase_conflict_uses_merge_conflict_verdict_and_blocks_task() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("rebase-conflict");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/rebase-conflict");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-rebase-conflict".to_string()),
            source_uri: Some("notes/rebase-conflict.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Rebase onto main and push the branch.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git rebase main\",\"exit_code\":1,\"stderr\":\"rebase conflict in src/lib.rs\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    assert_eq!(response.task_run.merge_verdict, MergeVerdict::MergeConflict);
    assert_eq!(
        response.task_run.final_verdict,
        TaskVerdict::TaskBlockedWithClearFix
    );
}

#[test]
fn ambiguous_rebase_activity_keeps_push_followup_task_ambiguous() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("ambiguous-rebase-activity");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/ambiguous-rebase-activity");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-ambiguous-rebase-activity".to_string()),
            source_uri: Some("notes/ambiguous-rebase-activity.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Rebase onto main and push the result.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git rebase main\",\"stdout\":\"Applying: update release notes\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":0,\"stdout\":\"main -> main\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    assert_eq!(
        response.task_run.final_verdict,
        TaskVerdict::AmbiguousMissingEvidence
    );
    assert!(response.task_run.missing_evidence.iter().any(|entry| {
        entry == "Captured merge or rebase activity did not include decisive success or conflict evidence."
    }));
}

#[test]
fn successful_merge_and_push_can_still_complete() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("successful-merge-and-push");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/successful-merge-and-push");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-successful-merge-and-push".to_string()),
            source_uri: Some("notes/successful-merge-and-push.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Merge the release branch and push the result.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git merge release/main\",\"exit_code\":0,\"stdout\":\"Merge made by the 'ort' strategy.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":0,\"stdout\":\"main -> main\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    assert_eq!(response.task_run.final_verdict, TaskVerdict::TaskCompleted);
}
