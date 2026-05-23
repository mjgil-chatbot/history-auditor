use super::schema::{
    ActionEvent, ActionKind, ActionStatus, ClaimEvent, ClaimKind, ErrorEvent, ErrorKind,
    IntentEvent, IntentKind, RawActor, RawEvent, StructuredNote,
};

#[derive(Clone, Debug)]
pub(super) struct StructuredExtraction {
    pub intent_notes: Vec<StructuredNote>,
    pub assistant_claims: Vec<ClaimEvent>,
    pub root_cause: Option<String>,
    pub root_cause_evidence_event_ids: Vec<String>,
    pub recommended_fix: Option<String>,
    pub recommended_fix_evidence_event_ids: Vec<String>,
    pub contradictions: Vec<String>,
}

pub(super) fn extract_structured_context(
    events: &[RawEvent],
    intents: &[IntentEvent],
    actions: &[ActionEvent],
    errors: &[ErrorEvent],
) -> StructuredExtraction {
    let intent_notes = extract_intent_notes(events, intents);
    let assistant_claims = extract_assistant_claims(events);
    let (root_cause, root_cause_evidence_event_ids) = root_cause(events, actions, errors);
    let (recommended_fix, recommended_fix_evidence_event_ids) =
        recommended_fix(events, actions, errors.first());
    let contradictions = claim_contradictions(&assistant_claims, actions, errors);
    StructuredExtraction {
        intent_notes,
        assistant_claims,
        root_cause,
        root_cause_evidence_event_ids,
        recommended_fix,
        recommended_fix_evidence_event_ids,
        contradictions,
    }
}

fn root_cause(
    events: &[RawEvent],
    actions: &[ActionEvent],
    errors: &[ErrorEvent],
) -> (Option<String>, Vec<String>) {
    if let Some(error) = errors.first() {
        return (
            Some(error.message.clone()),
            error.evidence_event_ids.clone(),
        );
    }
    if let Some(action) = actions
        .iter()
        .find(|action| action.status == ActionStatus::Failed)
    {
        let detail = action
            .stderr_excerpt
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or(action.stdout_excerpt.as_deref())
            .map(first_meaningful_line)
            .unwrap_or_else(|| "The command failed without a captured explanation.".to_string());
        let command = action.command.as_deref().unwrap_or("captured command");
        return (
            Some(format!(
                "Captured failed command `{command}` reported: {detail}"
            )),
            action.evidence_event_ids.clone(),
        );
    }
    let Some(event) = events.iter().rev().find(|event| {
        event.actor == RawActor::Assistant && contains_root_cause_language(&event.raw_content)
    }) else {
        return (None, Vec::new());
    };
    (
        Some(first_meaningful_line(&event.raw_content)),
        vec![event.event_id.clone()],
    )
}

fn recommended_fix(
    events: &[RawEvent],
    actions: &[ActionEvent],
    error: Option<&ErrorEvent>,
) -> (Option<String>, Vec<String>) {
    if let Some(error) = error {
        let fix = match error.kind {
            ErrorKind::NonFastForward => Some(
                "Run `git fetch origin`, inspect branch divergence, rebase or merge safely, rerun tests, then push again."
                    .to_string(),
            ),
            ErrorKind::MergeConflict => Some(
                "Resolve the merge conflict, rerun tests, and repeat the merge or rebase with fresh evidence."
                    .to_string(),
            ),
            ErrorKind::RebaseConflict => Some(
                "Resolve the rebase conflicts, rerun the relevant tests, and continue the rebase with fresh evidence."
                    .to_string(),
            ),
            ErrorKind::ProtectedBranch => Some(
                "Push to a feature branch or open the expected review path instead of pushing directly to the protected branch."
                    .to_string(),
            ),
            ErrorKind::PreReceiveHookDeclined => Some(
                "Inspect the remote hook policy message, satisfy the required checks, rerun validation, and retry the push."
                    .to_string(),
            ),
            ErrorKind::TestFailure => Some(
                "Fix the failing tests, rerun the suite, and only then continue with commit or push steps."
                    .to_string(),
            ),
            ErrorKind::NoUpstreamBranch => Some(
                "Set the branch upstream explicitly before retrying the push.".to_string(),
            ),
            ErrorKind::AuthFailed => Some(
                "Refresh the remote credentials or auth context, verify the target remote, and retry the push with captured output."
                    .to_string(),
            ),
            ErrorKind::RemoteNotFound => Some(
                "Verify the remote name and URL, correct the destination if needed, then retry the push."
                    .to_string(),
            ),
            ErrorKind::DetachedHead => Some(
                "Check out the intended branch before retrying commit or push operations.".to_string(),
            ),
            ErrorKind::NothingToCommit => Some(
                "Inspect the diff and staging area to confirm there are actual changes before retrying the commit."
                    .to_string(),
            ),
            ErrorKind::DirtyWorktree => Some(
                "Inspect the pending worktree changes, clean or commit them intentionally, then retry the workflow."
                    .to_string(),
            ),
            ErrorKind::DependencyFailure => Some(
                "Repair the missing or failing dependency, rerun the blocked command, and only then continue the workflow."
                    .to_string(),
            ),
            ErrorKind::PermissionDenied => Some(
                "Verify filesystem or remote permissions, correct the access issue, and retry the blocked command."
                    .to_string(),
            ),
            ErrorKind::Unknown => None,
        };
        if let Some(fix) = fix {
            return (Some(fix), error.evidence_event_ids.clone());
        }
    }

    let Some(event) = events.iter().rev().find(|event| {
        event.actor == RawActor::Assistant && contains_fix_language(&event.raw_content)
    }) else {
        return failed_action_fallback_fix(actions);
    };
    (
        Some(first_meaningful_line(&event.raw_content)),
        vec![event.event_id.clone()],
    )
}

