use rusqlite::Connection;
use std::fs;
use std::path::Path;

use super::ingest::{ingest_jsonl_chat, ingest_markdown_chat, ingest_shell_log};
use super::schema::ArtifactKind;
use super::service::{AuditArtifactRequest, HistoryAuditorService};
use super::test_support::{
    file_name, init_git_repo_with_branch, run_git, service_with_session_store, temp_dir,
    HistoryAuditorTestGuard,
};
use crate::data::{ChatMessage, ChatSession};
use crate::session_mcp::service::SessionMcpService;
use chrono::Utc;

#[test]
fn audit_session_persists_report_and_detects_non_fast_forward() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("audit");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let service = service_with_session_store(&root, &workspace, "history-audit-001");

    let response = service.audit_session("history-audit-001").unwrap();

    assert_eq!(
        response.task_run.push_verdict,
        super::schema::PushVerdict::PushFailed
    );
    assert_eq!(
        response.task_run.final_verdict,
        super::schema::TaskVerdict::TaskBlockedWithClearFix
    );
    assert!(Path::new(&response.report_path).is_file());
    assert!(!response.proposed_memories.is_empty());
}

#[test]
fn approve_memory_and_search_memory_corpus_round_trip() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("approve");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let service = service_with_session_store(&root, &workspace, "history-audit-approve");

    let audit = service.audit_session("history-audit-approve").unwrap();
    let memory_id = audit.proposed_memories[0].memory_id.clone();

    let approve = service.approve_memory(&memory_id).unwrap();
    assert_eq!(approve.status, "approved");

    let retrieval = service
        .get_relevant_memories(
            "non-fast-forward",
            Some(&workspace.display().to_string()),
            None,
            5,
        )
        .unwrap();
    assert!(retrieval.hit_count >= 1);

    let retracted = service
        .retract_memory(&memory_id, Some("test cleanup".to_string()))
        .unwrap();
    assert_eq!(retracted.status, "retracted");
}

#[test]
fn audit_history_and_explain_task_return_saved_results() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("search");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let service = service_with_session_store(&root, &workspace, "history-audit-search");

    service.audit_session("history-audit-search").unwrap();

    let history = service
        .audit_history(
            "non-fast-forward",
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
    assert!(history.hit_count >= 1);

    let explained = service.explain_task("history-audit-search").unwrap();
    assert!(explained.task_run.requested_goal.contains("Commit"));
}

