use serde::Deserialize;

use history_auditor_core::schema::ArtifactKind;

fn default_max_results() -> usize {
    5
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AuditSessionInput {
    /// Rust Chatbot session id to audit. Accepts the full id or a unique prefix.
    pub session_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AuditArtifactInput {
    /// Absolute workspace path used to scope storage and the read-only repo probe.
    pub workspace_path: String,
    /// Artifact format to normalize before audit extraction.
    pub artifact_kind: ArtifactKind,
    /// Optional stable audit id. If omitted, the auditor derives one.
    #[serde(default)]
    pub artifact_id: Option<String>,
    /// Optional logical source URI shown in evidence and persisted artifacts.
    #[serde(default)]
    pub source_uri: Option<String>,
    /// Optional filesystem path for loading the artifact text.
    #[serde(default)]
    pub artifact_path: Option<String>,
    /// Optional inline artifact content. Provide this or `artifact_path`.
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AuditHistoryInput {
    /// Natural-language query used to search existing audit reports.
    pub query: String,
    /// Optional absolute workspace path used to scope the search.
    #[serde(default)]
    pub repo: Option<String>,
    /// Optional exact branch filter applied before semantic ranking.
    #[serde(default)]
    pub branch: Option<String>,
    /// Optional exact task-type filter applied before semantic ranking.
    #[serde(default)]
    pub task_type: Option<String>,
    /// Optional exact audited session/task id filter applied before semantic ranking.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Optional exact final-verdict filter applied before semantic ranking.
    #[serde(default)]
    pub final_verdict: Option<String>,
    /// Optional exact normalized incident-pattern filter applied before semantic ranking.
    #[serde(default)]
    pub incident_pattern: Option<String>,
    /// Optional RFC3339 lower timestamp bound applied before semantic ranking.
    #[serde(default)]
    pub audited_after: Option<String>,
    /// Optional RFC3339 upper timestamp bound applied before semantic ranking.
    #[serde(default)]
    pub audited_before: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExplainTaskInput {
    /// Audited task/session id to explain.
    pub task_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SimilarFailuresInput {
    /// Error text to compare against stored audited failures.
    pub error_text: String,
    /// Optional absolute workspace path used to scope the search.
    #[serde(default)]
    pub repo: Option<String>,
    /// Optional exact branch filter applied before semantic ranking.
    #[serde(default)]
    pub branch: Option<String>,
    /// Optional exact task-type filter applied before semantic ranking.
    #[serde(default)]
    pub task_type: Option<String>,
    /// Optional exact audited session/task id filter applied before semantic ranking.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Optional exact final-verdict filter applied before semantic ranking.
    #[serde(default)]
    pub final_verdict: Option<String>,
    /// Optional exact normalized incident-pattern filter applied before semantic ranking.
    #[serde(default)]
    pub incident_pattern: Option<String>,
    /// Optional RFC3339 lower timestamp bound applied before semantic ranking.
    #[serde(default)]
    pub audited_after: Option<String>,
    /// Optional RFC3339 upper timestamp bound applied before semantic ranking.
    #[serde(default)]
    pub audited_before: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RelevantMemoriesInput {
    /// Query used to search approved history-auditor memories.
    pub query: String,
    /// Optional absolute workspace path used to scope the search.
    #[serde(default)]
    pub repo: Option<String>,
    /// Optional exact branch filter applied before semantic ranking.
    #[serde(default)]
    pub branch: Option<String>,
    /// Maximum number of returned memory hits.
    #[serde(default = "default_max_results")]
    pub max_results: usize,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ApproveMemoryInput {
    /// Proposed memory id returned by `audit_session` or `propose_memory_writes`.
    pub memory_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RetractMemoryInput {
    /// Memory id to retract from the durable history-auditor store.
    pub memory_id: String,
    /// Optional reason recorded alongside the retraction.
    #[serde(default)]
    pub reason: Option<String>,
}