fn failed_action_fallback_fix(actions: &[ActionEvent]) -> (Option<String>, Vec<String>) {
    let Some(action) = actions
        .iter()
        .find(|action| action.status == ActionStatus::Failed)
    else {
        return (None, Vec::new());
    };
    let fix = match action.kind {
        ActionKind::RunTests => {
            "Inspect the failing test output, fix the blocking issue, and rerun the exact test command before continuing."
        }
        ActionKind::GitPush => {
            "Inspect the captured push failure output, correct the remote or branch issue, and retry the push with fresh evidence."
        }
        ActionKind::GitMerge | ActionKind::GitRebase => {
            "Inspect the failed merge or rebase output, resolve the blocking issue, rerun relevant tests, and retry with fresh evidence."
        }
        _ => {
            "Inspect the failed command output, fix the blocking issue, and retry the workflow with fresh evidence."
        }
    };
    (Some(fix.to_string()), action.evidence_event_ids.clone())
}

fn extract_intent_notes(events: &[RawEvent], intents: &[IntentEvent]) -> Vec<StructuredNote> {
    let user_events = events
        .iter()
        .filter(|event| event.actor == RawActor::User)
        .collect::<Vec<_>>();
    let mut notes = Vec::new();
    if user_events
        .iter()
        .any(|event| contains_ambiguity_language(&event.raw_content))
    {
        notes.push(StructuredNote {
            summary: "User wording included ambiguity markers; workflow intent may require confirmation before mutation.".to_string(),
            evidence_event_ids: user_events
                .iter()
                .filter(|event| contains_ambiguity_language(&event.raw_content))
                .map(|event| event.event_id.clone())
                .collect(),
            confidence: 0.78,
        });
    }

    let has_investigation = intents
        .iter()
        .any(|intent| intent.kind == IntentKind::Investigate);
    let has_mutation = intents.iter().any(|intent| {
        matches!(
            intent.kind,
            IntentKind::CommitChanges
                | IntentKind::PushBranch
                | IntentKind::MergeBranch
                | IntentKind::EditFiles
        )
    });
    if has_investigation && has_mutation {
        notes.push(StructuredNote {
            summary: "The request mixed investigation and repository mutation goals; execution order should stay explicit.".to_string(),
            evidence_event_ids: user_events
                .iter()
                .map(|event| event.event_id.clone())
                .take(2)
                .collect(),
            confidence: 0.74,
        });
    }
    notes
}

