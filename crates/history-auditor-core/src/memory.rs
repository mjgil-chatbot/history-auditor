use anyhow::Result;
use chrono::Utc;
use semhash_rs::utils::Value;
use semhash_rs::{Backend, CandidateLimit, Record, SemHash, SemHashOptions, Strategy};
use std::collections::BTreeMap;
use std::collections::HashSet;

use super::incident::{incident_pattern_for_audit, incident_pattern_for_summary};
use super::schema::{
    AuditRecord, ClaimKind, CommitVerdict, ErrorKind, MemoryKind, MemoryRecord, MemoryStatus,
    RawActor, StoredAuditSummary, TaskVerdict,
};

pub fn compile_memories(
    audit: &AuditRecord,
    existing_texts: &[String],
    prior_audits: &[StoredAuditSummary],
) -> Result<Vec<MemoryRecord>> {
    let mut candidates = Vec::new();
    let now = Utc::now();

    if audit.task_run.confidence >= 0.60 {
        let summary = episodic_summary(audit);
        candidates.push(new_memory(
            audit,
            MemoryKind::Episodic,
            summary,
            audit.task_run.confidence,
            episodic_evidence_event_ids(audit),
            Vec::new(),
            now,
        ));
    }

    if audit.task_run.confidence >= 0.75 {
        if let Some((playbook, evidence_event_ids, supporting_task_ids, confidence)) =
            procedural_memory(audit, prior_audits)
        {
            candidates.push(new_memory(
                audit,
                MemoryKind::Procedural,
                playbook,
                confidence,
                evidence_event_ids,
                supporting_task_ids,
                now,
            ));
        }
    }

    if anti_pattern_candidate(audit).is_some() {
        let (text, evidence_event_ids) = anti_pattern_candidate(audit).unwrap();
        candidates.push(new_memory(
            audit,
            MemoryKind::AntiPattern,
            text,
            0.84,
            evidence_event_ids,
            Vec::new(),
            now,
        ));
    }

    if let Some((text, evidence_event_ids, supporting_task_ids, confidence)) =
        repeated_fact_candidate(audit, prior_audits, ErrorKind::ProtectedBranch)
    {
        candidates.push(new_memory(
            audit,
            MemoryKind::SemanticFact,
            text,
            confidence,
            evidence_event_ids,
            supporting_task_ids,
            now,
        ));
    }

    if let Some((text, evidence_event_ids, supporting_task_ids, confidence)) =
        repeated_fact_candidate(audit, prior_audits, ErrorKind::NoUpstreamBranch)
    {
        candidates.push(new_memory(
            audit,
            MemoryKind::RepoFact,
            text,
            confidence,
            evidence_event_ids,
            supporting_task_ids,
            now,
        ));
    }

    if let Some((text, evidence_event_ids, supporting_task_ids, confidence)) =
        repeated_fact_candidate(audit, prior_audits, ErrorKind::PreReceiveHookDeclined)
    {
        candidates.push(new_memory(
            audit,
            MemoryKind::RepoFact,
            text,
            confidence,
            evidence_event_ids,
            supporting_task_ids,
            now,
        ));
    }

    if let Some(error) = audit.errors.first() {
        if matches!(error.kind, ErrorKind::ProtectedBranch)
            && !candidates
                .iter()
                .any(|memory| memory.kind == MemoryKind::SemanticFact)
        {
            candidates.push(new_memory(
                audit,
                MemoryKind::RepoFact,
                format!(
                    "Repo {} appears to reject direct pushes to the active branch; prefer the review path or a feature branch.",
                    audit.workspace_root
                ),
                0.86,
                error.evidence_event_ids.clone(),
                Vec::new(),
                now,
            ));
        }
    }

    if let Some((text, evidence_event_ids)) = user_preference_memory(audit) {
        candidates.push(new_memory(
            audit,
            MemoryKind::UserPreference,
            text,
            0.91,
            evidence_event_ids,
            Vec::new(),
            now,
        ));
    }

    dedupe_candidates(candidates, existing_texts)
}

