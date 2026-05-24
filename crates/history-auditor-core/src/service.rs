use anyhow::{bail, Context, Result};
use chrono::Utc;
use semble_rs::dense::HashEmbedder;
use semble_rs::{SearchMode, SembleIndex};
use serde::Serialize;
use serde_json::json;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

use super::extract::extract_audit;
use super::incident::classify_incident_pattern;
use super::ingest::{
    ingest_chat_session, ingest_jsonl_chat, ingest_markdown_chat, ingest_provider_session,
    ingest_shell_log,
};
use super::memory::compile_memories;
use super::paths::{
    artifact_session_path, ensure_workspace_layout, history_auditor_root_dir, WorkspaceAuditorPaths,
};
use super::repo::probe_local_repo;
use super::report::write_report;
use super::schema::{ArtifactKind, AuditRecord, CorpusSearchHit, MemoryRecord, RepoProbe, TaskRun};
use super::session::{InspectedSession, SessionSnapshot, SessionSource};
use super::store;
use super::util::atomic_write;
use std::sync::Arc;

#[derive(Clone)]
pub struct HistoryAuditorService {
    session_source: Option<Arc<dyn SessionSource>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuditSessionResponse {
    pub workspace_root: String,
    pub workspace_id: String,
    pub session_id: String,
    pub session_name: String,
    pub provider: String,
    pub provider_display_name: String,
    pub audited_at: chrono::DateTime<Utc>,
    pub report_path: String,
    pub event_count: usize,
    pub intent_count: usize,
    pub action_count: usize,
    pub error_count: usize,
    pub task_run: TaskRun,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_probe: Option<RepoProbe>,
    #[serde(default)]
    pub proposed_memories: Vec<MemoryRecord>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HistorySearchResponse {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_verdict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incident_pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audited_after: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audited_before: Option<String>,
    pub hit_count: usize,
    pub hits: Vec<CorpusSearchHit>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExplainTaskResponse {
    pub task_id: String,
    pub task_run: TaskRun,
}

#[derive(Clone, Debug)]
pub struct AuditArtifactRequest {
    pub workspace_path: String,
    pub artifact_kind: ArtifactKind,
    pub artifact_id: Option<String>,
    pub source_uri: Option<String>,
    pub artifact_path: Option<String>,
    pub content: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MemoryMutationResponse {
    pub memory_id: String,
    pub status: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl HistoryAuditorService {
    pub fn new<T>(session_source: T) -> Self
    where
        T: SessionSource + 'static,
    {
        Self {
            session_source: Some(Arc::new(session_source)),
        }
    }

    pub fn without_session_source() -> Self {
        Self {
            session_source: None,
        }
    }

    pub fn audit_session(&self, session_id: &str) -> Result<AuditSessionResponse> {
        let inspected = self
            .session_source
            .as_ref()
            .context("session auditing requires a configured session source")?
            .inspect_session(session_id)?;
        let audit = self.build_session_audit_record(&inspected)?;
        self.finalize_audit(
            audit,
            serde_json::to_value(&inspected.session)
                .context("failed to serialize session snapshot")?,
        )
    }

    pub fn audit_artifact(&self, request: AuditArtifactRequest) -> Result<AuditSessionResponse> {
        let (audit, artifact_snapshot) = self.build_artifact_audit_record(request)?;
        self.finalize_audit(audit, artifact_snapshot)
    }

    pub fn propose_memory_writes(&self, session_id: &str) -> Result<Vec<MemoryRecord>> {
        let response = self.audit_session(session_id)?;
        Ok(response.proposed_memories)
    }

    pub fn approve_memory(&self, memory_id: &str) -> Result<MemoryMutationResponse> {
        let paths = store::list_workspace_paths(&history_auditor_root_dir())?;
        let memory = store::approve_memory(&paths, memory_id)?;
        Ok(MemoryMutationResponse {
            memory_id: memory.memory_id,
            status: "approved".to_string(),
            text: memory.text,
            reason: memory.status_reason,
        })
    }

    pub fn retract_memory(
        &self,
        memory_id: &str,
        reason: Option<String>,
    ) -> Result<MemoryMutationResponse> {
        let paths = store::list_workspace_paths(&history_auditor_root_dir())?;
        let memory = store::retract_memory(&paths, memory_id, reason.clone())?;
        Ok(MemoryMutationResponse {
            memory_id: memory.memory_id,
            status: "retracted".to_string(),
            text: memory.text,
            reason,
        })
    }

    pub fn explain_task(&self, task_id: &str) -> Result<ExplainTaskResponse> {
        let paths = store::list_workspace_paths(&history_auditor_root_dir())?;
        for path in paths {
            if !path.db_path.is_file() {
                continue;
            }
            if let Some(task_run) = store::explain_task(&path, task_id)? {
                return Ok(ExplainTaskResponse {
                    task_id: task_id.to_string(),
                    task_run,
                });
            }
        }
        bail!("No audited task found for `{task_id}`")
    }

    pub fn audit_history(
        &self,
        query: &str,
        repo: Option<&str>,
        branch: Option<&str>,
        task_type: Option<&str>,
        session_id: Option<&str>,
        final_verdict: Option<&str>,
        incident_pattern: Option<&str>,
        audited_after: Option<&str>,
        audited_before: Option<&str>,
    ) -> Result<HistorySearchResponse> {
        let audited_after = normalize_timestamp_filter(audited_after)?;
        let audited_before = normalize_timestamp_filter(audited_before)?;
        let hits = self.search_corpus(
            query,
            repo,
            CorpusKind::Reports,
            SearchScope {
                branch,
                task_type,
                error_kind: None,
                session_id,
                final_verdict,
                incident_pattern,
                audited_after: audited_after.as_deref(),
                audited_before: audited_before.as_deref(),
                failure_only: false,
            },
            8,
        )?;
        Ok(HistorySearchResponse {
            query: query.to_string(),
            repo: repo.map(str::to_string),
            branch: branch.map(str::to_string),
            task_type: task_type.map(str::to_string),
            session_id: session_id.map(str::to_string),
            final_verdict: final_verdict.map(str::to_string),
            incident_pattern: incident_pattern.map(str::to_string),
            audited_after,
            audited_before,
            hit_count: hits.len(),
            hits,
        })
    }

    pub fn find_similar_failures(
        &self,
        error_text: &str,
        repo: Option<&str>,
        branch: Option<&str>,
        task_type: Option<&str>,
        session_id: Option<&str>,
        final_verdict: Option<&str>,
        incident_pattern: Option<&str>,
        audited_after: Option<&str>,
        audited_before: Option<&str>,
    ) -> Result<HistorySearchResponse> {
        let audited_after = normalize_timestamp_filter(audited_after)?;
        let audited_before = normalize_timestamp_filter(audited_before)?;
        let hits = self.search_corpus(
            error_text,
            repo,
            CorpusKind::Reports,
            SearchScope {
                branch,
                task_type,
                error_kind: classify_error_text(error_text),
                session_id,
                final_verdict,
                incident_pattern: incident_pattern
                    .or_else(|| classify_incident_pattern(error_text, task_type)),
                audited_after: audited_after.as_deref(),
                audited_before: audited_before.as_deref(),
                failure_only: true,
            },
            5,
        )?;
        Ok(HistorySearchResponse {
            query: error_text.to_string(),
            repo: repo.map(str::to_string),
            branch: branch.map(str::to_string),
            task_type: task_type
                .map(str::to_string)
                .or_else(|| Some("failure".to_string())),
            session_id: session_id.map(str::to_string),
            final_verdict: final_verdict.map(str::to_string),
            incident_pattern: incident_pattern
                .or_else(|| classify_incident_pattern(error_text, task_type))
                .map(str::to_string),
            audited_after,
            audited_before,
            hit_count: hits.len(),
            hits,
        })
    }

    pub fn get_relevant_memories(
        &self,
        query: &str,
        repo: Option<&str>,
        branch: Option<&str>,
        max_results: usize,
    ) -> Result<HistorySearchResponse> {
        let hits = self.search_corpus(
            query,
            repo,
            CorpusKind::Memory,
            SearchScope {
                branch,
                task_type: None,
                error_kind: None,
                session_id: None,
                final_verdict: None,
                incident_pattern: None,
                audited_after: None,
                audited_before: None,
                failure_only: false,
            },
            max_results.max(1),
        )?;
        Ok(HistorySearchResponse {
            query: query.to_string(),
            repo: repo.map(str::to_string),
            branch: branch.map(str::to_string),
            task_type: Some("memory".to_string()),
            session_id: None,
            final_verdict: None,
            incident_pattern: None,
            audited_after: None,
            audited_before: None,
            hit_count: hits.len(),
            hits,
        })
    }

    fn build_session_audit_record(&self, inspected: &InspectedSession) -> Result<AuditRecord> {
        let workspace_root = inspected
            .session
            .launched_from
            .clone()
            .context("session has no launched_from workspace path")?;
        let paths = ensure_workspace_layout(&workspace_root)?;
        let repo_probe = probe_local_repo(Path::new(&workspace_root))?;
        let raw_events = ingest_chat_session(&inspected.session, &inspected.session_path)?;
        let extracted = extract_audit(&inspected.session_id, &raw_events, repo_probe.as_ref());

        Ok(AuditRecord {
            workspace_root: workspace_root.clone(),
            workspace_id: paths.workspace_id,
            provider: inspected.provider.clone(),
            provider_display_name: inspected.provider_display_name.clone(),
            session_id: inspected.session_id.clone(),
            session_name: inspected.session.name.clone(),
            session_path: inspected.session_path.clone(),
            audited_at: Utc::now(),
            raw_events,
            intents: extracted.intents,
            actions: extracted.actions,
            errors: extracted.errors,
            task_run: extracted.task_run,
            repo_probe,
            proposed_memories: Vec::new(),
        })
    }

    fn build_artifact_audit_record(
        &self,
        request: AuditArtifactRequest,
    ) -> Result<(AuditRecord, serde_json::Value)> {
        let paths = ensure_workspace_layout(&request.workspace_path)?;
        let workspace_root = paths.workspace_root.display().to_string();
        let repo_probe = probe_local_repo(paths.workspace_root.as_path())?;
        let raw = load_artifact_text(&request)?;
        let source_uri = request
            .source_uri
            .clone()
            .or_else(|| request.artifact_path.clone())
            .unwrap_or_else(|| format!("inline:{:?}", request.artifact_kind).to_ascii_lowercase());

        let (session_id, session_name, raw_events, artifact_snapshot) = match request.artifact_kind
        {
            ArtifactKind::MarkdownChat => {
                let session_id = request.artifact_id.unwrap_or_else(|| {
                    derived_artifact_id(request.artifact_kind, &source_uri, raw.as_str())
                });
                let events = ingest_markdown_chat(&session_id, &source_uri, &raw)?;
                (
                    session_id,
                    display_session_name("markdown chat", &source_uri),
                    events,
                    json!({
                        "artifact_kind": request.artifact_kind,
                        "source_uri": source_uri.clone(),
                        "artifact_path": request.artifact_path.clone(),
                        "content": raw,
                    }),
                )
            }
            ArtifactKind::JsonlChat => {
                let session_id = request.artifact_id.unwrap_or_else(|| {
                    derived_artifact_id(request.artifact_kind, &source_uri, raw.as_str())
                });
                let events = ingest_jsonl_chat(&session_id, &source_uri, &raw)?;
                (
                    session_id,
                    display_session_name("jsonl chat", &source_uri),
                    events,
                    json!({
                        "artifact_kind": request.artifact_kind,
                        "source_uri": source_uri.clone(),
                        "artifact_path": request.artifact_path.clone(),
                        "content": raw,
                    }),
                )
            }
            ArtifactKind::ShellLog => {
                let session_id = request.artifact_id.unwrap_or_else(|| {
                    derived_artifact_id(request.artifact_kind, &source_uri, raw.as_str())
                });
                let events = ingest_shell_log(&session_id, &source_uri, &raw)?;
                (
                    session_id,
                    display_session_name("shell log", &source_uri),
                    events,
                    json!({
                        "artifact_kind": request.artifact_kind,
                        "source_uri": source_uri.clone(),
                        "artifact_path": request.artifact_path.clone(),
                        "content": raw,
                    }),
                )
            }
            ArtifactKind::ProviderSessionJson => {
                let session: SessionSnapshot = serde_json::from_str(&raw)
                    .context("failed to parse provider session artifact JSON")?;
                let session_id = request.artifact_id.unwrap_or_else(|| {
                    format!(
                        "artifact-provider-{}",
                        stable_hash(&(source_uri.as_str(), session.id.as_str(), raw.as_str()))
                    )
                });
                let events = ingest_provider_session(&session_id, &session, &source_uri)?;
                (
                    session_id,
                    format!("Imported provider session: {}", session.name),
                    events,
                    json!({
                        "artifact_kind": request.artifact_kind,
                        "source_uri": source_uri.clone(),
                        "artifact_path": request.artifact_path.clone(),
                        "session": session,
                    }),
                )
            }
        };
        let extracted = extract_audit(&session_id, &raw_events, repo_probe.as_ref());

        Ok((
            AuditRecord {
                workspace_root,
                workspace_id: paths.workspace_id,
                provider: "artifact_import".to_string(),
                provider_display_name: "Artifact Import".to_string(),
                session_id: session_id.clone(),
                session_name,
                session_path: source_uri,
                audited_at: Utc::now(),
                raw_events,
                intents: extracted.intents,
                actions: extracted.actions,
                errors: extracted.errors,
                task_run: extracted.task_run,
                repo_probe,
                proposed_memories: Vec::new(),
            },
            artifact_snapshot,
        ))
    }

    fn finalize_audit(
        &self,
        mut audit: AuditRecord,
        artifact_snapshot: serde_json::Value,
    ) -> Result<AuditSessionResponse> {
        let paths = ensure_workspace_layout(&audit.workspace_root)?;
        store::ensure_store(&paths)?;

        let existing_memory_texts = store::load_existing_memory_texts(&paths)?;
        let prior_audits = store::list_audit_summaries(&paths)?;
        audit.proposed_memories = compile_memories(&audit, &existing_memory_texts, &prior_audits)?;

        store_artifact_snapshot(&paths, &audit.session_id, &artifact_snapshot)?;
        store::persist_audit(&paths, &audit)?;
        let report_path = write_report(&paths, &audit)?;

        Ok(AuditSessionResponse {
            workspace_root: audit.workspace_root,
            workspace_id: audit.workspace_id,
            session_id: audit.session_id,
            session_name: audit.session_name,
            provider: audit.provider,
            provider_display_name: audit.provider_display_name,
            audited_at: audit.audited_at,
            report_path,
            event_count: audit.raw_events.len(),
            intent_count: audit.intents.len(),
            action_count: audit.actions.len(),
            error_count: audit.errors.len(),
            task_run: audit.task_run,
            repo_probe: audit.repo_probe,
            proposed_memories: audit.proposed_memories,
        })
    }

    fn search_corpus(
        &self,
        query: &str,
        repo: Option<&str>,
        kind: CorpusKind,
        scope: SearchScope<'_>,
        max_results: usize,
    ) -> Result<Vec<CorpusSearchHit>> {
        let workspace_paths = self.workspace_paths_for_repo(repo)?;
        let mut hits = Vec::new();
        for paths in workspace_paths {
            let corpus_dir = match kind {
                CorpusKind::Reports => paths.reports_dir.clone(),
                CorpusKind::Memory => paths.memory_corpus_dir.clone(),
            };
            if !corpus_dir.is_dir() {
                continue;
            }
            let allowed_file_names = allowed_file_names(&paths, kind, scope)?;
            if matches!(allowed_file_names.as_ref(), Some(file_names) if file_names.is_empty()) {
                continue;
            }
            let index = SembleIndex::from_path_with_encoder(
                &corpus_dir,
                HashEmbedder::default(),
                true,
                None,
            )
            .with_context(|| format!("failed to index {}", corpus_dir.display()))?;
            let search_limit = search_limit(max_results, allowed_file_names.as_ref());
            let search_results = index.search(query, search_limit, SearchMode::Hybrid)?;
            for result in search_results {
                if !matches_allowed_file_name(&result.chunk.file_path, allowed_file_names.as_ref())
                {
                    continue;
                }
                hits.push(CorpusSearchHit {
                    workspace_root: workspace_label(&paths, repo),
                    workspace_id: paths.workspace_id.clone(),
                    file_path: result.chunk.file_path,
                    start_line: result.chunk.start_line,
                    end_line: result.chunk.end_line,
                    score: result.score,
                    snippet: result
                        .chunk
                        .content
                        .lines()
                        .take(8)
                        .collect::<Vec<_>>()
                        .join("\n"),
                });
            }
        }
        hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.file_path.cmp(&right.file_path))
        });
        hits.truncate(max_results);
        Ok(hits)
    }

    fn workspace_paths_for_repo(&self, repo: Option<&str>) -> Result<Vec<WorkspaceAuditorPaths>> {
        if let Some(repo) = repo {
            return Ok(vec![ensure_workspace_layout(repo)?]);
        }
        store::list_workspace_paths(&history_auditor_root_dir())
    }
}

#[derive(Clone, Copy, Debug)]
enum CorpusKind {
    Reports,
    Memory,
}

#[derive(Clone, Copy, Debug)]
struct SearchScope<'a> {
    branch: Option<&'a str>,
    task_type: Option<&'a str>,
    error_kind: Option<&'a str>,
    session_id: Option<&'a str>,
    final_verdict: Option<&'a str>,
    incident_pattern: Option<&'a str>,
    audited_after: Option<&'a str>,
    audited_before: Option<&'a str>,
    failure_only: bool,
}

fn store_artifact_snapshot(
    paths: &WorkspaceAuditorPaths,
    session_id: &str,
    artifact_snapshot: &serde_json::Value,
) -> Result<()> {
    let path = artifact_session_path(paths, session_id);
    let json = serde_json::to_vec_pretty(artifact_snapshot)?;
    atomic_write(&path, &json).with_context(|| format!("failed to write {}", path.display()))
}

fn load_artifact_text(request: &AuditArtifactRequest) -> Result<String> {
    if let Some(content) = request.content.clone() {
        return Ok(content);
    }
    let Some(path) = request.artifact_path.as_ref() else {
        bail!("Provide either `content` or `artifact_path` for `audit_artifact`")
    };
    fs::read_to_string(path).with_context(|| format!("failed to read artifact {}", path))
}

fn display_session_name(kind_label: &str, source_uri: &str) -> String {
    let source_name = Path::new(source_uri)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(source_uri);
    format!("Imported {kind_label}: {source_name}")
}

fn derived_artifact_id(kind: ArtifactKind, source_uri: &str, raw: &str) -> String {
    format!(
        "artifact-{}",
        stable_hash(&(format!("{kind:?}").to_ascii_lowercase(), source_uri, raw))
    )
}

fn stable_hash<T: Hash>(value: &T) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn workspace_label(paths: &WorkspaceAuditorPaths, repo: Option<&str>) -> String {
    if paths.workspace_root.as_os_str().is_empty() {
        repo.unwrap_or_default().to_string()
    } else {
        paths.workspace_root.display().to_string()
    }
}

fn allowed_file_names(
    paths: &WorkspaceAuditorPaths,
    kind: CorpusKind,
    scope: SearchScope<'_>,
) -> Result<Option<HashSet<String>>> {
    match kind {
        CorpusKind::Reports => {
            if scope.branch.is_none()
                && scope.task_type.is_none()
                && scope.error_kind.is_none()
                && scope.session_id.is_none()
                && scope.final_verdict.is_none()
                && scope.incident_pattern.is_none()
                && scope.audited_after.is_none()
                && scope.audited_before.is_none()
                && !scope.failure_only
            {
                return Ok(None);
            }
            store::list_report_file_names(
                paths,
                scope.branch,
                scope.task_type,
                scope.error_kind,
                scope.session_id,
                scope.final_verdict,
                scope.incident_pattern,
                scope.audited_after,
                scope.audited_before,
                scope.failure_only,
            )
            .map(Some)
        }
        CorpusKind::Memory => {
            if scope.branch.is_none() {
                return Ok(None);
            }
            store::list_memory_file_names(paths, scope.branch).map(Some)
        }
    }
}

fn matches_allowed_file_name(
    file_path: &str,
    allowed_file_names: Option<&HashSet<String>>,
) -> bool {
    let Some(allowed_file_names) = allowed_file_names else {
        return true;
    };
    Path::new(file_path)
        .file_name()
        .and_then(|value| value.to_str())
        .map(|file_name| allowed_file_names.contains(file_name))
        .unwrap_or(false)
}

fn search_limit(max_results: usize, allowed_file_names: Option<&HashSet<String>>) -> usize {
    let base_limit = max_results.max(1);
    let fanout = allowed_file_names
        .map(|file_names| file_names.len().max(base_limit))
        .unwrap_or(base_limit);
    base_limit.max(16).max(fanout).min(128)
}

fn classify_error_text(error_text: &str) -> Option<&'static str> {
    let normalized = error_text.to_ascii_lowercase();
    [
        ("non-fast-forward", "non_fast_forward"),
        ("non fast forward", "non_fast_forward"),
        ("auth failed", "auth_failed"),
        ("authentication failed", "auth_failed"),
        ("protected branch", "protected_branch"),
        ("pre-receive hook", "pre_receive_hook_declined"),
        ("remote not found", "remote_not_found"),
        ("repository not found", "remote_not_found"),
        ("no upstream", "no_upstream_branch"),
        ("upstream branch", "no_upstream_branch"),
        ("merge conflict", "merge_conflict"),
        ("rebase conflict", "rebase_conflict"),
        ("detached head", "detached_head"),
        ("detached-head", "detached_head"),
        ("nothing to commit", "nothing_to_commit"),
        ("dirty worktree", "dirty_worktree"),
        ("working tree dirty", "dirty_worktree"),
        ("working tree is dirty", "dirty_worktree"),
        ("test failure", "test_failure"),
        ("tests failed", "test_failure"),
        ("dependency failure", "dependency_failure"),
        ("permission denied", "permission_denied"),
    ]
    .into_iter()
    .find_map(|(needle, label)| normalized.contains(needle).then_some(label))
}

fn normalize_timestamp_filter(value: Option<&str>) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let parsed = chrono::DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("invalid RFC3339 timestamp filter `{value}`"))?;
    Ok(Some(parsed.with_timezone(&Utc).to_rfc3339()))
}
