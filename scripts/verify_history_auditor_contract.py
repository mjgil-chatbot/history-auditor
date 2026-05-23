#!/usr/bin/env python3
import re
import subprocess
import sys
from pathlib import Path


ARCH_DOC_REL = Path("docs/ARCHITECTURE.md")
VERIFY_DOC_REL = Path("docs/verification.md")
REGRESSION_SCRIPT_REL = Path("scripts/verify-history-auditor-regressions.sh")
WORKFLOW_RELS = {
    Path(".github/workflows/ci.yml"): (
        "python3 scripts/verify_history_auditor_contract.py",
        "cargo test --workspace",
        "./scripts/verify-history-auditor-regressions.sh",
        "cargo build --workspace --release",
    ),
    Path(".github/workflows/fmt.yml"): (
        "cargo fmt --all -- --check",
    ),
}

REQUIRED_DOC_SNIPPETS = {
    ARCH_DOC_REL: (
        "history-auditor-core",
        "history-auditor-mcp",
        "SessionSource",
        ".github/workflows/ci.yml",
        ".github/workflows/fmt.yml",
        "verify_history_auditor_contract.py",
        "verify-history-auditor-regressions.sh",
    ),
    VERIFY_DOC_REL: (
        "## History Auditor Contract",
        "SessionSource",
        "focused release regression bundle",
        ".github/workflows/ci.yml",
        ".github/workflows/fmt.yml",
        "verify_history_auditor_contract.py",
        "verify-history-auditor-regressions.sh",
    ),
}

REQUIRED_TESTS = {
    Path("crates/history-auditor-core/src/tests.rs"): (
        "audit_session_persists_report_and_detects_non_fast_forward",
        "approve_memory_and_search_memory_corpus_round_trip",
        "audit_history_and_explain_task_return_saved_results",
        "audit_history_honors_branch_and_task_type_filters",
        "persisted_audit_records_include_typed_events_and_edges",
        "relevant_memories_honor_branch_filters",
        "ingest_helpers_accept_markdown_jsonl_and_shell_logs",
        "audit_artifact_accepts_inline_markdown_and_jsonl_artifacts",
        "audit_artifact_reads_shell_log_from_path_and_supports_task_explanation",
        "audit_artifact_accepts_provider_session_json",
        "missing_requested_push_evidence_keeps_delivery_task_ambiguous",
        "repeated_non_fast_forward_failures_promote_supported_procedural_memory",
        "preference_and_claim_contradiction_memories_are_proposed",
    ),
    Path("crates/history-auditor-core/src/retrieval_tests.rs"): (
        "audit_history_honors_session_and_final_verdict_filters",
        "find_similar_failures_honors_branch_session_and_failure_filters",
        "find_similar_failures_infers_nothing_to_commit_incident_pattern",
        "find_similar_failures_infers_dirty_worktree_incident_pattern",
    ),
    Path("crates/history-auditor-core/src/memory_tests.rs"): (
        "repeated_auth_failures_promote_supported_procedural_memory",
        "repeated_protected_branch_failures_promote_supported_procedural_memory",
        "repeated_nothing_to_commit_promote_supported_procedural_memory",
        "approving_stronger_memory_supersedes_weaker_related_memory",
        "repeated_no_upstream_failures_promote_supported_procedural_memory",
        "repeated_pre_receive_failures_need_multiple_supporting_tasks_for_repo_fact",
        "repeated_dependency_failures_promote_supported_procedural_memory",
    ),
    Path("crates/history-auditor-core/src/extraction_tests.rs"): (
        "failed_action_without_normalized_error_uses_action_root_cause_and_blocks_task",
        "contradictory_uncertainty_claim_keeps_success_shaped_task_ambiguous",
        "exit_zero_push_without_remote_update_keeps_task_ambiguous",
        "locally_confirmed_commit_uses_commit_exists_locally_verdict",
        "unconfirmed_commit_output_keeps_commit_only_task_ambiguous",
        "locally_confirmed_merge_uses_local_merge_completed_verdict",
        "ambiguous_merge_activity_keeps_push_followup_task_ambiguous",
        "rebase_conflict_uses_merge_conflict_verdict_and_blocks_task",
        "ambiguous_rebase_activity_keeps_push_followup_task_ambiguous",
        "successful_merge_and_push_can_still_complete",
    ),
}

