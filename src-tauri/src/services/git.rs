use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRepoInfo {
    pub root: String,
    pub current_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detached_at: Option<String>,
    pub dirty: bool,
    pub dirty_files: u32,
    pub ahead: Option<i64>,
    pub behind: Option<i64>,
    pub last_fetch_at: Option<u64>,
    pub warnings: Vec<GitWarning>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum GitWarning {
    NoUpstream,
    Diverged { ahead: i64, behind: i64 },
    DetachedHead,
    MergeInProgress,
    StaleFetch { days: u64 },
}

#[derive(Debug)]
pub enum GitError {
    NotARepo,
    AuthFailed(String),
    Failed(String),
    Timeout,
}

const INFO_TIMEOUT: Duration = Duration::from_secs(10);
const NETWORK_TIMEOUT: Duration = Duration::from_secs(90);
const STALE_FETCH_DAYS: u64 = 7;

async fn run_git(repo: &str, args: &[&str], timeout: Duration) -> Result<String, GitError> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("-C")
        .arg(repo)
        .args(args)
        // Mai bloccarsi su un prompt credenziali: meglio fallire con errore chiaro.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_SSH_COMMAND", "ssh -oBatchMode=yes");
    let output = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| GitError::Timeout)?
        .map_err(|e| GitError::Failed(format!("git non eseguibile: {e}")))?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let lower = stderr.to_lowercase();
    if lower.contains("not a git repository") {
        Err(GitError::NotARepo)
    } else if lower.contains("authentication")
        || lower.contains("could not read username")
        || lower.contains("permission denied (publickey")
        || lower.contains("terminal prompts disabled")
    {
        Err(GitError::AuthFailed(stderr.trim().to_string()))
    } else {
        Err(GitError::Failed(stderr.trim().to_string()))
    }
}

pub async fn repo_info(path: &str) -> Result<GitRepoInfo, GitError> {
    let root = run_git(path, &["rev-parse", "--show-toplevel"], INFO_TIMEOUT)
        .await?
        .trim()
        .to_string();
    let git_dir = run_git(path, &["rev-parse", "--absolute-git-dir"], INFO_TIMEOUT)
        .await?
        .trim()
        .to_string();
    let status = run_git(
        path,
        &["status", "--porcelain=v2", "--branch"],
        INFO_TIMEOUT,
    )
    .await?;

    let mut current_branch: Option<String> = None;
    let mut oid_short: Option<String> = None;
    let mut has_upstream = false;
    let mut ahead: Option<i64> = None;
    let mut behind: Option<i64> = None;
    let mut dirty_files: u32 = 0;

    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("# branch.oid ") {
            if rest != "(initial)" {
                oid_short = Some(rest.chars().take(7).collect());
            }
        } else if let Some(rest) = line.strip_prefix("# branch.head ") {
            if rest != "(detached)" {
                current_branch = Some(rest.to_string());
            }
        } else if line.starts_with("# branch.upstream ") {
            has_upstream = true;
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            // formato: "+A -B"
            let mut parts = rest.split_whitespace();
            ahead = parts.next().and_then(|s| s.trim_start_matches('+').parse().ok());
            behind = parts.next().and_then(|s| s.trim_start_matches('-').parse().ok());
        } else if line.starts_with(['1', '2', 'u', '?']) {
            dirty_files += 1;
        }
    }

    let last_fetch_at = std::fs::metadata(PathBuf::from(&git_dir).join("FETCH_HEAD"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64);

    let merge_in_progress = PathBuf::from(&git_dir).join("MERGE_HEAD").exists();

    let mut warnings = Vec::new();
    if current_branch.is_none() {
        warnings.push(GitWarning::DetachedHead);
    } else if !has_upstream {
        warnings.push(GitWarning::NoUpstream);
    }
    if let (Some(a), Some(b)) = (ahead, behind) {
        if a > 0 && b > 0 {
            warnings.push(GitWarning::Diverged { ahead: a, behind: b });
        }
    }
    if merge_in_progress {
        warnings.push(GitWarning::MergeInProgress);
    }
    if let Some(fetched) = last_fetch_at {
        let age_ms = crate::events::now_ms().saturating_sub(fetched);
        let days = age_ms / 86_400_000;
        if days >= STALE_FETCH_DAYS {
            warnings.push(GitWarning::StaleFetch { days });
        }
    }

    Ok(GitRepoInfo {
        root,
        detached_at: if current_branch.is_none() { oid_short } else { None },
        current_branch,
        dirty: dirty_files > 0,
        dirty_files,
        ahead: has_upstream.then_some(ahead.unwrap_or(0)),
        behind: has_upstream.then_some(behind.unwrap_or(0)),
        last_fetch_at,
        warnings,
    })
}

/// `git fetch --prune`; ritorna lo stato aggiornato.
pub async fn fetch(path: &str) -> Result<GitRepoInfo, GitError> {
    run_git(path, &["fetch", "--prune", "--quiet"], NETWORK_TIMEOUT).await?;
    repo_info(path).await
}

/// `git pull --ff-only`: mai merge automatici; se diverged fallisce con messaggio chiaro.
pub async fn pull(path: &str) -> Result<(GitRepoInfo, String), GitError> {
    let output = run_git(path, &["pull", "--ff-only"], NETWORK_TIMEOUT).await?;
    let info = repo_info(path).await?;
    let summary = output.lines().last().unwrap_or("").trim().to_string();
    Ok((info, summary))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_str().unwrap();
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.email", "test@test.it"],
            vec!["config", "user.name", "Test"],
            vec!["commit", "--allow-empty", "-m", "init"],
        ] {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(path)
                .args(&args)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        }
        dir
    }

    #[tokio::test]
    async fn info_su_repo_pulito() {
        let dir = init_repo().await;
        let info = repo_info(dir.path().to_str().unwrap()).await.expect("info");
        assert_eq!(info.current_branch.as_deref(), Some("main"));
        assert!(!info.dirty);
        assert_eq!(info.ahead, None); // nessun upstream
        assert!(info.warnings.iter().any(|w| matches!(w, GitWarning::NoUpstream)));
    }

    #[tokio::test]
    async fn info_rileva_dirty() {
        let dir = init_repo().await;
        std::fs::write(dir.path().join("nuovo.txt"), "ciao").unwrap();
        let info = repo_info(dir.path().to_str().unwrap()).await.expect("info");
        assert!(info.dirty);
        assert_eq!(info.dirty_files, 1);
    }

    #[tokio::test]
    async fn cartella_non_repo() {
        let dir = tempfile::tempdir().unwrap();
        let result = repo_info(dir.path().to_str().unwrap()).await;
        assert!(matches!(result, Err(GitError::NotARepo)));
    }
}