fn dedupe_candidates(
    candidates: Vec<MemoryRecord>,
    existing_texts: &[String],
) -> Result<Vec<MemoryRecord>> {
    let existing = if existing_texts.is_empty() {
        None
    } else {
        Some(SemHash::from_records(
            existing_texts
                .iter()
                .cloned()
                .map(Record::text)
                .collect::<Vec<_>>(),
            SemHashOptions::new().ann_backend(Backend::Exact),
        )?)
    };

    let mut deduped = Vec::new();
    let mut seen_texts = HashSet::new();
    for candidate in candidates {
        if !seen_texts.insert(candidate.text.clone()) {
            continue;
        }
        let duplicate_existing = if candidate.supporting_task_ids.is_empty() {
            existing.as_ref().is_some_and(|semhash| {
                semhash
                    .deduplicate(vec![Record::text(candidate.text.clone())], 0.92)
                    .map(|result| result.selected.is_empty())
                    .unwrap_or(false)
            })
        } else {
            existing_texts
                .iter()
                .any(|existing_text| existing_text == &candidate.text)
        };
        if !duplicate_existing {
            deduped.push(candidate);
        }
    }

    Ok(deduped)
}

fn new_memory(
    audit: &AuditRecord,
    kind: MemoryKind,
    text: String,
    confidence: f32,
    evidence_event_ids: Vec<String>,
    supporting_task_ids: Vec<String>,
    now: chrono::DateTime<Utc>,
) -> MemoryRecord {
    MemoryRecord {
        memory_id: uuid::Uuid::new_v4().to_string(),
        session_id: audit.session_id.clone(),
        task_id: audit.task_run.task_id.clone(),
        kind,
        status: MemoryStatus::Proposed,
        text,
        confidence,
        repo_path: Some(audit.workspace_root.clone()),
        branch: audit
            .repo_probe
            .as_ref()
            .and_then(|probe| probe.branch.clone()),
        evidence_event_ids,
        supporting_task_ids,
        created_at: now,
        updated_at: now,
        status_reason: None,
    }
}

