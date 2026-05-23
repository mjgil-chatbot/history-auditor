use super::extract_structured::extract_structured_context;
use super::extract_support::{
    append_expected_action_gaps, confidence_from_evidence_tier, final_task_verdict,
};
use super::schema::{
    ActionEvent, ActionKind, ActionStatus, CommitVerdict, ErrorEvent, ErrorKind, IntentEvent,
    IntentKind, MergeVerdict, PushVerdict, RawActor, RawEvent, RawEventKind, RepoProbe, TaskRun,
};

#[derive(Clone, Debug)]
pub struct AuditExtraction {
    pub intents: Vec<IntentEvent>,
    pub actions: Vec<ActionEvent>,
    pub errors: Vec<ErrorEvent>,
    pub task_run: TaskRun,
}

pub fn extract_audit(
    session_id: &str,
    events: &[RawEvent],
    repo_probe: Option<&RepoProbe>,
) -> AuditExtraction {
    let intents = extract_intents(events);
    let actions = extract_actions(events);
    let errors = extract_errors(events, &actions);
    let structured = extract_structured_context(events, &intents, &actions, &errors);
    let task_run = build_task_run(
        session_id,
        events,
        &intents,
        &structured,
        &actions,
        &errors,
        repo_probe,
    );
    AuditExtraction {
        intents,
        actions,
        errors,
        task_run,
    }
}

fn extract_intents(events: &[RawEvent]) -> Vec<IntentEvent> {
    let mut intents = Vec::new();
    for event in events.iter().filter(|event| event.actor == RawActor::User) {
        let text = normalized_text(event);
        for (kind, needle) in [
            (IntentKind::CommitChanges, "commit"),
            (IntentKind::PushBranch, "push"),
            (IntentKind::MergeBranch, "merge"),
            (IntentKind::FixError, "fix"),
            (IntentKind::RunTests, "test"),
            (IntentKind::EditFiles, "edit"),
            (IntentKind::Investigate, "investigate"),
        ] {
            if text.contains(needle)
                && !intents
                    .iter()
                    .any(|intent: &IntentEvent| intent.kind == kind)
            {
                intents.push(IntentEvent {
                    intent_id: format!("intent-{}", intents.len() + 1),
                    kind,
                    evidence_event_ids: vec![event.event_id.clone()],
                    confidence: 0.75,
                });
            }
        }
    }
    intents
}

fn extract_actions(events: &[RawEvent]) -> Vec<ActionEvent> {
    let mut actions = Vec::new();
    for event in events {
        if let Some(kind) = detect_action_kind(event) {
            let status = match event.kind {
                RawEventKind::ShellCommand => ActionStatus::Attempted,
                RawEventKind::ShellResult => match event.exit_code {
                    Some(0) => ActionStatus::Succeeded,
                    Some(_) => ActionStatus::Failed,
                    None => ActionStatus::Unknown,
                },
                _ if has_explicit_success(event) => ActionStatus::Succeeded,
                _ if has_explicit_failure(event) => ActionStatus::Failed,
                _ => ActionStatus::Attempted,
            };
            actions.push(ActionEvent {
                action_id: format!("action-{}", actions.len() + 1),
                kind,
                status,
                command: event
                    .command
                    .clone()
                    .or_else(|| shellish_command(&event.raw_content)),
                exit_code: event.exit_code,
                stdout_excerpt: excerpt(event.stdout.as_deref().unwrap_or(&event.raw_content)),
                stderr_excerpt: excerpt(event.stderr.as_deref().unwrap_or("")),
                commit_hash: extract_commit_hash(event),
                evidence_event_ids: vec![event.event_id.clone()],
                evidence_tier: evidence_tier(event),
            });
        }
    }
    actions
}

