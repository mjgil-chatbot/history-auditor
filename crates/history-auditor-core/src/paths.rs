use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub const HISTORY_AUDITOR_DIR_NAME: &str = "history-auditor";
pub const AUDIT_DB_FILE_NAME: &str = "history-auditor.sqlite3";
pub const REPORTS_DIR_NAME: &str = "reports";
pub const MEMORY_CORPUS_DIR_NAME: &str = "memory-corpus";
pub const ARTIFACTS_DIR_NAME: &str = "artifacts";

const CANONICAL_HISTORY_AUDITOR_ROOT_DIR: &str = ".history-auditor";
const LEGACY_HISTORY_AUDITOR_ROOT_DIRS: &[&str] = &[
    ".rust-chatbot/history-auditor",
    ".claude-desktop/history-auditor",
];
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0001_0000_01b3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceAuditorPaths {
    pub workspace_id: String,
    pub workspace_root: PathBuf,
    pub root_dir: PathBuf,
    pub db_path: PathBuf,
    pub reports_dir: PathBuf,
    pub memory_corpus_dir: PathBuf,
    pub artifacts_dir: PathBuf,
}

pub fn history_auditor_root_dir() -> PathBuf {
    resolve_history_auditor_root_dir(&dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")))
}

pub fn workspace_auditor_paths(workspace_path: impl AsRef<Path>) -> Result<WorkspaceAuditorPaths> {
    let workspace_root = std::fs::canonicalize(workspace_path.as_ref()).with_context(|| {
        format!(
            "failed to canonicalize workspace path {}",
            workspace_path.as_ref().display()
        )
    })?;
    let workspace_id = workspace_id_for_path(&workspace_root);
    let root_dir = history_auditor_root_dir().join(&workspace_id);
    Ok(WorkspaceAuditorPaths {
        workspace_id,
        workspace_root,
        db_path: root_dir.join(AUDIT_DB_FILE_NAME),
        reports_dir: root_dir.join(REPORTS_DIR_NAME),
        memory_corpus_dir: root_dir.join(MEMORY_CORPUS_DIR_NAME),
        artifacts_dir: root_dir.join(ARTIFACTS_DIR_NAME),
        root_dir,
    })
}

pub fn ensure_workspace_layout(workspace_path: impl AsRef<Path>) -> Result<WorkspaceAuditorPaths> {
    let paths = workspace_auditor_paths(workspace_path)?;
    fs::create_dir_all(&paths.reports_dir)
        .with_context(|| format!("failed to create {}", paths.reports_dir.display()))?;
    fs::create_dir_all(&paths.memory_corpus_dir)
        .with_context(|| format!("failed to create {}", paths.memory_corpus_dir.display()))?;
    fs::create_dir_all(&paths.artifacts_dir)
        .with_context(|| format!("failed to create {}", paths.artifacts_dir.display()))?;
    Ok(paths)
}

pub fn report_path(paths: &WorkspaceAuditorPaths, session_id: &str) -> PathBuf {
    paths.reports_dir.join(format!("{session_id}.md"))
}

pub fn artifact_session_path(paths: &WorkspaceAuditorPaths, session_id: &str) -> PathBuf {
    paths.artifacts_dir.join(format!("{session_id}.json"))
}

pub fn memory_corpus_path(paths: &WorkspaceAuditorPaths, memory_id: &str) -> PathBuf {
    paths.memory_corpus_dir.join(format!("{memory_id}.md"))
}

fn resolve_history_auditor_root_dir(home: &Path) -> PathBuf {
    let canonical = home.join(CANONICAL_HISTORY_AUDITOR_ROOT_DIR);
    if canonical.exists() {
        return canonical;
    }

    legacy_history_auditor_root_dir(home).unwrap_or(canonical)
}

fn legacy_history_auditor_root_dir(home: &Path) -> Option<PathBuf> {
    LEGACY_HISTORY_AUDITOR_ROOT_DIRS
        .iter()
        .map(|relative| home.join(relative))
        .find(|path| path.exists())
}

fn workspace_id_for_path(workspace_root: &Path) -> String {
    format!(
        "{:016x}",
        fnv_hash_bytes(
            FNV_OFFSET_BASIS,
            workspace_root.to_string_lossy().as_bytes()
        )
    )
}

fn fnv_hash_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::{history_auditor_root_dir, resolve_history_auditor_root_dir};
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn root_dir_defaults_to_standalone_location() {
        let home = tempdir().unwrap();
        assert_eq!(
            resolve_history_auditor_root_dir(home.path()),
            home.path().join(".history-auditor")
        );
    }

    #[test]
    fn root_dir_uses_existing_legacy_location_when_present() {
        let home = tempdir().unwrap();
        let legacy_root = home.path().join(".rust-chatbot/history-auditor");
        std::fs::create_dir_all(&legacy_root).unwrap();

        assert_eq!(resolve_history_auditor_root_dir(home.path()), legacy_root);
    }

    #[test]
    fn public_root_dir_is_non_empty() {
        assert_ne!(history_auditor_root_dir(), PathBuf::new());
    }
}