fn procedural_memory(
    audit: &AuditRecord,
    prior_audits: &[StoredAuditSummary],
) -> Option<(String, Vec<String>, Vec<String>, f32)> {
    let error = audit.errors.first()?;
    let base_text = match error.kind {
        ErrorKind::NonFastForward => {
            "When push fails with non-fast-forward: 1. `git fetch origin` 2. inspect divergence 3. rebase or merge safely 4. resolve conflicts 5. rerun tests 6. push again."
                .to_string()
        }
        ErrorKind::AuthFailed => {
            "When remote authentication fails: verify the intended remote, refresh the credential or auth context, confirm access for the target branch, then retry the push with captured output."
                .to_string()
        }
        ErrorKind::ProtectedBranch => {
            "When a protected branch rejects a push: confirm the target branch policy, move the change onto the intended feature or review branch, rerun validation if needed, and continue through the repo's review path instead of pushing directly."
                .to_string()
        }
        ErrorKind::MergeConflict => {
            "When merge conflicts appear: 1. inspect both sides of the conflict 2. resolve the files intentionally 3. rerun the relevant test suite 4. repeat the merge with captured evidence."
                .to_string()
        }
        ErrorKind::RebaseConflict => {
            "When rebase conflicts appear: 1. inspect the conflicting commits 2. resolve the files intentionally 3. rerun the relevant test suite 4. continue the rebase with captured evidence."
                .to_string()
        }
        ErrorKind::RemoteNotFound => {
            "When the remote repository is not found: verify the remote name and URL, correct the destination if needed, confirm access to the target repo, then retry the push with captured output."
                .to_string()
        }
        ErrorKind::NoUpstreamBranch => {
            "When a branch has no upstream: set the upstream explicitly, confirm the destination branch, then retry the push with captured output."
                .to_string()
        }
        ErrorKind::DetachedHead => {
            "When git operations run from detached HEAD: inspect the current commit, check out the intended branch, confirm the destination ref, then retry commit or push steps with captured output."
                .to_string()
        }
        ErrorKind::NothingToCommit => {
            "When git reports nothing to commit: confirm whether the requested edits were already committed or never created, inspect the intended files for a real delta, then retry the commit only after actual changes exist."
                .to_string()
        }
        ErrorKind::DirtyWorktree => {
            "When the worktree is dirty during a delivery task: inspect pending changes, separate intentional edits from leftovers, clean or commit them deliberately, rerun validation, then continue the workflow."
                .to_string()
        }
        ErrorKind::PreReceiveHookDeclined => {
            "When a push is rejected by a pre-receive hook: inspect the remote policy message, satisfy the required checks, rerun validation, then retry the push with captured output."
                .to_string()
        }
        ErrorKind::TestFailure => {
            "When tests fail during a delivery task: stop the push path, fix the failing tests, rerun the exact suite, then continue the git workflow."
                .to_string()
        }
        ErrorKind::DependencyFailure => {
            "When a dependency failure blocks the workflow: identify the missing or incompatible dependency, repair the environment or manifest, rerun the blocked command, and only then continue delivery steps."
                .to_string()
        }
        ErrorKind::PermissionDenied => {
            "When a workflow fails with permission denied: determine whether the blocked step is local filesystem access or remote authz, fix that access issue explicitly, rerun the exact command, then continue."
                .to_string()
        }
        _ => return None,
    };
    let repeated = repeated_incident_support(audit, prior_audits, error.kind.clone());
    let (text, supporting_task_ids, confidence) = match repeated {
        Some(cluster)
            if matches!(
                error.kind,
                ErrorKind::NonFastForward
                    | ErrorKind::AuthFailed
                    | ErrorKind::ProtectedBranch
                    | ErrorKind::MergeConflict
                    | ErrorKind::RebaseConflict
                    | ErrorKind::RemoteNotFound
                    | ErrorKind::NoUpstreamBranch
                    | ErrorKind::DetachedHead
                    | ErrorKind::NothingToCommit
                    | ErrorKind::DirtyWorktree
                    | ErrorKind::PreReceiveHookDeclined
                    | ErrorKind::TestFailure
                    | ErrorKind::DependencyFailure
                    | ErrorKind::PermissionDenied
            ) && cluster.supporting_task_ids.len() >= 2 =>
        {
            let repeated_text = match error.kind {
                ErrorKind::NonFastForward => format!(
                    "Repo {} has repeated non-fast-forward delivery failures. {}",
                    audit.workspace_root, base_text
                ),
                ErrorKind::AuthFailed => format!(
                    "Repo {} has repeated remote authentication failures during delivery work. {}",
                    audit.workspace_root, base_text
                ),
                ErrorKind::ProtectedBranch => format!(
                    "Repo {} has repeated protected-branch delivery rejections. {}",
                    audit.workspace_root, base_text
                ),
                ErrorKind::MergeConflict => format!(
                    "Repo {} has repeated merge-conflict recovery work. {}",
                    audit.workspace_root, base_text
                ),
                ErrorKind::RebaseConflict => format!(
                    "Repo {} has repeated rebase-conflict recovery work. {}",
                    audit.workspace_root, base_text
                ),
                ErrorKind::RemoteNotFound => format!(
                    "Repo {} has repeated remote-destination failures during delivery work. {}",
                    audit.workspace_root, base_text
                ),
                ErrorKind::NoUpstreamBranch => format!(
                    "Repo {} has repeated first-push upstream mistakes. {}",
                    audit.workspace_root, base_text
                ),
                ErrorKind::DetachedHead => format!(
                    "Repo {} has repeated detached-HEAD delivery mistakes. {}",
                    audit.workspace_root, base_text
                ),
                ErrorKind::NothingToCommit => format!(
                    "Repo {} has repeated commit attempts with no actual file delta. {}",
                    audit.workspace_root, base_text
                ),
                ErrorKind::DirtyWorktree => format!(
                    "Repo {} has repeated dirty-worktree delivery failures. {}",
                    audit.workspace_root, base_text
                ),
                ErrorKind::PreReceiveHookDeclined => format!(
                    "Repo {} has repeated pre-receive hook delivery failures. {}",
                    audit.workspace_root, base_text
                ),
                ErrorKind::TestFailure => format!(
                    "Repo {} has repeated delivery-blocking test failures. {}",
                    audit.workspace_root, base_text
                ),
                ErrorKind::DependencyFailure => format!(
                    "Repo {} has repeated dependency-related delivery failures. {}",
                    audit.workspace_root, base_text
                ),
                ErrorKind::PermissionDenied => format!(
                    "Repo {} has repeated permission-related delivery failures. {}",
                    audit.workspace_root, base_text
                ),
                _ => base_text.clone(),
            };
            let text = cluster
                .representative_summary
                .as_ref()
                .map(|summary| format!("{repeated_text} Representative incident: {summary}."))
                .unwrap_or(repeated_text);
            let confidence = if cluster.supporting_task_ids.len() >= 3 {
                0.91
            } else {
                0.89
            };
            (text, cluster.supporting_task_ids, confidence)
        }
        _ => (base_text, Vec::new(), audit.task_run.confidence.max(0.75)),
    };
    Some((
        text,
        error.evidence_event_ids.clone(),
        supporting_task_ids,
        confidence,
    ))
}