#[test]
fn audit_history_honors_branch_and_task_type_filters() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("branch-task-type");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/history-audit");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::MarkdownChat,
            artifact_id: Some("artifact-git-feature".to_string()),
            source_uri: Some("notes/git-feature.md".to_string()),
            artifact_path: None,
            content: Some(
                "## User\nHandle release check by pushing the branch.\n\n## Assistant\n$ git push origin main\n[exit_code=1]\n[stderr]\nnon-fast-forward"
                    .to_string(),
            ),
        })
        .unwrap();
    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-test-feature".to_string()),
            source_uri: Some("notes/test-feature.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Handle release check by running tests.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"cargo test --release\",\"exit_code\":0,\"stdout\":\"all tests passed\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    run_git(
        &workspace,
        &["git", "checkout", "-B", "release/history-audit"],
    );
    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::MarkdownChat,
            artifact_id: Some("artifact-git-release".to_string()),
            source_uri: Some("notes/git-release.md".to_string()),
            artifact_path: None,
            content: Some(
                "## User\nHandle release check by pushing the branch.\n\n## Assistant\n$ git push origin main\n[exit_code=1]\n[stderr]\nnon-fast-forward"
                    .to_string(),
            ),
        })
        .unwrap();

    let history = service
        .audit_history(
            "release check",
            Some(&workspace.display().to_string()),
            Some("feature/history-audit"),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
    assert!(history.hit_count >= 1);
    assert!(history.hits.iter().all(|hit| {
        matches!(
            file_name(&hit.file_path).as_str(),
            "artifact-git-feature.md" | "artifact-test-feature.md"
        )
    }));

    let git_only = service
        .audit_history(
            "release check",
            Some(&workspace.display().to_string()),
            Some("feature/history-audit"),
            Some("git_delivery"),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
    assert!(git_only.hit_count >= 1);
    assert!(git_only
        .hits
        .iter()
        .all(|hit| file_name(&hit.file_path) == "artifact-git-feature.md"));

    let tests_only = service
        .audit_history(
            "release check",
            Some(&workspace.display().to_string()),
            Some("feature/history-audit"),
            Some("test_run"),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
    assert!(tests_only.hit_count >= 1);
    assert!(tests_only
        .hits
        .iter()
        .all(|hit| file_name(&hit.file_path) == "artifact-test-feature.md"));

    let wrong_branch = service
        .audit_history(
            "release check",
            Some(&workspace.display().to_string()),
            Some("main"),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
    assert_eq!(wrong_branch.hit_count, 0);
}

#[test]
fn persisted_audit_records_include_typed_events_and_edges() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("typed-store");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/store-shape");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-store-001".to_string()),
            source_uri: Some("notes/store.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Maybe investigate first, then commit and push the change if needed.\"}\n{\"actor\":\"assistant\",\"kind\":\"message\",\"content\":\"Done, the task is complete.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git commit -m \\\"Store shape\\\"\",\"exit_code\":0,\"stdout\":\"[feature/store-shape abc1234] Store shape\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"non-fast-forward\"}"
                    .to_string(),
            ),
        })
        .unwrap();

    let paths = super::paths::ensure_workspace_layout(&workspace).unwrap();
    let conn = Connection::open(&paths.db_path).unwrap();

    let session_shape = conn
        .query_row(
            "SELECT branch, task_type, primary_error_kind, incident_pattern, final_verdict \
             FROM audit_sessions WHERE session_id = ?1",
            ["artifact-store-001"],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(session_shape.0.as_deref(), Some("feature/store-shape"));
    assert_eq!(session_shape.1, "git_delivery");
    assert_eq!(session_shape.2.as_deref(), Some("non_fast_forward"));
    assert_eq!(
        session_shape.3.as_deref(),
        Some("git_delivery_push_non_fast_forward")
    );
    assert_eq!(session_shape.4, "task_blocked_with_clear_fix");

    let intent_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM intent_events WHERE session_id = ?1",
            ["artifact-store-001"],
            |row| row.get(0),
        )
        .unwrap();
    let action_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM action_events WHERE session_id = ?1",
            ["artifact-store-001"],
            |row| row.get(0),
        )
        .unwrap();
    let error_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM error_events WHERE session_id = ?1",
            ["artifact-store-001"],
            |row| row.get(0),
        )
        .unwrap();
    let claim_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM claim_events WHERE session_id = ?1",
            ["artifact-store-001"],
            |row| row.get(0),
        )
        .unwrap();
    let edge_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_edges WHERE session_id = ?1",
            ["artifact-store-001"],
            |row| row.get(0),
        )
        .unwrap();
    assert!(intent_count >= 1);
    assert!(action_count >= 2);
    assert!(error_count >= 1);
    assert!(claim_count >= 1);
    assert!(edge_count >= 5);
}

