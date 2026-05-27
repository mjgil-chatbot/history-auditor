use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, InitializeResult, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ServerHandler};
use serde::Serialize;

use super::tools::{
    ApproveMemoryInput, AuditArtifactInput, AuditHistoryInput, AuditSessionInput, ExplainTaskInput,
    RelevantMemoriesInput, RetractMemoryInput, SimilarFailuresInput,
};
use history_auditor_core::service::{AuditArtifactRequest, HistoryAuditorService};

#[derive(Clone)]
pub struct HistoryAuditorMcpHandler {
    service: HistoryAuditorService,
    tool_router: ToolRouter<Self>,
}

impl HistoryAuditorMcpHandler {
    pub fn new(service: HistoryAuditorService) -> Self {
        Self {
            service,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl HistoryAuditorMcpHandler {
    /// Audit one session and persist the evidence-backed report.
    #[tool(name = "audit_session")]
    async fn audit_session(&self, Parameters(input): Parameters<AuditSessionInput>) -> String {
        let service = self.service.clone();
        run_blocking_json(move || service.audit_session(&input.session_id)).await
    }

    /// Audit one imported artifact and persist the evidence-backed report.
    #[tool(name = "audit_artifact")]
    async fn audit_artifact(&self, Parameters(input): Parameters<AuditArtifactInput>) -> String {
        let service = self.service.clone();
        run_blocking_json(move || {
            service.audit_artifact(AuditArtifactRequest {
                workspace_path: input.workspace_path,
                artifact_kind: input.artifact_kind,
                artifact_id: input.artifact_id,
                source_uri: input.source_uri,
                artifact_path: input.artifact_path,
                content: input.content,
            })
        })
        .await
    }

    /// Search stored audit reports for similar prior work or failures.
    #[tool(name = "audit_history")]
    async fn audit_history(&self, Parameters(input): Parameters<AuditHistoryInput>) -> String {
        let service = self.service.clone();
        run_blocking_json(move || {
            service.audit_history(
                &input.query,
                input.repo.as_deref(),
                input.branch.as_deref(),
                input.task_type.as_deref(),
                input.session_id.as_deref(),
                input.final_verdict.as_deref(),
                input.incident_pattern.as_deref(),
                input.audited_after.as_deref(),
                input.audited_before.as_deref(),
            )
        })
        .await
    }

    /// Explain the typed verdict and evidence for a previously audited task.
    #[tool(name = "explain_task")]
    async fn explain_task(&self, Parameters(input): Parameters<ExplainTaskInput>) -> String {
        let service = self.service.clone();
        run_blocking_json(move || service.explain_task(&input.task_id)).await
    }

    /// Find similar prior failures from stored reports.
    #[tool(name = "find_similar_failures")]
    async fn find_similar_failures(
        &self,
        Parameters(input): Parameters<SimilarFailuresInput>,
    ) -> String {
        let service = self.service.clone();
        run_blocking_json(move || {
            service.find_similar_failures(
                &input.error_text,
                input.repo.as_deref(),
                input.branch.as_deref(),
                input.task_type.as_deref(),
                input.session_id.as_deref(),
                input.final_verdict.as_deref(),
                input.incident_pattern.as_deref(),
                input.audited_after.as_deref(),
                input.audited_before.as_deref(),
            )
        })
        .await
    }

    /// Search approved history-auditor memories.
    #[tool(name = "get_relevant_memories")]
    async fn get_relevant_memories(
        &self,
        Parameters(input): Parameters<RelevantMemoriesInput>,
    ) -> String {
        let service = self.service.clone();
        run_blocking_json(move || {
            service.get_relevant_memories(
                &input.query,
                input.repo.as_deref(),
                input.branch.as_deref(),
                input.max_results,
            )
        })
        .await
    }

    /// Recompute and list memory proposals for one session.
    #[tool(name = "propose_memory_writes")]
    async fn propose_memory_writes(
        &self,
        Parameters(input): Parameters<AuditSessionInput>,
    ) -> String {
        let service = self.service.clone();
        run_blocking_json(move || service.propose_memory_writes(&input.session_id)).await
    }

    /// Approve one proposed memory into the durable retrieval corpus.
    #[tool(name = "approve_memory")]
    async fn approve_memory(&self, Parameters(input): Parameters<ApproveMemoryInput>) -> String {
        let service = self.service.clone();
        run_blocking_json(move || service.approve_memory(&input.memory_id)).await
    }

    /// Retract a previously proposed or approved memory.
    #[tool(name = "retract_memory")]
    async fn retract_memory(&self, Parameters(input): Parameters<RetractMemoryInput>) -> String {
        let service = self.service.clone();
        run_blocking_json(move || service.retract_memory(&input.memory_id, input.reason)).await
    }
}

#[tool_handler]
impl ServerHandler for HistoryAuditorMcpHandler {
    fn get_info(&self) -> ServerInfo {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "history-auditor-mcp",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(concat!(
                "History Auditor MCP. Tools:\n\n",
                "- audit_session: normalize one session into an evidence-backed task audit\n\n",
                "- audit_artifact: normalize one imported markdown, JSONL, shell-log, or provider-session artifact into an evidence-backed task audit\n\n",
                "- audit_history: search stored audit reports\n\n",
                "- explain_task: return the typed verdict for one audited task\n\n",
                "- find_similar_failures: search prior failure reports\n\n",
                "- get_relevant_memories: search approved history-auditor memories\n\n",
                "- propose_memory_writes: recompute pending memory proposals for one session\n\n",
                "- approve_memory: promote a proposal into the durable memory corpus\n\n",
                "- retract_memory: retract a previously stored memory",
            ))
    }
}

async fn run_blocking_json<F, T>(f: F) -> String
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Serialize + Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(result)) => to_pretty_json(&result),
        Ok(Err(error)) => to_pretty_json(&ErrorResponse {
            error: error.to_string(),
        }),
        Err(error) => to_pretty_json(&ErrorResponse {
            error: format!("Request task panicked: {error}"),
        }),
    }
}

fn to_pretty_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}
