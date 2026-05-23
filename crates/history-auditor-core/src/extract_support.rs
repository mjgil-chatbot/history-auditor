use super::schema::{
    ActionEvent, ActionKind, ErrorEvent, IntentEvent, IntentKind, MergeVerdict, PushVerdict,
    TaskVerdict,
};

pub(super) fn append_expected_action_gaps(
    intents: &[IntentEvent],
    actions: &[ActionEvent],
    missing: &mut Vec<String>,
) {
    let expects_commit = intents
        .iter()
        .any(|intent| intent.kind == IntentKind::CommitChanges);
    let expects_push = intents
        .iter()
        .any(|intent| intent.kind == IntentKind::PushBranch);
    let expects_merge = intents
        .iter()
        .any(|intent| intent.kind == IntentKind::MergeBranch);
    let expects_tests = intents
        .iter()
        .any(|intent| intent.kind == IntentKind::RunTests);

    if expects_commit
        && !actions
            .iter()
            .any(|action| action.kind == ActionKind::GitCommit)
    {
        push_missing_once(
            missing,
            "The request asked for a commit, but no captured git commit action was recorded.",
        );
    }
    if expects_push
        && !actions
            .iter()
            .any(|action| action.kind == ActionKind::GitPush)
    {
        push_missing_once(
            missing,
            "The request asked for a push, but no captured git push action was recorded.",
        );
    }
    if expects_merge
        && !actions
            .iter()
            .any(|action| matches!(action.kind, ActionKind::GitMerge | ActionKind::GitRebase))
    {
        push_missing_once(
            missing,
            "The request asked for a merge or rebase, but no captured merge action was recorded.",
        );
    }
    if expects_tests
        && !actions
            .iter()
            .any(|action| action.kind == ActionKind::RunTests)
    {
        push_missing_once(
            missing,
            "The request asked for tests, but no captured test execution was recorded.",
        );
    }
}

pub(super) fn final_task_verdict(
    push_verdict: &PushVerdict,
    merge_verdict: &MergeVerdict,
    errors: &[ErrorEvent],
    recommended_fix_present: bool,
    missing_evidence: &[String],
    actions: &[ActionEvent],
    contradictions_present: bool,
) -> TaskVerdict {
    let has_failed_actions = actions
        .iter()
        .any(|action| action.status == super::schema::ActionStatus::Failed);
    match push_verdict {
        PushVerdict::PushFailed => TaskVerdict::TaskBlockedWithClearFix,
        PushVerdict::PushSucceededFromToolOutput if contradictions_present => {
            TaskVerdict::AmbiguousMissingEvidence
        }
        PushVerdict::PushSucceededFromToolOutput
            if errors.is_empty() && missing_evidence.is_empty() =>
        {
            TaskVerdict::TaskCompleted
        }
        _ if *merge_verdict == MergeVerdict::MergeConflict => TaskVerdict::TaskBlockedWithClearFix,
        _ if contradictions_present => TaskVerdict::AmbiguousMissingEvidence,
        _ if !errors.is_empty() && recommended_fix_present => TaskVerdict::TaskBlockedWithClearFix,
        _ if !errors.is_empty() => TaskVerdict::TaskFailed,
        _ if has_failed_actions && recommended_fix_present => TaskVerdict::TaskBlockedWithClearFix,
        _ if has_failed_actions => TaskVerdict::TaskFailed,
        _ if !actions.is_empty() && missing_evidence.is_empty() => TaskVerdict::TaskCompleted,
        _ => TaskVerdict::AmbiguousMissingEvidence,
    }
}

pub(super) fn confidence_from_evidence_tier(evidence_tier: u8) -> f32 {
    match evidence_tier {
        5 => 0.96,
        4 => 0.88,
        3 => 0.74,
        2 => 0.58,
        _ => 0.35,
    }
}

fn push_missing_once(missing: &mut Vec<String>, message: &str) {
    if missing.iter().any(|existing| existing == message) {
        return;
    }
    missing.push(message.to_string());
}