fn extract_errors(events: &[RawEvent], actions: &[ActionEvent]) -> Vec<ErrorEvent> {
    let mut errors = Vec::new();
    for event in events {
        let haystack = event_text_for_matching(event);
        let Some((kind, message)) = detect_error(&haystack) else {
            continue;
        };
        if errors
            .iter()
            .any(|existing: &ErrorEvent| existing.kind == kind)
        {
            continue;
        }
        errors.push(ErrorEvent {
            error_id: format!("error-{}", errors.len() + 1),
            kind,
            message,
            evidence_event_ids: vec![event.event_id.clone()],
            evidence_tier: evidence_tier(event),
        });
    }
    if errors.is_empty()
        && actions.iter().any(|action| {
            action.kind == ActionKind::RunTests && action.status == ActionStatus::Failed
        })
    {
        errors.push(ErrorEvent {
            error_id: "error-1".to_string(),
            kind: ErrorKind::TestFailure,
            message: "Tests failed according to captured tool output.".to_string(),
            evidence_event_ids: actions
                .iter()
                .filter(|action| action.kind == ActionKind::RunTests)
                .flat_map(|action| action.evidence_event_ids.clone())
                .collect(),
            evidence_tier: 5,
        });
    }
    errors
}

fn build_task_run(
    session_id: &str,
    events: &[RawEvent],
    intents: &[IntentEvent],
    structured: &super::extract_structured::StructuredExtraction,
    actions: &[ActionEvent],
    errors: &[ErrorEvent],
    repo_probe: Option<&RepoProbe>,
) -> TaskRun {
    let (requested_goal, requested_goal_evidence_event_ids) = requested_goal(events);
    let expected_actions = intents
        .iter()
        .map(|intent| format!("{:?}", intent.kind).to_ascii_lowercase())
        .collect();
    let observed_action_ids = actions
        .iter()
        .map(|action| action.action_id.clone())
        .collect();
    let error_ids = errors.iter().map(|error| error.error_id.clone()).collect();
    let commit_verdict = commit_verdict(actions, repo_probe);
    let push_verdict = push_verdict(actions, errors);
    let merge_verdict = merge_verdict(actions, errors, repo_probe);
    let mut missing_evidence = missing_evidence(
        actions,
        &structured.assistant_claims,
        repo_probe,
        &commit_verdict,
        &merge_verdict,
    );
    missing_evidence.extend(structured.contradictions.clone());
    append_expected_action_gaps(intents, actions, &mut missing_evidence);
    let final_verdict = final_task_verdict(
        &push_verdict,
        &merge_verdict,
        errors,
        structured.recommended_fix.is_some(),
        &missing_evidence,
        actions,
        !structured.contradictions.is_empty(),
    );
    let evidence_tier = actions
        .iter()
        .map(|action| action.evidence_tier)
        .max()
        .or_else(|| errors.iter().map(|error| error.evidence_tier).max())
        .unwrap_or(2);
    let confidence = confidence_from_evidence_tier(evidence_tier);

    TaskRun {
        task_id: session_id.to_string(),
        session_id: session_id.to_string(),
        requested_goal,
        requested_goal_evidence_event_ids,
        expected_actions,
        observed_action_ids,
        error_ids,
        commit_verdict,
        push_verdict,
        merge_verdict,
        final_verdict,
        intent_notes: structured.intent_notes.clone(),
        assistant_claims: structured.assistant_claims.clone(),
        root_cause: structured.root_cause.clone(),
        root_cause_evidence_event_ids: structured.root_cause_evidence_event_ids.clone(),
        recommended_fix: structured.recommended_fix.clone(),
        recommended_fix_evidence_event_ids: structured.recommended_fix_evidence_event_ids.clone(),
        missing_evidence,
        confidence,
        evidence_tier,
    }
}

fn commit_verdict(actions: &[ActionEvent], repo_probe: Option<&RepoProbe>) -> CommitVerdict {
    let local_commits = repo_probe.map(|probe| &probe.recent_commits);
    if actions.iter().any(|action| {
        action.kind == ActionKind::GitCommit
            && action.status == ActionStatus::Succeeded
            && action
                .commit_hash
                .as_ref()
                .zip(local_commits)
                .is_some_and(|(hash, commits)| {
                    commits.iter().any(|commit| commit.starts_with(hash))
                })
    }) {
        return CommitVerdict::CommitExistsLocally;
    }
    if actions.iter().any(|action| {
        action.kind == ActionKind::GitCommit && action.status == ActionStatus::Succeeded
    }) {
        return CommitVerdict::CommitCreatedFromLog;
    }
    if actions.iter().any(|action| {
        action.kind == ActionKind::GitCommit
            && action
                .commit_hash
                .as_ref()
                .zip(local_commits)
                .is_some_and(|(hash, commits)| {
                    commits.iter().any(|commit| commit.starts_with(hash))
                })
    }) {
        return CommitVerdict::CommitExistsLocally;
    }
    CommitVerdict::CommitNotAttempted
}