fn episodic_summary(audit: &AuditRecord) -> String {
    let mut summary = format!(
        "On {} in repo {}, the agent was asked to {}. Commit verdict: {:?}. Push verdict: {:?}. Final verdict: {:?}.",
        audit.audited_at.date_naive(),
        audit.workspace_root,
        audit.task_run.requested_goal,
        audit.task_run.commit_verdict,
        audit.task_run.push_verdict,
        audit.task_run.final_verdict,
    );
    if let Some(root_cause) = &audit.task_run.root_cause {
        summary.push_str(&format!(" Root cause: {}.", root_cause));
    }
    if let Some(claim) = audit
        .task_run
        .assistant_claims
        .iter()
        .find(|claim| claim.kind == ClaimKind::TaskCompleted)
    {
        summary.push_str(&format!(" Assistant claim recorded: {}.", claim.summary));
    }
    summary
}

fn episodic_evidence_event_ids(audit: &AuditRecord) -> Vec<String> {
    let mut ids = Vec::new();
    ids.extend(audit.task_run.requested_goal_evidence_event_ids.clone());
    ids.extend(audit.task_run.root_cause_evidence_event_ids.clone());
    ids.extend(
        audit
            .task_run
            .assistant_claims
            .iter()
            .flat_map(|claim| claim.evidence_event_ids.clone()),
    );
    dedupe_ids(ids)
}

fn anti_pattern_candidate(audit: &AuditRecord) -> Option<(String, Vec<String>)> {
    if audit.task_run.commit_verdict == CommitVerdict::CommitCreatedFromLog
        && audit.task_run.final_verdict != TaskVerdict::TaskCompleted
    {
        return Some((
            "Do not mark a task complete just because a commit was created; verify push, merge, and test evidence separately."
                .to_string(),
            dedupe_ids(
                audit
                    .errors
                    .iter()
                    .flat_map(|error| error.evidence_event_ids.clone())
                    .collect(),
            ),
        ));
    }

    let claim = audit.task_run.assistant_claims.iter().find(|claim| {
        matches!(
            claim.kind,
            ClaimKind::TaskCompleted | ClaimKind::PushSucceeded
        )
    })?;
    if matches!(
        audit.task_run.final_verdict,
        TaskVerdict::TaskBlockedWithClearFix | TaskVerdict::TaskFailed
    ) {
        return Some((
            "Do not rely on assistant completion claims without matching tool evidence; verify the workflow outcome from captured command results."
                .to_string(),
            dedupe_ids(
                claim.evidence_event_ids
                    .iter()
                    .cloned()
                    .chain(
                        audit.errors
                            .iter()
                            .flat_map(|error| error.evidence_event_ids.clone()),
                    )
                    .collect(),
            ),
        ));
    }
    None
}

