use super::schema::{AuditRecord, ErrorKind, StoredAuditSummary};
use super::store_support::task_type;
pub(super) fn incident_pattern_for_audit(audit: &AuditRecord) -> Option<String> {
    let error_kind = audit.errors.first().map(|error| &error.kind)?;
    incident_pattern_label(task_type(audit), error_kind).map(str::to_string)
}
pub(super) fn incident_pattern_for_summary(summary: &StoredAuditSummary) -> Option<String> {
    summary.incident_pattern.clone().or_else(|| {
        summary
            .primary_error_kind
            .as_ref()
            .and_then(|error_kind| incident_pattern_label("unknown", error_kind))
            .map(str::to_string)
    })
}
pub(super) fn incident_pattern_label(
    task_type: &str,
    error_kind: &ErrorKind,
) -> Option<&'static str> {
    let pattern = match (task_type, error_kind) {
        ("git_delivery", ErrorKind::NonFastForward) => "git_delivery_push_non_fast_forward",
        ("git_delivery", ErrorKind::AuthFailed) => "git_delivery_push_auth_failed",
        ("git_delivery", ErrorKind::ProtectedBranch) => "git_delivery_push_protected_branch",
        ("git_delivery", ErrorKind::PreReceiveHookDeclined) => {
            "git_delivery_push_pre_receive_hook_declined"
        }
        ("git_delivery", ErrorKind::RemoteNotFound) => "git_delivery_push_remote_not_found",
        ("git_delivery", ErrorKind::NoUpstreamBranch) => "git_delivery_push_no_upstream_branch",
        ("git_delivery", ErrorKind::MergeConflict) => "git_delivery_merge_conflict",
        ("git_delivery", ErrorKind::RebaseConflict) => "git_delivery_rebase_conflict",
        ("git_delivery", ErrorKind::DetachedHead) => "git_delivery_push_detached_head",
        ("git_delivery", ErrorKind::NothingToCommit) => "git_delivery_nothing_to_commit",
        ("git_delivery", ErrorKind::DirtyWorktree) => "git_delivery_dirty_worktree",
        ("git_delivery", ErrorKind::TestFailure) => "git_delivery_test_failure",
        ("git_delivery", ErrorKind::DependencyFailure) => "git_delivery_dependency_failure",
        ("git_delivery", ErrorKind::PermissionDenied) => "git_delivery_permission_denied",
        ("test_run", ErrorKind::TestFailure) => "test_run_test_failure",
        ("test_run", ErrorKind::DependencyFailure) => "test_run_dependency_failure",
        (_, ErrorKind::NonFastForward) => "non_fast_forward",
        (_, ErrorKind::AuthFailed) => "auth_failed",
        (_, ErrorKind::ProtectedBranch) => "protected_branch",
        (_, ErrorKind::PreReceiveHookDeclined) => "pre_receive_hook_declined",
        (_, ErrorKind::RemoteNotFound) => "remote_not_found",
        (_, ErrorKind::NoUpstreamBranch) => "no_upstream_branch",
        (_, ErrorKind::MergeConflict) => "merge_conflict",
        (_, ErrorKind::RebaseConflict) => "rebase_conflict",
        (_, ErrorKind::DetachedHead) => "detached_head",
        (_, ErrorKind::NothingToCommit) => "nothing_to_commit",
        (_, ErrorKind::DirtyWorktree) => "dirty_worktree",
        (_, ErrorKind::TestFailure) => "test_failure",
        (_, ErrorKind::DependencyFailure) => "dependency_failure",
        (_, ErrorKind::PermissionDenied) => "permission_denied",
        (_, ErrorKind::Unknown) => return None,
    };
    Some(pattern)
}
