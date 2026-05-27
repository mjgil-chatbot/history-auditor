use chrono::Utc;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use super::service::HistoryAuditorService;
use super::session::{InspectedSession, SessionMessage, SessionSnapshot, SessionSource};

static TEST_ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub(crate) struct HistoryAuditorTestGuard {
    _lock: MutexGuard<'static, ()>,
    _home_dir: tempfile::TempDir,
    previous_home: Option<OsString>,
}

impl HistoryAuditorTestGuard {
    pub(crate) fn new() -> Self {
        let lock = TEST_ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let home_dir = tempfile::TempDir::new().unwrap();
        let previous_home = std::env::var_os("HOME");
        std::env::set_var("HOME", home_dir.path());
        Self {
            _lock: lock,
            _home_dir: home_dir,
            previous_home,
        }
    }
}

impl Drop for HistoryAuditorTestGuard {
    fn drop(&mut self) {
        match self.previous_home.as_ref() {
            Some(home) => std::env::set_var("HOME", home),
            None => std::env::remove_var("HOME"),
        }
    }
}

#[derive(Clone, Debug)]
struct StaticSessionSource {
    sessions: HashMap<String, InspectedSession>,
}

impl StaticSessionSource {
    fn new(sessions: Vec<InspectedSession>) -> Self {
        Self {
            sessions: sessions
                .into_iter()
                .map(|session| (session.session_id.clone(), session))
                .collect(),
        }
    }
}

impl SessionSource for StaticSessionSource {
    fn inspect_session(&self, session_id: &str) -> anyhow::Result<InspectedSession> {
        self.sessions
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("No fixture session found for `{session_id}`"))
    }
}

pub(crate) fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("history-auditor-{label}-{nanos}"))
}

pub(crate) fn run_git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new(args[0])
        .args(&args[1..])
        .current_dir(repo)
        .output()
        .unwrap_or_else(|error| panic!("failed to run `{}`: {error}", args.join(" ")));
    if !output.status.success() {
        panic!(
            "`{}` failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub(crate) fn init_git_repo_with_branch(workspace: &Path, branch: &str) {
    fs::create_dir_all(workspace.join("src")).unwrap();
    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"history-auditor-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        workspace.join("src/lib.rs"),
        "pub fn fixture() -> u32 { 1 }\n",
    )
    .unwrap();

    run_git(workspace, &["git", "init"]);
    run_git(
        workspace,
        &["git", "config", "user.name", "History Auditor Tests"],
    );
    run_git(
        workspace,
        &[
            "git",
            "config",
            "user.email",
            "history-auditor-tests@example.com",
        ],
    );
    run_git(workspace, &["git", "add", "."]);
    run_git(
        workspace,
        &["git", "commit", "-m", "Initial fixture commit"],
    );
    run_git(workspace, &["git", "checkout", "-B", branch]);
}

fn write_session(data_dir: &Path, workspace: &Path, session_id: &str) -> InspectedSession {
    let now = Utc::now();
    let session = SessionSnapshot {
        id: session_id.to_string(),
        name: "History Audit Session".to_string(),
        launched_from: Some(workspace.display().to_string()),
        messages: vec![
            SessionMessage {
                content: "Commit the current changes and push the branch.".to_string(),
                is_user: true,
                timestamp: now,
            },
            SessionMessage {
                content: "$ git commit -m \"Add history auditor\"\n[exit_code=0]\n[stdout]\n[main abc1234] Add history auditor"
                    .to_string(),
                is_user: false,
                timestamp: now,
            },
            SessionMessage {
                content: "$ git push origin main\n[exit_code=1]\n[stderr]\n! [rejected] main -> main (non-fast-forward)"
                    .to_string(),
                is_user: false,
                timestamp: now,
            },
        ],
    };

    let session_path = data_dir.join("sessions").join(format!("{session_id}.json"));
    fs::create_dir_all(session_path.parent().unwrap()).unwrap();
    fs::write(&session_path, serde_json::to_vec_pretty(&session).unwrap()).unwrap();

    InspectedSession {
        provider: "codex".to_string(),
        provider_display_name: "Codex".to_string(),
        session_id: session_id.to_string(),
        session_path: session_path.display().to_string(),
        session,
    }
}

pub(crate) fn service_with_session_store(
    root: &Path,
    workspace: &Path,
    session_id: &str,
) -> HistoryAuditorService {
    let inspected = write_session(&root.join("codex"), workspace, session_id);
    HistoryAuditorService::new(Arc::new(StaticSessionSource::new(vec![inspected])))
}

pub(crate) fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .to_string()
}