#[test]
fn relevant_memories_honor_branch_filters() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("memory-branch");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/memory-audit");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    let feature_audit = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-memory-feature".to_string()),
            source_uri: Some("notes/memory-feature.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Commit and push the change.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git commit -m \\\"Feature memory\\\"\",\"exit_code\":0,\"stdout\":\"[feature/memory-audit abc1234] Feature memory\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"non-fast-forward\"}"
                    .to_string(),
            ),
        })
        .unwrap();
    let feature_memory = service
        .approve_memory(&feature_audit.proposed_memories[0].memory_id)
        .unwrap();

    run_git(
        &workspace,
        &["git", "checkout", "-B", "release/memory-audit"],
    );
    let release_audit = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-memory-release".to_string()),
            source_uri: Some("notes/memory-release.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Run the release verification tests.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"cargo test --release\",\"exit_code\":0,\"stdout\":\"release verification passed\"}"
                    .to_string(),
            ),
        })
        .unwrap();
    let release_memory = service
        .approve_memory(&release_audit.proposed_memories[0].memory_id)
        .unwrap();

    let feature_hits = service
        .get_relevant_memories(
            &feature_memory.text,
            Some(&workspace.display().to_string()),
            Some("feature/memory-audit"),
            8,
        )
        .unwrap();
    assert!(feature_hits.hit_count >= 1);
    assert!(feature_hits
        .hits
        .iter()
        .all(|hit| file_name(&hit.file_path) == format!("{}.md", feature_memory.memory_id)));

    let release_hits = service
        .get_relevant_memories(
            &release_memory.text,
            Some(&workspace.display().to_string()),
            Some("release/memory-audit"),
            8,
        )
        .unwrap();
    assert!(release_hits.hit_count >= 1);
    assert!(release_hits
        .hits
        .iter()
        .all(|hit| file_name(&hit.file_path) == format!("{}.md", release_memory.memory_id)));

    let missing_branch_hits = service
        .get_relevant_memories(
            &feature_memory.text,
            Some(&workspace.display().to_string()),
            Some("main"),
            8,
        )
        .unwrap();
    assert_eq!(missing_branch_hits.hit_count, 0);
}

#[test]
fn ingest_helpers_accept_markdown_jsonl_and_shell_logs() {
    let markdown = "## User\nFix the push\n\n## Assistant\n$ git push origin main\n[exit_code=1]\n[stderr]\nnon-fast-forward";
    let jsonl = "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"run tests\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"cargo test\",\"exit_code\":0,\"stdout\":\"ok\"}";
    let shell = "$ git status\n[exit_code=0]\n[stdout]\nclean";

    let md_events = ingest_markdown_chat("s1", "markdown.md", markdown).unwrap();
    let jsonl_events = ingest_jsonl_chat("s2", "events.jsonl", jsonl).unwrap();
    let shell_events = ingest_shell_log("s3", "session.log", shell).unwrap();

    assert_eq!(md_events.len(), 2);
    assert_eq!(jsonl_events.len(), 2);
    assert_eq!(shell_events.len(), 2);
}

#[test]
fn audit_artifact_accepts_inline_markdown_and_jsonl_artifacts() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("artifact-inline");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    let markdown = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::MarkdownChat,
            artifact_id: Some("artifact-md-001".to_string()),
            source_uri: Some("notes/history.md".to_string()),
            artifact_path: None,
            content: Some(
                "## User\nPush the branch\n\n## Assistant\n$ git push origin main\n[exit_code=1]\n[stderr]\nnon-fast-forward"
                    .to_string(),
            ),
        })
        .unwrap();
    assert_eq!(markdown.session_id, "artifact-md-001");
    assert_eq!(
        markdown.task_run.final_verdict,
        super::schema::TaskVerdict::TaskBlockedWithClearFix
    );

    let jsonl = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-jsonl-001".to_string()),
            source_uri: Some("traces/history.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"run tests\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"cargo test\",\"exit_code\":0,\"stdout\":\"ok\"}"
                    .to_string(),
            ),
        })
        .unwrap();
    assert_eq!(jsonl.session_id, "artifact-jsonl-001");
    assert_eq!(
        jsonl.task_run.final_verdict,
        super::schema::TaskVerdict::TaskCompleted
    );
}

#[test]
fn audit_artifact_reads_shell_log_from_path_and_supports_task_explanation() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("artifact-shell");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let artifact_path = root.join("push.log");
    fs::write(
        &artifact_path,
        "$ git push origin main\n[exit_code=1]\n[stderr]\n! [rejected] main -> main (non-fast-forward)\n",
    )
    .unwrap();
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::ShellLog,
            artifact_id: Some("artifact-shell-001".to_string()),
            source_uri: None,
            artifact_path: Some(artifact_path.display().to_string()),
            content: None,
        })
        .unwrap();

    assert_eq!(
        response.task_run.push_verdict,
        super::schema::PushVerdict::PushFailed
    );
    let explained = service.explain_task("artifact-shell-001").unwrap();
    assert_eq!(explained.task_run.task_id, "artifact-shell-001");
}

