use std::fs;

use rusqlite::{params, Connection};

use super::paths;
use super::schema::ArtifactKind;
use super::service::{AuditArtifactRequest, HistoryAuditorService};
use super::test_support::{
    file_name, init_git_repo_with_branch, temp_dir, HistoryAuditorTestGuard,
};
use crate::session_mcp::service::SessionMcpService;

#[test]
fn repeated_no_upstream_failures_promote_supported_procedural_memory() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("repeat-no-upstream");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/repeat-no-upstream");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-repeat-upstream-seed".to_string()),
            source_uri: Some("notes/repeat-upstream-seed.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Push the new branch upstream.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin HEAD\",\"exit_code\":1,\"stderr\":\"fatal: The current branch feature/repeat-no-upstream has no upstream branch\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-repeat-upstream-seed-2".to_string()),
            source_uri: Some("notes/repeat-upstream-seed-2.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Retry the branch push after setting upstream details.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin HEAD\",\"exit_code\":1,\"stderr\":\"fatal: The current branch feature/repeat-no-upstream has no upstream branch\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-repeat-upstream-current".to_string()),
            source_uri: Some("notes/repeat-upstream-current.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Push the release prep branch upstream.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin HEAD\",\"exit_code\":1,\"stderr\":\"fatal: The current branch feature/repeat-no-upstream has no upstream branch\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    let procedural = response
        .proposed_memories
        .iter()
        .find(|memory| memory.kind == super::schema::MemoryKind::Procedural)
        .unwrap_or_else(|| {
            panic!(
                "missing procedural memory for repeated no-upstream failures: {:?}",
                response.proposed_memories
            )
        });
    assert!(procedural
        .supporting_task_ids
        .contains(&"artifact-repeat-upstream-seed".to_string()));
    assert!(procedural
        .supporting_task_ids
        .contains(&"artifact-repeat-upstream-seed-2".to_string()));
    assert!(procedural
        .text
        .contains("repeated first-push upstream mistakes"));
    assert!(procedural.confidence >= 0.89);
}