fn extract_assistant_claims(events: &[RawEvent]) -> Vec<ClaimEvent> {
    let mut claims = Vec::new();
    for event in events
        .iter()
        .filter(|event| event.actor == RawActor::Assistant)
    {
        let lower = normalized_text(event);
        let mut kinds = Vec::new();
        if lower.contains("done") || lower.contains("completed") {
            kinds.push(ClaimKind::TaskCompleted);
        }
        if lower.contains("blocked") || lower.contains("cannot") || lower.contains("can't") {
            kinds.push(ClaimKind::TaskBlocked);
        }
        if lower.contains("push succeeded") || lower.contains("pushed successfully") {
            kinds.push(ClaimKind::PushSucceeded);
        }
        if lower.contains("push failed") || lower.contains("non-fast-forward") {
            kinds.push(ClaimKind::PushFailed);
        }
        if lower.contains("commit created") || lower.contains("committed") {
            kinds.push(ClaimKind::CommitCreated);
        }
        if lower.contains("tests passed") || lower.contains("all tests passed") {
            kinds.push(ClaimKind::TestsPassed);
        }
        if lower.contains("tests failed") || lower.contains("failing tests") {
            kinds.push(ClaimKind::TestsFailed);
        }
        if lower.contains("need more information")
            || lower.contains("need confirmation")
            || lower.contains("need more context")
        {
            kinds.push(ClaimKind::NeedsMoreInformation);
        }

        if kinds.is_empty() {
            continue;
        }

        for kind in kinds {
            claims.push(ClaimEvent {
                claim_id: format!("claim-{}", claims.len() + 1),
                kind,
                summary: first_meaningful_line(&event.raw_content),
                evidence_event_ids: vec![event.event_id.clone()],
                confidence: 0.62,
            });
        }
    }
    claims
}

fn claim_contradictions(
    assistant_claims: &[ClaimEvent],
    actions: &[ActionEvent],
    errors: &[ErrorEvent],
) -> Vec<String> {
    let mut contradictions = Vec::new();
    let push_failed_evidence = errors.iter().any(|error| {
        matches!(
            error.kind,
            ErrorKind::NonFastForward
                | ErrorKind::AuthFailed
                | ErrorKind::ProtectedBranch
                | ErrorKind::PreReceiveHookDeclined
                | ErrorKind::RemoteNotFound
                | ErrorKind::NoUpstreamBranch
        )
    }) || actions
        .iter()
        .any(|action| action.kind == ActionKind::GitPush && action.status == ActionStatus::Failed);
    let push_succeeded_evidence = actions.iter().any(|action| {
        action.kind == ActionKind::GitPush
            && action.status == ActionStatus::Succeeded
            && action.evidence_tier >= 5
    });
    let tests_failed_evidence = errors
        .iter()
        .any(|error| error.kind == ErrorKind::TestFailure)
        || actions.iter().any(|action| {
            action.kind == ActionKind::RunTests && action.status == ActionStatus::Failed
        });
    let tests_passed_evidence = actions.iter().any(|action| {
        action.kind == ActionKind::RunTests
            && action.status == ActionStatus::Succeeded
            && action.evidence_tier >= 5
    });

    for claim in assistant_claims {
        match claim.kind {
            ClaimKind::TaskCompleted | ClaimKind::PushSucceeded if push_failed_evidence => {
                push_unique(
                    &mut contradictions,
                    "Assistant completion or push-success claims conflicted with captured push failure evidence.",
                );
            }
            ClaimKind::TestsPassed if tests_failed_evidence => {
                push_unique(
                    &mut contradictions,
                    "Assistant test-success claims conflicted with captured failing test evidence.",
                );
            }
            ClaimKind::TaskBlocked | ClaimKind::NeedsMoreInformation if push_succeeded_evidence => {
                push_unique(
                    &mut contradictions,
                    "Assistant blockage or uncertainty claims conflicted with captured successful push evidence.",
                );
            }
            ClaimKind::TestsFailed if tests_passed_evidence => {
                push_unique(
                    &mut contradictions,
                    "Assistant test-failure claims conflicted with captured successful test evidence.",
                );
            }
            _ => {}
        }
    }

    contradictions
}

fn normalized_text(event: &RawEvent) -> String {
    [
        event.raw_content.as_str(),
        event.command.as_deref().unwrap_or_default(),
        event.stdout.as_deref().unwrap_or_default(),
        event.stderr.as_deref().unwrap_or_default(),
    ]
    .join("\n")
    .to_ascii_lowercase()
}

fn contains_ambiguity_language(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    ["maybe", "if needed", "if necessary", "or ", "depending on"]
        .iter()
        .any(|needle| lower.contains(needle))
}

fn contains_fix_language(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    ["should", "need to", "next step", "fix by", "retry", "rerun"]
        .iter()
        .any(|needle| lower.contains(needle))
}

fn contains_root_cause_language(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "because",
        "due to",
        "blocked by",
        "failed because",
        "missing ",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn first_meaningful_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("unknown")
        .to_string()
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if values.iter().all(|existing| existing != value) {
        values.push(value.to_string());
    }
}