#[test]
fn audit_artifact_accepts_provider_session_json() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("artifact-provider");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let now = Utc::now();
    let imported = ChatSession {
        id: "provider-session-123".to_string(),
        name: "Imported Provider Session".to_string(),
        launched_from: Some(workspace.display().to_string()),
        messages: vec![
            ChatMessage {
                content: "Commit and push the change.".to_string(),
                is_user: true,
                timestamp: now,
            },
            ChatMessage {
                content:
                    "$ git push origin main\n[exit_code=1]\n[stderr]\n! [rejected] main -> main (non-fast-forward)"
                        .to_string(),
                is_user: false,
                timestamp: now,
            },
        ],
    };
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::ProviderSessionJson,
            artifact_id: Some("artifact-provider-001".to_string()),
            source_uri: Some("imports/provider-session.json".to_string()),
            artifact_path: None,
            content: Some(serde_json::to_string_pretty(&imported).unwrap()),
        })
        .unwrap();

    assert_eq!(response.session_id, "artifact-provider-001");
    assert!(response.event_count >= 3);
    assert!(!response.proposed_memories.is_empty());
}

#[test]
fn missing_requested_push_evidence_keeps_delivery_task_ambiguous() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("missing-push");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/missing-push");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::MarkdownChat,
            artifact_id: Some("artifact-missing-push-001".to_string()),
            source_uri: Some("notes/missing-push.md".to_string()),
            artifact_path: None,
            content: Some(
                "## User\nCommit and push the change.\n\n## Assistant\n$ git commit -m \"Ship it\"\n[exit_code=0]\n[stdout]\n[feature/missing-push abc1234] Ship it"
                    .to_string(),
            ),
        })
        .unwrap();

    assert_eq!(
        response.task_run.final_verdict,
        super::schema::TaskVerdict::AmbiguousMissingEvidence
    );
    assert!(response.task_run.missing_evidence.iter().any(|entry| {
        entry == "The request asked for a push, but no captured git push action was recorded."
    }));
}

#[test]
fn repeated_non_fast_forward_failures_promote_supported_procedural_memory() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("repeat-procedural");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/repeat-procedural");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    let seed = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-repeat-procedural-seed".to_string()),
            source_uri: Some("notes/repeat-procedural-seed.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Push the branch after the fix.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"non-fast-forward\"}"
                    .to_string(),
            ),
        })
        .unwrap();
    assert!(seed
        .proposed_memories
        .iter()
        .any(|memory| memory.kind == super::schema::MemoryKind::Procedural));

    let paths = super::paths::ensure_workspace_layout(&workspace).unwrap();
    let summaries = super::store::list_audit_summaries(&paths).unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(
        summaries[0].primary_error_kind,
        Some(super::schema::ErrorKind::NonFastForward)
    );

    let thin_support = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-repeat-procedural-seed-2".to_string()),
            source_uri: Some("notes/repeat-procedural-seed-2.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Retry the push after checking divergence.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"non-fast-forward\"}"
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
        assert!(!thin_procedural.text.contains("repeated non-fast-forward"));
    }

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::JsonlChat,
            artifact_id: Some("artifact-repeat-procedural-current".to_string()),
            source_uri: Some("notes/repeat-procedural-current.jsonl".to_string()),
            artifact_path: None,
            content: Some(
                "{\"actor\":\"user\",\"kind\":\"message\",\"content\":\"Push the branch after updating the release notes.\"}\n{\"actor\":\"tool\",\"kind\":\"shell_result\",\"command\":\"git push origin main\",\"exit_code\":1,\"stderr\":\"non-fast-forward\"}"
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
                "missing procedural memory: {:?}",
                response.proposed_memories
            )
        });
    assert!(procedural
        .supporting_task_ids
        .contains(&"artifact-repeat-procedural-seed".to_string()));
    assert!(procedural
        .supporting_task_ids
        .contains(&"artifact-repeat-procedural-seed-2".to_string()));
    assert!(procedural.text.contains("repeated non-fast-forward"));
    assert!(procedural.confidence >= 0.89);
}