fn user_preference_memory(audit: &AuditRecord) -> Option<(String, Vec<String>)> {
    let event = audit.raw_events.iter().find(|event| {
        event.actor == RawActor::User && contains_preference_language(&event.raw_content)
    })?;
    Some((
        format!(
            "User preference for repo {}: {}",
            audit.workspace_root,
            event.raw_content.trim()
        ),
        vec![event.event_id.clone()],
    ))
}

fn repeated_fact_candidate(
    audit: &AuditRecord,
    prior_audits: &[StoredAuditSummary],
    error_kind: ErrorKind,
) -> Option<(String, Vec<String>, Vec<String>, f32)> {
    let current_error = audit.errors.iter().find(|error| error.kind == error_kind)?;
    let cluster = repeated_incident_support(audit, prior_audits, error_kind.clone())?;
    if cluster.supporting_task_ids.len() < 2 {
        return None;
    }
    let base_text = match error_kind {
        ErrorKind::ProtectedBranch => format!(
            "Repo {} repeatedly rejects direct pushes to its protected branch; use a feature branch or the review path instead of pushing directly.",
            audit.workspace_root
        ),
        ErrorKind::NoUpstreamBranch => format!(
            "Branches in repo {} often require an explicit upstream before the first push; set the upstream and confirm the destination branch before retrying.",
            audit.workspace_root
        ),
        ErrorKind::PreReceiveHookDeclined => format!(
            "Repo {} repeatedly rejects pushes through a pre-receive hook; expect server-side policy checks and satisfy them before retrying the push.",
            audit.workspace_root
        ),
        _ => return None,
    };
    let text = cluster
        .representative_summary
        .as_ref()
        .map(|summary| format!("{base_text} Representative incident: {summary}."))
        .unwrap_or(base_text);
    let confidence = if cluster.supporting_task_ids.len() >= 3 {
        0.93
    } else {
        0.90
    };
    Some((
        text,
        current_error.evidence_event_ids.clone(),
        cluster.supporting_task_ids,
        confidence,
    ))
}

fn repeated_incident_support(
    audit: &AuditRecord,
    prior_audits: &[StoredAuditSummary],
    error_kind: ErrorKind,
) -> Option<IncidentCluster> {
    audit.errors.iter().find(|error| error.kind == error_kind)?;
    let incident_pattern = incident_pattern_for_audit(audit)?;
    let mut incidents = prior_audits
        .iter()
        .filter(|summary| {
            incident_pattern_for_summary(summary).as_deref() == Some(incident_pattern.as_str())
        })
        .map(incident_pattern_from_summary)
        .collect::<Vec<_>>();
    incidents.push(current_incident_pattern(audit, &error_kind));
    representative_incident_cluster(&incidents, &audit.session_id)
        .ok()
        .flatten()
}