fn push_verdict(actions: &[ActionEvent], errors: &[ErrorEvent]) -> PushVerdict {
    let push_actions = actions
        .iter()
        .filter(|action| action.kind == ActionKind::GitPush)
        .collect::<Vec<_>>();
    if push_actions.is_empty() {
        return PushVerdict::PushNotAttempted;
    }
    if errors.iter().any(|error| {
        matches!(
            error.kind,
            ErrorKind::NonFastForward
                | ErrorKind::AuthFailed
                | ErrorKind::ProtectedBranch
                | ErrorKind::PreReceiveHookDeclined
                | ErrorKind::RemoteNotFound
                | ErrorKind::NoUpstreamBranch
        )
    }) || push_actions
        .iter()
        .any(|action| action.status == ActionStatus::Failed)
    {
        return PushVerdict::PushFailed;
    }
    if push_actions
        .iter()
        .any(|action| decisive_push_success(action))
    {
        return PushVerdict::PushSucceededFromToolOutput;
    }
    PushVerdict::RemotePushUnknown
}

fn merge_verdict(
    actions: &[ActionEvent],
    errors: &[ErrorEvent],
    repo_probe: Option<&RepoProbe>,
) -> MergeVerdict {
    if errors.iter().any(|error| {
        matches!(
            error.kind,
            ErrorKind::MergeConflict | ErrorKind::RebaseConflict
        )
    }) {
        return MergeVerdict::MergeConflict;
    }
    let merge_actions = actions
        .iter()
        .filter(|action| matches!(action.kind, ActionKind::GitMerge | ActionKind::GitRebase));
    if merge_actions.clone().next().is_none() {
        return MergeVerdict::MergeNotAttempted;
    }
    if merge_actions
        .clone()
        .any(|action| action.status == ActionStatus::Succeeded && action.evidence_tier >= 5)
    {
        return MergeVerdict::LocalMergeCompleted;
    }
    if merge_actions
        .clone()
        .any(|action| action.kind == ActionKind::GitMerge)
        && repo_probe
            .and_then(|probe| probe.head_parent_count)
            .is_some_and(|count| count >= 2)
    {
        return MergeVerdict::LocalMergeCompleted;
    }
    MergeVerdict::RemoteMergeUnknown
}

