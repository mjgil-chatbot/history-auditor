use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    MarkdownChat,
    JsonlChat,
    ShellLog,
    ProviderSessionJson,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RawActor {
    User,
    Assistant,
    Tool,
    System,
    External,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RawEventKind {
    Message,
    ToolCall,
    ToolResult,
    ShellCommand,
    ShellResult,
    FileChange,
    Error,
    Summary,
    Artifact,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RawEvent {
    pub event_id: String,
    pub session_id: String,
    pub ordinal: usize,
    pub actor: RawActor,
    pub kind: RawEventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
    pub source_uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_span: Option<String>,
    pub raw_content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_args_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub content_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IntentKind {
    CommitChanges,
    PushBranch,
    MergeBranch,
    FixError,
    RunTests,
    EditFiles,
    Investigate,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct IntentEvent {
    pub intent_id: String,
    pub kind: IntentKind,
    pub evidence_event_ids: Vec<String>,
    pub confidence: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Intended,
    Attempted,
    Succeeded,
    Failed,
    Skipped,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    GitStatus,
    GitAdd,
    GitCommit,
    GitPush,
    GitPull,
    GitFetch,
    GitMerge,
    GitRebase,
    GitCheckout,
    GitLog,
    EditFile,
    RunTests,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ActionEvent {
    pub action_id: String,
    pub kind: ActionKind,
    pub status: ActionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_hash: Option<String>,
    pub evidence_event_ids: Vec<String>,
    pub evidence_tier: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    NonFastForward,
    AuthFailed,
    ProtectedBranch,
    PreReceiveHookDeclined,
    RemoteNotFound,
    NoUpstreamBranch,
    MergeConflict,
    RebaseConflict,
    DetachedHead,
    NothingToCommit,
    DirtyWorktree,
    TestFailure,
    DependencyFailure,
    PermissionDenied,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ErrorEvent {
    pub error_id: String,
    pub kind: ErrorKind,
    pub message: String,
    pub evidence_event_ids: Vec<String>,
    pub evidence_tier: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommitVerdict {
    CommitNotAttempted,
    CommitCreatedFromLog,
    CommitExistsLocally,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PushVerdict {
    PushNotAttempted,
    PushSucceededFromToolOutput,
    PushFailed,
    RemotePushUnknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MergeVerdict {
    MergeNotAttempted,
    LocalMergeCompleted,
    MergeConflict,
    RemoteMergeUnknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskVerdict {
    TaskCompleted,
    TaskBlockedWithClearFix,
    TaskFailed,
    AmbiguousMissingEvidence,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    TaskCompleted,
    TaskBlocked,
    PushSucceeded,
    PushFailed,
    CommitCreated,
    TestsPassed,
    TestsFailed,
    NeedsMoreInformation,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ClaimEvent {
    pub claim_id: String,
    pub kind: ClaimKind,
    pub summary: String,
    pub evidence_event_ids: Vec<String>,
    pub confidence: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StructuredNote {
    pub summary: String,
    pub evidence_event_ids: Vec<String>,
    pub confidence: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TaskRun {
    pub task_id: String,
    pub session_id: String,
    pub requested_goal: String,
    #[serde(default)]
    pub requested_goal_evidence_event_ids: Vec<String>,
    pub expected_actions: Vec<String>,
    pub observed_action_ids: Vec<String>,
    pub error_ids: Vec<String>,
    pub commit_verdict: CommitVerdict,
    pub push_verdict: PushVerdict,
    pub merge_verdict: MergeVerdict,
    pub final_verdict: TaskVerdict,
    #[serde(default)]
    pub intent_notes: Vec<StructuredNote>,
    #[serde(default)]
    pub assistant_claims: Vec<ClaimEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_cause: Option<String>,
    #[serde(default)]
    pub root_cause_evidence_event_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_fix: Option<String>,
    #[serde(default)]
    pub recommended_fix_evidence_event_ids: Vec<String>,
    #[serde(default)]
    pub missing_evidence: Vec<String>,
    pub confidence: f32,
    pub evidence_tier: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Episodic,
    Procedural,
    SemanticFact,
    AntiPattern,
    RepoFact,
    UserPreference,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    Proposed,
    Approved,
    Retracted,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MemoryRecord {
    pub memory_id: String,
    pub session_id: String,
    pub task_id: String,
    pub kind: MemoryKind,
    pub status: MemoryStatus,
    pub text: String,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub evidence_event_ids: Vec<String>,
    #[serde(default)]
    pub supporting_task_ids: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RepoProbe {
    pub repo_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_parent_count: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_subject: Option<String>,
    #[serde(default)]
    pub status_lines: Vec<String>,
    #[serde(default)]
    pub cached_status_lines: Vec<String>,
    #[serde(default)]
    pub recent_commits: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AuditRecord {
    pub workspace_root: String,
    pub workspace_id: String,
    pub provider: String,
    pub provider_display_name: String,
    pub session_id: String,
    pub session_name: String,
    pub session_path: String,
    pub audited_at: DateTime<Utc>,
    pub raw_events: Vec<RawEvent>,
    pub intents: Vec<IntentEvent>,
    pub actions: Vec<ActionEvent>,
    pub errors: Vec<ErrorEvent>,
    pub task_run: TaskRun,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_probe: Option<RepoProbe>,
    #[serde(default)]
    pub proposed_memories: Vec<MemoryRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CorpusSearchHit {
    pub workspace_root: String,
    pub workspace_id: String,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub score: f32,
    pub snippet: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoredAuditSummary {
    pub session_id: String,
    pub audited_at: DateTime<Utc>,
    pub branch: Option<String>,
    pub primary_error_kind: Option<ErrorKind>,
    pub incident_pattern: Option<String>,
    pub task_run: TaskRun,
}