fn representative_incident_cluster(
    incidents: &[IncidentPattern],
    current_session_id: &str,
) -> Result<Option<IncidentCluster>> {
    if incidents.len() < 2 {
        return Ok(None);
    }
    if incidents.len() == 2
        && incidents
            .iter()
            .any(|incident| incident.task_id == current_session_id)
    {
        return Ok(Some(IncidentCluster {
            representative_summary: incidents.first().map(|incident| incident.summary.clone()),
            supporting_task_ids: incidents
                .iter()
                .filter(|incident| incident.task_id != current_session_id)
                .map(|incident| incident.task_id.clone())
                .collect(),
        }));
    }
    let records = incidents.iter().map(incident_record).collect::<Vec<_>>();
    let semhash = SemHash::from_records(
        records.clone(),
        SemHashOptions::new()
            .columns(["summary"])
            .ann_backend(Backend::Exact),
    )?;
    let filtered = semhash.self_filter_outliers(0.20)?;
    let mut selected = filtered
        .selected
        .iter()
        .filter_map(incident_pattern_from_record)
        .collect::<Vec<_>>();
    let exact_supporting_task_ids = exact_supporting_task_ids(incidents, current_session_id);
    if selected.len() < 2
        || !selected
            .iter()
            .any(|incident| incident.task_id == current_session_id)
    {
        if exact_supporting_task_ids.len() >= 2 {
            return Ok(Some(IncidentCluster {
                representative_summary: incidents.first().map(|incident| incident.summary.clone()),
                supporting_task_ids: exact_supporting_task_ids,
            }));
        }
        return Ok(None);
    }
    let representative = SemHash::from_records(
        selected.iter().map(incident_record).collect(),
        SemHashOptions::new()
            .columns(["summary"])
            .ann_backend(Backend::Exact),
    )?
    .self_find_representative(1, CandidateLimit::Auto, 0.50, Strategy::MMR)?;
    let representative_summary = representative
        .selected
        .first()
        .and_then(incident_pattern_from_record)
        .map(|incident| incident.summary);
    selected.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    let mut supporting_task_ids = selected
        .into_iter()
        .filter(|incident| incident.task_id != current_session_id)
        .map(|incident| incident.task_id)
        .take(3)
        .collect::<Vec<_>>();
    if supporting_task_ids.len() < 2 && exact_supporting_task_ids.len() >= 2 {
        supporting_task_ids = exact_supporting_task_ids;
    }
    Ok(Some(IncidentCluster {
        representative_summary,
        supporting_task_ids,
    }))
}

fn exact_supporting_task_ids(
    incidents: &[IncidentPattern],
    current_session_id: &str,
) -> Vec<String> {
    let mut supporting_task_ids = incidents
        .iter()
        .filter(|incident| incident.task_id != current_session_id)
        .map(|incident| incident.task_id.clone())
        .collect::<Vec<_>>();
    supporting_task_ids.sort();
    supporting_task_ids.truncate(3);
    supporting_task_ids
}

fn current_incident_pattern(audit: &AuditRecord, error_kind: &ErrorKind) -> IncidentPattern {
    let error = audit
        .errors
        .iter()
        .find(|error| &error.kind == error_kind)
        .expect("current incident pattern requires the matching error");
    IncidentPattern {
        task_id: audit.session_id.clone(),
        summary: format!(
            "{} | {:?} | {}",
            audit.task_run.requested_goal, audit.task_run.final_verdict, error.message
        ),
    }
}

fn incident_pattern_from_summary(summary: &StoredAuditSummary) -> IncidentPattern {
    IncidentPattern {
        task_id: summary.session_id.clone(),
        summary: format!(
            "{} | {:?} | {}",
            summary.task_run.requested_goal,
            summary.task_run.final_verdict,
            summary
                .task_run
                .root_cause
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        ),
    }
}

fn incident_record(incident: &IncidentPattern) -> Record {
    let mut record = BTreeMap::new();
    record.insert("task_id".to_string(), Value::from(incident.task_id.clone()));
    record.insert("summary".to_string(), Value::from(incident.summary.clone()));
    Record::dict(record)
}

fn incident_pattern_from_record(record: &Record) -> Option<IncidentPattern> {
    let record = record.as_dict()?;
    Some(IncidentPattern {
        task_id: record.get("task_id")?.as_string_lossy(),
        summary: record.get("summary")?.as_string_lossy(),
    })
}

fn contains_preference_language(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    ["prefer", "do not", "don't", "always", "never"]
        .iter()
        .any(|needle| lower.contains(needle))
}

fn dedupe_ids(ids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for id in ids {
        if seen.insert(id.clone()) {
            ordered.push(id);
        }
    }
    ordered
}

#[derive(Clone, Debug)]
struct IncidentPattern {
    task_id: String,
    summary: String,
}

#[derive(Clone, Debug)]
struct IncidentCluster {
    representative_summary: Option<String>,
    supporting_task_ids: Vec<String>,
}