#[test]
fn repeated_auth_failures_promote_supported_procedural_memory() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("repeat-auth-failure");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/repeat-auth-failure");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-repeat-auth-seed".to_string()),
            source_uri: Some("notes/repeat-auth-seed.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Push the release branch to origin.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"authentication failed for origin\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-repeat-auth-seed-2".to_string()),
            source_uri: Some("notes/repeat-auth-seed-2.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Retry the release push after checking the auth context.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"authentication failed for origin\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-repeat-auth-current".to_string()),
            source_uri: Some("notes/repeat-auth-current.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Retry the release push after updating the token.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"authentication failed for origin\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    let procedural = response
        .proposed_memories
        .iter()
        .find(|memory| memory.kind == super::schema::MemoryKind::Procedural)
        .unwrap_or_else(|| {
            panic!(
                "missing procedural memory for repeated auth failures: {:?}",
                response.proposed_memories
            )
        });
    assert!(procedural
        .supporting_task_ids
        .contains(&"artifact-repeat-auth-seed".to_string()));
    assert!(procedural
        .supporting_task_ids
        .contains(&"artifact-repeat-auth-seed-2".to_string()));
    assert!(procedural
        .text
        .contains("repeated remote authentication failures"));
    assert!(procedural
        .text
        .contains("refresh the credential or auth context"));
    assert!(procedural.confidence >= 0.89);
}

#[test]
fn repeated_protected_branch_failures_promote_supported_procedural_memory() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("repeat-protected-branch");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/repeat-protected-branch");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-repeat-protected-seed".to_string()),
            source_uri: Some("notes/repeat-protected-seed.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Push the change to main.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"remote: protected branch hook declined\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-repeat-protected-seed-2".to_string()),
            source_uri: Some("notes/repeat-protected-seed-2.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Retry the protected branch push after reviewing the policy.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"remote: protected branch hook declined\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-repeat-protected-current".to_string()),
            source_uri: Some("notes/repeat-protected-current.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Retry the release push after the branch protection failure.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"remote: protected branch hook declined\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    let procedural = response
        .proposed_memories
        .iter()
        .find(|memory| memory.kind == super::schema::MemoryKind::Procedural)
        .unwrap_or_else(|| {
            panic!(
                "missing procedural memory for repeated protected-branch failures: {:?}",
                response.proposed_memories
            )
        });
    assert!(procedural
        .supporting_task_ids
        .contains(&"artifact-repeat-protected-seed".to_string()));
    assert!(procedural
        .supporting_task_ids
        .contains(&"artifact-repeat-protected-seed-2".to_string()));
    assert!(procedural
        .text
        .contains("repeated protected-branch delivery rejections"));
    assert!(procedural
        .text
        .contains("continue through the repo's review path"));
    assert!(procedural.confidence >= 0.89);
}

#[test]
fn repeated_nothing_to_commit_promote_supported_procedural_memory() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("repeat-nothing-to-commit");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/repeat-nothing-to-commit");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-repeat-nothing-seed".to_string()),
            source_uri: Some("notes/repeat-nothing-seed.jsonl".to_string()),
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
            artifact_id: Some("artifact-repeat-nothing-seed-2".to_string()),
            source_uri: Some("notes/repeat-nothing-seed-2.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Retry the release metadata commit after checking the changed files.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git commit -m \\\"Release metadata second retry\\\"\",\"exit_code\":1,\"stderr\":\"nothing to commit, working tree clean\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-repeat-nothing-current".to_string()),
            source_uri: Some("notes/repeat-nothing-current.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Retry the release metadata commit.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git commit -m \\\"Release metadata retry\\\"\",\"exit_code\":1,\"stderr\":\"nothing to commit, working tree clean\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    let procedural = response
        .proposed_memories
        .iter()
        .find(|memory| memory.kind == super::schema::MemoryKind::Procedural)
        .unwrap_or_else(|| {
            panic!(
                "missing procedural memory for repeated nothing-to-commit failures: {:?}",
                response.proposed_memories
            )
        });
    assert!(procedural
        .supporting_task_ids
        .contains(&"artifact-repeat-nothing-seed".to_string()));
    assert!(procedural
        .supporting_task_ids
        .contains(&"artifact-repeat-nothing-seed-2".to_string()));
    assert!(procedural
        .text
        .contains("repeated commit attempts with no actual file delta"));
    assert!(procedural
        .text
        .contains("confirm whether the requested edits were already committed"));
    assert!(procedural.confidence >= 0.89);
}

#[test]
fn approving_stronger_memory_supersedes_weaker_related_memory() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("memory-supersedes");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/memory-supersedes");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    let initial = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-supersede-seed".to_string()),
            source_uri: Some("notes/supersede-seed.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Push the release branch.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"non-fast-forward\"}"
                    .to_string(),
            ),
        })
        .unwrap();
    let weaker_memory = initial
        .proposed_memories
        .iter()
        .find(|memory| memory.kind == super::schema::MemoryKind::Procedural)
        .unwrap()
        .clone();
    service.approve_memory(&weaker_memory.memory_id).unwrap();

    let stronger = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-supersede-seed-2".to_string()),
            source_uri: Some("notes/supersede-seed-2.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Retry the release push after checking divergence.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"non-fast-forward\"}"
                    .to_string(),
            ),
        })
        .unwrap();
    let stronger = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-supersede-current".to_string()),
            source_uri: Some("notes/supersede-current.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Retry the release push after fetching origin.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"non-fast-forward\"}"
                    .to_string(),
            ),
        })
        .unwrap();
    let stronger_memory = stronger
        .proposed_memories
        .iter()
        .find(|memory| {
            memory.kind == super::schema::MemoryKind::Procedural
                && !memory.supporting_task_ids.is_empty()
        })
        .unwrap()
        .clone();
    service.approve_memory(&stronger_memory.memory_id).unwrap();

    let paths = paths::ensure_workspace_layout(&workspace).unwrap();
    let conn = Connection::open(&paths.db_path).unwrap();

    let weaker_row = conn
        .query_row(
            "SELECT status, status_reason FROM memory_records WHERE memory_id = ?1",
            params![weaker_memory.memory_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .unwrap();
    let expected_reason = format!("superseded by {}", stronger_memory.memory_id);
    assert_eq!(weaker_row.0, "retracted");
    assert_eq!(weaker_row.1.as_deref(), Some(expected_reason.as_str()));

    let supersedes_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_edges WHERE session_id = ?1 AND edge_kind = 'supersedes' AND src_kind = 'memory' AND src_id = ?2 AND dst_kind = 'memory' AND dst_id = ?3",
            params![
                stronger_memory.session_id,
                stronger_memory.memory_id,
                weaker_memory.memory_id
            ],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(supersedes_count, 1);

    let relevant = service
        .get_relevant_memories(
            "non-fast-forward",
            Some(&workspace.display().to_string()),
            None,
            10,
        )
        .unwrap();
    assert!(relevant
        .hits
        .iter()
        .any(|hit| file_name(&hit.file_path) == format!("{}.md", stronger_memory.memory_id)));
    assert!(relevant
        .hits
        .iter()
        .all(|hit| file_name(&hit.file_path) != format!("{}.md", weaker_memory.memory_id)));
}

#[test]
fn repeated_pre_receive_failures_need_multiple_supporting_tasks_for_repo_fact() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("repeat-pre-receive");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/repeat-pre-receive");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::MarkdownChat,
            artifact_id: Some("artifact-repeat-pre-receive-seed".to_string()),
            source_uri: Some("notes/repeat-pre-receive-seed.md".to_string()),
            artifact_path: None,
            content: Some(
                "## User\nPush the release branch.\n\n## Assistant\n$ git push origin main\n[exit_code=1]\n[stderr]\npre-receive hook declined"
                    .to_string(),
            ),
        })
        .unwrap();

    let thin_support = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::MarkdownChat,
            artifact_id: Some("artifact-repeat-pre-receive-thin".to_string()),
            source_uri: Some("notes/repeat-pre-receive-thin.md".to_string()),
            artifact_path: None,
            content: Some(
                "## User\nRetry the protected release push after the checks.\n\n## Assistant\n$ git push origin main\n[exit_code=1]\n[stderr]\npre-receive hook declined"
                    .to_string(),
            ),
        })
        .unwrap();
    assert!(thin_support
        .proposed_memories
        .iter()
        .all(|memory| memory.kind != super::schema::MemoryKind::RepoFact));

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::MarkdownChat,
            artifact_id: Some("artifact-repeat-pre-receive-seed-2".to_string()),
            source_uri: Some("notes/repeat-pre-receive-seed-2.md".to_string()),
            artifact_path: None,
            content: Some(
                "## User\nPush the release branch after satisfying the checks.\n\n## Assistant\n$ git push origin main\n[exit_code=1]\n[stderr]\npre-receive hook declined"
                    .to_string(),
            ),
        })
        .unwrap();

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::MarkdownChat,
            artifact_id: Some("artifact-repeat-pre-receive-current".to_string()),
            source_uri: Some("notes/repeat-pre-receive-current.md".to_string()),
            artifact_path: None,
            content: Some(
                "## User\nRetry the protected release push after the checks.\n\n## Assistant\n$ git push origin main\n[exit_code=1]\n[stderr]\npre-receive hook declined"
                    .to_string(),
            ),
        })
        .unwrap();

    let repo_fact = response
        .proposed_memories
        .iter()
        .find(|memory| memory.kind == super::schema::MemoryKind::RepoFact)
        .unwrap_or_else(|| {
            panic!(
                "missing repo fact for repeated pre-receive failures: {:?}",
                response.proposed_memories
            )
        });
    assert!(repo_fact
        .supporting_task_ids
        .contains(&"artifact-repeat-pre-receive-seed".to_string()));
    assert!(repo_fact
        .supporting_task_ids
        .contains(&"artifact-repeat-pre-receive-seed-2".to_string()));
    assert!(repo_fact.text.contains("pre-receive hook"));
    assert!(repo_fact.confidence >= 0.90);
}