fn requested_goal(events: &[RawEvent]) -> (String, Vec<String>) {
    let user_events = events
        .iter()
        .filter(|event| event.actor == RawActor::User)
        .collect::<Vec<_>>();
    let requested_goal = user_events
        .iter()
        .find_map(|event| {
            let content = event.raw_content.trim();
            (!content.is_empty()).then(|| content.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());
    let evidence = user_events
        .iter()
        .filter(|event| !event.raw_content.trim().is_empty())
        .take(2)
        .map(|event| event.event_id.clone())
        .collect();
    (requested_goal, evidence)
}

fn missing_evidence(
    actions: &[ActionEvent],
    assistant_claims: &[super::schema::ClaimEvent],
    repo_probe: Option<&RepoProbe>,
    commit_verdict: &CommitVerdict,
    merge_verdict: &MergeVerdict,
) -> Vec<String> {
    let mut missing = Vec::new();
    if actions
        .iter()
        .any(|action| action.kind == ActionKind::GitPush)
        && actions
            .iter()
            .filter(|action| action.kind == ActionKind::GitPush)
            .all(|action| action.evidence_tier < 5)
    {
        missing.push("Missing captured git push exit-code evidence.".to_string());
    }
    if actions
        .iter()
        .any(|action| action.kind == ActionKind::GitPush)
        && actions
            .iter()
            .filter(|action| action.kind == ActionKind::GitPush)
            .any(|action| action.status == ActionStatus::Succeeded && action.evidence_tier >= 5)
        && actions
            .iter()
            .filter(|action| action.kind == ActionKind::GitPush)
            .all(|action| !decisive_push_success(action))
    {
        missing.push(
            "Captured git push activity did not include decisive remote update evidence."
                .to_string(),
        );
    }
    if repo_probe.is_none() && actions.iter().any(|action| is_git_action(&action.kind)) {
        missing
            .push("No local repo probe was available for after-the-fact validation.".to_string());
    }
    if !actions.is_empty() && actions.iter().all(|action| action.evidence_tier <= 2) {
        missing.push(
            "Only natural-language or transcript-level claims were captured for the observed actions."
                .to_string(),
        );
    }
    if !assistant_claims.is_empty() && actions.is_empty() {
        missing.push("Assistant claims were present without captured action evidence.".to_string());
    }
    if actions
        .iter()
        .any(|action| action.status == ActionStatus::Failed)
        && !actions
            .iter()
            .any(|action| action.status == ActionStatus::Failed && action.evidence_tier >= 5)
    {
        missing.push(
            "Failed actions were captured without high-tier tool-output evidence.".to_string(),
        );
    }
    let merge_actions = actions
        .iter()
        .filter(|action| matches!(action.kind, ActionKind::GitMerge | ActionKind::GitRebase))
        .collect::<Vec<_>>();
    if !merge_actions.is_empty()
        && !merge_actions
            .iter()
            .any(|action| action.status == ActionStatus::Succeeded && action.evidence_tier >= 5)
        && !merge_actions
            .iter()
            .any(|action| action.status == ActionStatus::Failed && action.evidence_tier >= 5)
        && *merge_verdict != MergeVerdict::LocalMergeCompleted
    {
        missing.push(
            "Captured merge or rebase activity did not include decisive success or conflict evidence."
                .to_string(),
        );
    }
    if *commit_verdict == CommitVerdict::CommitCreatedFromLog
        && actions.iter().any(|action| {
            action.kind == ActionKind::GitCommit && action.status == ActionStatus::Succeeded
        })
        && repo_probe.is_some()
    {
        missing.push(
            "Captured git commit output was not confirmed by the local repo probe.".to_string(),
        );
    }
    missing
}

fn detect_action_kind(event: &RawEvent) -> Option<ActionKind> {
    let text = normalized_text(event);
    let command = event
        .command
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let source = if command.is_empty() { text } else { command };
    if source.contains("git status") {
        Some(ActionKind::GitStatus)
    } else if source.contains("git add") {
        Some(ActionKind::GitAdd)
    } else if source.contains("git commit") {
        Some(ActionKind::GitCommit)
    } else if source.contains("git push") {
        Some(ActionKind::GitPush)
    } else if source.contains("git pull") {
        Some(ActionKind::GitPull)
    } else if source.contains("git fetch") {
        Some(ActionKind::GitFetch)
    } else if source.contains("git merge") {
        Some(ActionKind::GitMerge)
    } else if source.contains("git rebase") {
        Some(ActionKind::GitRebase)
    } else if source.contains("git checkout") {
        Some(ActionKind::GitCheckout)
    } else if source.contains("git log") {
        Some(ActionKind::GitLog)
    } else if source.contains("cargo test")
        || source.contains("pytest")
        || source.contains("npm test")
        || source.contains("pnpm test")
    {
        Some(ActionKind::RunTests)
    } else if source.contains("apply_patch") || source.contains("edit_file") {
        Some(ActionKind::EditFile)
    } else {
        None
    }
}

fn is_git_action(kind: &ActionKind) -> bool {
    matches!(
        kind,
        ActionKind::GitStatus
            | ActionKind::GitAdd
            | ActionKind::GitCommit
            | ActionKind::GitPush
            | ActionKind::GitPull
            | ActionKind::GitFetch
            | ActionKind::GitMerge
            | ActionKind::GitRebase
            | ActionKind::GitCheckout
            | ActionKind::GitLog
    )
}

fn decisive_push_success(action: &ActionEvent) -> bool {
    if action.kind != ActionKind::GitPush
        || action.status != ActionStatus::Succeeded
        || action.evidence_tier < 5
    {
        return false;
    }
    let output = format!(
        "{}\n{}",
        action.stdout_excerpt.as_deref().unwrap_or_default(),
        action.stderr_excerpt.as_deref().unwrap_or_default()
    )
    .to_ascii_lowercase();
    output.contains("->")
        || output.contains("[new branch]")
        || output.contains("[new tag]")
        || output.contains("everything up-to-date")
        || output.contains("everything up to date")
}

fn detect_error(text: &str) -> Option<(ErrorKind, String)> {
    for (kind, needle, message) in [
        (
            ErrorKind::NonFastForward,
            "non-fast-forward",
            "Push failed with non-fast-forward.",
        ),
        (
            ErrorKind::AuthFailed,
            "authentication failed",
            "Remote authentication failed.",
        ),
        (
            ErrorKind::ProtectedBranch,
            "protected branch",
            "Push failed because the branch is protected.",
        ),
        (
            ErrorKind::PreReceiveHookDeclined,
            "pre-receive hook declined",
            "Remote pre-receive hook declined the push.",
        ),
        (
            ErrorKind::RemoteNotFound,
            "repository not found",
            "Remote repository was not found.",
        ),
        (
            ErrorKind::NoUpstreamBranch,
            "no upstream branch",
            "Push failed because the branch has no upstream.",
        ),
        (
            ErrorKind::MergeConflict,
            "merge conflict",
            "Local merge failed with conflicts.",
        ),
        (
            ErrorKind::RebaseConflict,
            "rebase conflict",
            "Rebase failed with conflicts.",
        ),
        (
            ErrorKind::DetachedHead,
            "detached head",
            "Command ran from detached HEAD state.",
        ),
        (
            ErrorKind::NothingToCommit,
            "nothing to commit",
            "Commit attempt reported nothing to commit.",
        ),
        (
            ErrorKind::DirtyWorktree,
            "dirty worktree",
            "Worktree remained dirty.",
        ),
        (ErrorKind::TestFailure, "test failed", "Tests failed."),
        (
            ErrorKind::DependencyFailure,
            "dependency",
            "Dependency failure was reported.",
        ),
        (
            ErrorKind::PermissionDenied,
            "permission denied",
            "Permission denied was reported.",
        ),
    ] {
        if text.contains(needle) {
            return Some((kind, message.to_string()));
        }
    }
    None
}

fn extract_commit_hash(event: &RawEvent) -> Option<String> {
    let candidates = [
        event.stdout.as_deref().unwrap_or_default(),
        event.stderr.as_deref().unwrap_or_default(),
        event.raw_content.as_str(),
    ];
    for candidate in candidates {
        for token in
            candidate.split(|ch: char| ch.is_whitespace() || ['[', ']', '(', ')'].contains(&ch))
        {
            let trimmed = token.trim_matches(|ch: char| ch == ':' || ch == ',' || ch == ';');
            if (7..=40).contains(&trimmed.len()) && trimmed.chars().all(|ch| ch.is_ascii_hexdigit())
            {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn evidence_tier(event: &RawEvent) -> u8 {
    if matches!(event.kind, RawEventKind::ShellResult) && event.exit_code.is_some() {
        5
    } else if matches!(event.kind, RawEventKind::ShellCommand) {
        5
    } else if event.actor == RawActor::Assistant {
        2
    } else {
        1
    }
}

fn shellish_command(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("$ ").map(str::to_string))
}

fn normalized_text(event: &RawEvent) -> String {
    event_text_for_matching(event)
}

fn event_text_for_matching(event: &RawEvent) -> String {
    [
        event.raw_content.as_str(),
        event.command.as_deref().unwrap_or_default(),
        event.stdout.as_deref().unwrap_or_default(),
        event.stderr.as_deref().unwrap_or_default(),
    ]
    .join("\n")
    .to_ascii_lowercase()
}

fn has_explicit_success(event: &RawEvent) -> bool {
    event_text_for_matching(event).contains("exit_code=0")
}

fn has_explicit_failure(event: &RawEvent) -> bool {
    event.exit_code.is_some_and(|code| code != 0)
        || event_text_for_matching(event).contains("failed")
}

fn excerpt(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.lines().take(4).collect::<Vec<_>>().join("\n"))
    }
}