REQUIRED_REGRESSION_TESTS = (
    "audit_session_persists_report_and_detects_non_fast_forward",
    "approve_memory_and_search_memory_corpus_round_trip",
    "audit_history_and_explain_task_return_saved_results",
    "audit_history_honors_branch_and_task_type_filters",
    "persisted_audit_records_include_typed_events_and_edges",
    "relevant_memories_honor_branch_filters",
    "ingest_helpers_accept_markdown_jsonl_and_shell_logs",
    "audit_artifact_accepts_inline_markdown_and_jsonl_artifacts",
    "audit_artifact_reads_shell_log_from_path_and_supports_task_explanation",
    "audit_artifact_accepts_provider_session_json",
    "missing_requested_push_evidence_keeps_delivery_task_ambiguous",
    "repeated_non_fast_forward_failures_promote_supported_procedural_memory",
    "preference_and_claim_contradiction_memories_are_proposed",
    "audit_history_honors_session_and_final_verdict_filters",
    "find_similar_failures_honors_branch_session_and_failure_filters",
    "repeated_auth_failures_promote_supported_procedural_memory",
    "repeated_protected_branch_failures_promote_supported_procedural_memory",
    "repeated_nothing_to_commit_promote_supported_procedural_memory",
    "approving_stronger_memory_supersedes_weaker_related_memory",
    "repeated_no_upstream_failures_promote_supported_procedural_memory",
    "repeated_pre_receive_failures_need_multiple_supporting_tasks_for_repo_fact",
    "repeated_dependency_failures_promote_supported_procedural_memory",
    "find_similar_failures_infers_nothing_to_commit_incident_pattern",
    "find_similar_failures_infers_dirty_worktree_incident_pattern",
    "failed_action_without_normalized_error_uses_action_root_cause_and_blocks_task",
    "contradictory_uncertainty_claim_keeps_success_shaped_task_ambiguous",
    "exit_zero_push_without_remote_update_keeps_task_ambiguous",
    "locally_confirmed_commit_uses_commit_exists_locally_verdict",
    "unconfirmed_commit_output_keeps_commit_only_task_ambiguous",
    "locally_confirmed_merge_uses_local_merge_completed_verdict",
    "ambiguous_merge_activity_keeps_push_followup_task_ambiguous",
    "rebase_conflict_uses_merge_conflict_verdict_and_blocks_task",
    "ambiguous_rebase_activity_keeps_push_followup_task_ambiguous",
    "successful_merge_and_push_can_still_complete",
)


def repo_root() -> Path:
    script_repo_root = Path(__file__).resolve().parent.parent
    result = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        check=False,
        cwd=script_repo_root,
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        return Path(result.stdout.strip()).resolve()
    return script_repo_root


def verify_docs(root: Path) -> list[str]:
    failures: list[str] = []
    for relative, snippets in REQUIRED_DOC_SNIPPETS.items():
        path = root / relative
        if not path.is_file():
            failures.append(f"missing documentation artifact: {relative.as_posix()}")
            continue
        content = path.read_text(encoding="utf-8")
        for snippet in snippets:
            if snippet not in content:
                failures.append(
                    f"{relative.as_posix()}: missing required snippet `{snippet}`"
                )
    return failures


def verify_tests(root: Path) -> list[str]:
    failures: list[str] = []
    for relative, test_names in REQUIRED_TESTS.items():
        path = root / relative
        if not path.is_file():
            failures.append(f"missing regression source file: {relative.as_posix()}")
            continue
        content = path.read_text(encoding="utf-8")
        for test_name in test_names:
            if re.search(rf"\bfn\s+{re.escape(test_name)}\s*\(", content) is None:
                failures.append(
                    f"{relative.as_posix()}: missing regression `{test_name}`"
                )
    return failures


def verify_regression_script(root: Path) -> list[str]:
    path = root / REGRESSION_SCRIPT_REL
    if not path.is_file():
        return [f"missing regression script: {REGRESSION_SCRIPT_REL.as_posix()}"]
    content = path.read_text(encoding="utf-8")
    return [
        f"{REGRESSION_SCRIPT_REL.as_posix()}: missing focused regression `{test_name}`"
        for test_name in REQUIRED_REGRESSION_TESTS
        if test_name not in content
    ]


def verify_workflows(root: Path) -> list[str]:
    failures: list[str] = []
    for relative, snippets in WORKFLOW_RELS.items():
        path = root / relative
        if not path.is_file():
            failures.append(f"missing workflow: {relative.as_posix()}")
            continue
        content = path.read_text(encoding="utf-8")
        for snippet in snippets:
            if snippet not in content:
                failures.append(
                    f"{relative.as_posix()}: missing workflow command `{snippet}`"
                )
    return failures


def main() -> int:
    root = repo_root()
    failures = [
        *verify_docs(root),
        *verify_tests(root),
        *verify_regression_script(root),
        *verify_workflows(root),
    ]
    if failures:
        for failure in failures:
            print(f"history-auditor-contract: FAIL: {failure}", file=sys.stderr)
        return 1
    print(
        "history-auditor-contract: PASS: verified docs, regressions, and contract wiring"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
