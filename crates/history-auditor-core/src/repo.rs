use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use super::schema::RepoProbe;

pub fn probe_local_repo(repo_path: &Path) -> Result<Option<RepoProbe>> {
    if !repo_path.exists() {
        return Ok(None);
    }
    let Some(branch) = run_git(repo_path, &["git", "branch", "--show-current"])? else {
        return Ok(None);
    };
    let head_commit = run_git(repo_path, &["git", "rev-parse", "HEAD"])?;
    let head_parent_count = run_git(
        repo_path,
        &["git", "rev-list", "--parents", "-n", "1", "HEAD"],
    )?
    .and_then(|line| parse_head_parent_count(&line));
    let mut log_lines = run_git_lines(repo_path, &["git", "log", "--format=%H%x09%s", "-n", "50"])?;
    let head_subject = log_lines
        .as_ref()
        .and_then(|lines| lines.first())
        .and_then(|line| line.split_once('\t'))
        .map(|(_, subject)| subject.trim().to_string());
    let status_lines = run_git_lines(repo_path, &["git", "status", "--porcelain=v2", "--branch"])?
        .unwrap_or_default();
    let cached_status_lines =
        run_git_lines(repo_path, &["git", "diff", "--cached", "--name-status"])?
            .unwrap_or_default();
    let recent_commits = log_lines
        .take()
        .unwrap_or_default()
        .drain(..)
        .filter_map(|line| line.split_once('\t').map(|(hash, _)| hash.to_string()))
        .collect();

    Ok(Some(RepoProbe {
        repo_path: repo_path.display().to_string(),
        branch: normalize_string(branch),
        head_commit,
        head_parent_count,
        head_subject,
        status_lines,
        cached_status_lines,
        recent_commits,
    }))
}

fn run_git(repo_path: &Path, argv: &[&str]) -> Result<Option<String>> {
    let output = Command::new(argv[0])
        .args(&argv[1..])
        .current_dir(repo_path)
        .output()
        .with_context(|| {
            format!(
                "failed to run `{}` in {}",
                argv.join(" "),
                repo_path.display()
            )
        })?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(normalize_string(
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
    ))
}

fn run_git_lines(repo_path: &Path, argv: &[&str]) -> Result<Option<Vec<String>>> {
    Ok(run_git(repo_path, argv)?.map(|value| {
        value
            .lines()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
    }))
}

fn normalize_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_head_parent_count(line: &str) -> Option<u8> {
    let parent_count = line.split_whitespace().count().checked_sub(1)?;
    u8::try_from(parent_count).ok()
}