#[test]
fn preference_and_claim_contradiction_memories_are_proposed() {
    let _guard = HistoryAuditorTestGuard::new();
    let root = temp_dir("memory-structured");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    init_git_repo_with_branch(&workspace, "feature/structured-memory");
    let service = HistoryAuditorService::new(SessionMcpService::new(Vec::new()));

    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::MarkdownChat,
            artifact_id: Some("artifact-memory-structured-seed".to_string()),
            source_uri: Some("notes/structured-memory-seed.md".to_string()),
            artifact_path: None,
            content: Some(
                "## User\nPush the branch to main.\n\n## Assistant\n$ git push origin main\n[exit_code=1]\n[stderr]\nprotected branch"
                    .to_string(),
            ),
        })
        .unwrap();
    service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::MarkdownChat,
            artifact_id: Some("artifact-memory-structured-seed-2".to_string()),
            source_uri: Some("notes/structured-memory-seed-2.md".to_string()),
            artifact_path: None,
            content: Some(
                "## User\nRetry the direct push to main.\n\n## Assistant\n$ git push origin main\n[exit_code=1]\n[stderr]\nprotected branch"
                    .to_string(),
            ),
        })
        .unwrap();

    let response = service
        .audit_artifact(AuditArtifactRequest {
            workspace_path: workspace.display().to_string(),
            artifact_kind: ArtifactKind::MarkdownChat,
            artifact_id: Some("artifact-memory-structured".to_string()),
            source_uri: Some("notes/structured-memory.md".to_string()),
            artifact_path: None,
            content: Some(
                "## User\nMaybe do not push directly to main; prefer a feature branch if needed.\n\n## Assistant\nDone, the task is complete and pushed successfully.\n\n## Assistant\n$ git push origin main\n[exit_code=1]\n[stderr]\nprotected branch"
                    .to_string(),
            ),
        })
        .unwrap();

    assert!(response
        .task_run
        .assistant_claims
        .iter()
        .any(|claim| { claim.kind == super::schema::ClaimKind::TaskCompleted }));
    assert!(response
        .task_run
        .intent_notes
        .iter()
        .any(|note| !note.evidence_event_ids.is_empty()));
    assert!(!response.task_run.root_cause_evidence_event_ids.is_empty());
    assert!(!response
        .task_run
        .recommended_fix_evidence_event_ids
        .is_empty());
    assert!(response
        .proposed_memories
        .iter()
        .any(|memory| { memory.kind == super::schema::MemoryKind::UserPreference }));
    assert!(response
        .proposed_memories
        .iter()
        .any(|memory| { memory.kind == super::schema::MemoryKind::AntiPattern }));
    let semantic_fact = response
        .proposed_memories
        .iter()
        .find(|memory| memory.kind == super::schema::MemoryKind::SemanticFact)
        .unwrap();
    assert!(semantic_fact
        .supporting_task_ids
        .contains(&"artifact-memory-structured-seed".to_string()));
    assert!(semantic_fact
        .supporting_task_ids
        .contains(&"artifact-memory-structured-seed-2".to_string()));

    let paths = super::paths::ensure_workspace_layout(&workspace).unwrap();
    let conn = Connection::open(&paths.db_path).unwrap();
    let similar_edges: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_edges WHERE session_id = ?1 AND edge_kind = 'similar_to'",
            ["artifact-memory-structured"],
            |row| row.get(0),
        )
        .unwrap();
    assert!(similar_edges >= 1);
}
