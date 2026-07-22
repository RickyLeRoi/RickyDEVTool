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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranch {
    pub name: String,
    pub is_current: bool,
    pub is_remote_only: bool,
    pub last_commit: LastCommit,
    pub stale_weeks: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastCommit {
    pub short_hash: String,
    pub author_name: String,
    pub date: u64, // epoch ms
    pub subject: String,
}

/// Branch locali + remote-only, ordinati per data commit decrescente.
pub async fn branches(path: &str) -> Result<Vec<GitBranch>, GitError> {
    let current = run_git(path, &["rev-parse", "--abbrev-ref", "HEAD"], INFO_TIMEOUT)
        .await
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let output = run_git(
        path,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)\x1f%(objectname:short)\x1f%(committerdate:unix)\x1f%(authorname)\x1f%(subject)",
            "refs/heads",
            "refs/remotes",
        ],
        INFO_TIMEOUT,
    )
    .await?;

    let now_s = crate::events::now_ms() / 1000;
    let mut locals: Vec<GitBranch> = Vec::new();
    let mut remotes: Vec<GitBranch> = Vec::new();

    for line in output.lines() {
        let fields: Vec<&str> = line.split('\x1f').collect();
        if fields.len() != 5 {
            continue;
        }
        let full_name = fields[0].to_string();
        if full_name.ends_with("/HEAD") {
            continue;
        }
        let is_remote = full_name.contains('/') && full_name.starts_with("origin/");
        let date_s: u64 = fields[2].parse().unwrap_or(0);
        let branch = GitBranch {
            is_current: full_name == current,
            is_remote_only: is_remote, // rifinito sotto
            last_commit: LastCommit {
                short_hash: fields[1].to_string(),
                author_name: fields[3].to_string(),
                date: date_s * 1000,
                subject: fields[4].to_string(),
            },
            stale_weeks: now_s.saturating_sub(date_s) / (7 * 86_400),
            name: full_name,
        };
        if is_remote {
            remotes.push(branch);
        } else {
            locals.push(branch);
        }
    }

    // I remote con un locale corrispondente non sono "remote-only": si scartano.
    let local_names: std::collections::HashSet<String> =
        locals.iter().map(|b| b.name.clone()).collect();
    let mut result = locals;
    for remote in remotes {
        let short = remote.name.trim_start_matches("origin/");
        if !local_names.contains(short) {
            result.push(remote);
        }
    }
    result.sort_by(|a, b| b.last_commit.date.cmp(&a.last_commit.date));
    Ok(result)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommit {
    pub hash: String,
    pub short_hash: String,
    pub author_name: String,
    pub author_email: String,
    pub date: u64, // epoch ms
    pub subject: String,
    /// Decorazioni (branch/tag che puntano al commit), già ripulite.
    pub refs: Vec<String>,
}

const MAX_COMMITS: u32 = 200;

/// Log dei commit di HEAD, dal più recente. `skip` per la paginazione ("carica altri").
pub async fn commits(path: &str, limit: u32, skip: u32) -> Result<Vec<GitCommit>, GitError> {
    let limit = limit.clamp(1, MAX_COMMITS).to_string();
    let skip = skip.to_string();
    // Campi separati da \x1f; ogni commit sta su una riga (nessun campo del
    // formato contiene newline).
    let output = run_git(
        path,
        &[
            "log",
            "--max-count",
            &limit,
            "--skip",
            &skip,
            "--format=%H\x1f%h\x1f%an\x1f%ae\x1f%ct\x1f%s\x1f%D",
        ],
        INFO_TIMEOUT,
    )
    .await?;

    let mut commits = Vec::new();
    for line in output.lines() {
        let fields: Vec<&str> = line.split('\x1f').collect();
        if fields.len() != 7 {
            continue;
        }
        let date_s: u64 = fields[4].parse().unwrap_or(0);
        let refs = parse_decorations(fields[6]);
        commits.push(GitCommit {
            hash: fields[0].to_string(),
            short_hash: fields[1].to_string(),
            author_name: fields[2].to_string(),
            author_email: fields[3].to_string(),
            date: date_s * 1000,
            subject: fields[5].to_string(),
            refs,
        });
    }
    Ok(commits)
}

/// "HEAD -> main, origin/main, tag: v1.0" → ["main", "origin/main", "tag: v1.0"]
fn parse_decorations(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|d| d.trim().trim_start_matches("HEAD -> ").trim().to_string())
        .filter(|d| !d.is_empty() && d != "HEAD")
        .collect()
}

/// Checkout di un commit specifico: porta in detached HEAD. Rifiutato se dirty.
pub async fn checkout_commit(path: &str, hash: &str) -> Result<GitRepoInfo, GitError> {
    if hash.is_empty() || hash.len() > 40 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(GitError::Failed("hash commit non valido".to_string()));
    }
    let info = repo_info(path).await?;
    if info.dirty {
        return Err(GitError::Failed(
            "working tree non pulito: committa o stasha prima del checkout".to_string(),
        ));
    }
    // Un hash grezzo mette git in detached HEAD (nessun branch creato).
    run_git(path, &["checkout", hash], INFO_TIMEOUT).await?;
    repo_info(path).await
}

/// Checkout di un branch. Rifiutato se il working tree è dirty.
/// Per i branch remote-only usa il nome corto: git crea il tracking locale.
pub async fn checkout(path: &str, branch: &str) -> Result<GitRepoInfo, GitError> {
    let info = repo_info(path).await?;
    if info.dirty {
        return Err(GitError::Failed(
            "working tree non pulito: committa o stasha prima del checkout".to_string(),
        ));
    }
    let target = branch.trim_start_matches("origin/");
    run_git(path, &["checkout", target], INFO_TIMEOUT).await?;
    repo_info(path).await
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

    #[test]
    fn decorazioni_ripulite() {
        assert_eq!(
            parse_decorations("HEAD -> main, origin/main, tag: v1.0"),
            vec!["main", "origin/main", "tag: v1.0"]
        );
        assert_eq!(parse_decorations(""), Vec::<String>::new());
        assert_eq!(parse_decorations("HEAD"), Vec::<String>::new());
    }

    async fn git_in(dir: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    #[tokio::test]
    async fn commits_e_checkout_detached() {
        let dir = init_repo().await;
        let path = dir.path().to_str().unwrap();
        std::fs::write(dir.path().join("a.txt"), "1").unwrap();
        git_in(dir.path(), &["add", "."]).await;
        git_in(dir.path(), &["commit", "-m", "secondo commit"]).await;

        let commits = commits(path, 10, 0).await.expect("commits");
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].subject, "secondo commit");
        assert!(commits[0].refs.iter().any(|r| r == "main"));

        // Checkout del commit iniziale → detached HEAD.
        let initial = &commits[1].hash;
        let info = checkout_commit(path, initial).await.expect("checkout");
        assert!(info.current_branch.is_none());
        assert!(info.detached_at.is_some());
        assert!(info.warnings.iter().any(|w| matches!(w, GitWarning::DetachedHead)));
    }

    #[tokio::test]
    async fn checkout_commit_rifiuta_hash_invalido() {
        let dir = init_repo().await;
        let result = checkout_commit(dir.path().to_str().unwrap(), "non-esadecimale!").await;
        assert!(matches!(result, Err(GitError::Failed(_))));
    }
}