#[test]
fn repeated_dependency_failures_promote_supported_procedural_memory() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("repeat-dependency-failure");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/repeat-dependency-failure");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-repeat-dependency-seed".to_string()),
            source_uri: Some("notes/repeat-dependency-seed.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Run the release verification tests.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"cargo test --release\",\"exit_code\":1,\"stderr\":\"dependency resolution failure\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    let thin_support = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-repeat-dependency-thin".to_string()),
            source_uri: Some("notes/repeat-dependency-thin.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Retry the release verification tests after the dependency update.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"cargo test --release\",\"exit_code\":1,\"stderr\":\"dependency resolution failure\"}"
                    .to_string(),
            ),
        })
        .unwrap();
    if let Some(thin_procedural) = thin_support
        .proposed_memories
        .iter()
        .find(|memory| memory.kind == super::schema::MemoryKind::Procedural)
    {
        assert!(thin_procedural.supporting_task_ids.is_empty());
        assert!(!thin_procedural
            .text
            .contains("repeated dependency-related delivery failures"));
    }

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-repeat-dependency-seed-2".to_string()),
            source_uri: Some("notes/repeat-dependency-seed-2.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Retry the release verification tests after refreshing the lockfile.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"cargo test --release\",\"exit_code\":1,\"stderr\":\"dependency resolution failure\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    let procedural = response
        .proposed_memories
        .iter()
        .find(|memory| memory.kind == super::schema::MemoryKind::Procedural)
        .unwrap_or_else(|| {
            panic!(
                "missing procedural memory for repeated dependency failures: {:?}",
                response.proposed_memories
            )
        });
    assert!(procedural
        .supporting_task_ids
        .contains(&"artifact-repeat-dependency-seed".to_string()));
    assert!(procedural.supporting_task_ids.len() >= 2);
    assert!(procedural
        .text
        .contains("repeated dependency-related delivery failures"));
    assert!(procedural
        .text
        .contains("repair the environment or manifest"));
    assert!(procedural.confidence >= 0.89);
}
