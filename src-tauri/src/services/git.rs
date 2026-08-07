use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

use crate::exec;

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

use crate::constants::{GIT_INFO_TIMEOUT, GIT_MAX_COMMITS, GIT_NETWORK_TIMEOUT, GIT_STALE_FETCH_DAYS};

async fn run_git(repo: &str, args: &[&str], timeout: Duration) -> Result<String, GitError> {
    let mut cmd = exec::cmd("git");
    cmd.arg("-C")
        .arg(repo)
        .args(args)
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
    let root = run_git(path, &["rev-parse", "--show-toplevel"], GIT_INFO_TIMEOUT)
        .await?
        .trim()
        .to_string();
    let git_dir = run_git(path, &["rev-parse", "--absolute-git-dir"], GIT_INFO_TIMEOUT)
        .await?
        .trim()
        .to_string();
    let status = run_git(
        path,
        &["status", "--porcelain=v2", "--branch"],
        GIT_INFO_TIMEOUT,
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
        if days >= GIT_STALE_FETCH_DAYS {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_ref: Option<String>,
    pub last_commit: LastCommit,
    pub stale_weeks: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastCommit {
    pub short_hash: String,
    pub author_name: String,
    pub date: u64,
    pub subject: String,
}

pub async fn branches(path: &str) -> Result<Vec<GitBranch>, GitError> {
    let current = run_git(path, &["rev-parse", "--abbrev-ref", "HEAD"], GIT_INFO_TIMEOUT)
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
        GIT_INFO_TIMEOUT,
    )
    .await?;

    let now_s = crate::events::now_ms() / 1000;
    let weeks = |date_s: u64| now_s.saturating_sub(date_s) / (7 * 86_400);

    struct Raw {
        name: String,
        short_hash: String,
        date_s: u64,
        author: String,
        subject: String,
        is_remote: bool,
    }
    let mut raws: Vec<Raw> = Vec::new();
    for line in output.lines() {
        let fields: Vec<&str> = line.split('\x1f').collect();
        if fields.len() != 5 {
            continue;
        }
        let name = fields[0].to_string();
        if name.ends_with("/HEAD") {
            continue;
        }
        raws.push(Raw {
            is_remote: name.starts_with("origin/"),
            short_hash: fields[1].to_string(),
            date_s: fields[2].parse().unwrap_or(0),
            author: fields[3].to_string(),
            subject: fields[4].to_string(),
            name,
        });
    }

    let remote_by_short: std::collections::HashMap<String, (String, u64)> = raws
        .iter()
        .filter(|r| r.is_remote)
        .map(|r| {
            let short = r.name.trim_start_matches("origin/").to_string();
            (short, (r.name.clone(), r.date_s))
        })
        .collect();
    let local_names: std::collections::HashSet<&str> =
        raws.iter().filter(|r| !r.is_remote).map(|r| r.name.as_str()).collect();

    let mut result: Vec<GitBranch> = Vec::new();
    for r in &raws {
        let last_commit = LastCommit {
            short_hash: r.short_hash.clone(),
            author_name: r.author.clone(),
            date: r.date_s * 1000,
            subject: r.subject.clone(),
        };
        if r.is_remote {
            let short = r.name.trim_start_matches("origin/");
            if local_names.contains(short) {
                continue;
            }
            result.push(GitBranch {
                is_current: false,
                is_remote_only: true,
                remote_ref: None,
                stale_weeks: weeks(r.date_s),
                last_commit,
                name: r.name.clone(),
            });
        } else {
            let remote = remote_by_short.get(&r.name);
            let stale_date = remote.map(|(_, d)| *d).unwrap_or(r.date_s);
            result.push(GitBranch {
                is_current: r.name == current,
                is_remote_only: false,
                remote_ref: remote.map(|(name, _)| name.clone()),
                stale_weeks: weeks(stale_date),
                last_commit,
                name: r.name.clone(),
            });
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
    pub date: u64,
    pub subject: String,
    pub refs: Vec<String>,
}

fn valid_ref(r: &str) -> bool {
    !r.is_empty()
        && r.len() <= 200
        && !r.starts_with('-')
        && !r.contains("..")
        && r.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
}

pub async fn commits(
    path: &str,
    git_ref: Option<&str>,
    limit: u32,
    skip: u32,
) -> Result<Vec<GitCommit>, GitError> {
    if let Some(r) = git_ref {
        if !valid_ref(r) {
            return Err(GitError::Failed("ref non valido".to_string()));
        }
    }
    let limit = limit.clamp(1, GIT_MAX_COMMITS).to_string();
    let skip = skip.to_string();
    let mut args = vec![
        "log",
        "--max-count",
        &limit,
        "--skip",
        &skip,
        "--format=%H\x1f%h\x1f%an\x1f%ae\x1f%ct\x1f%s\x1f%D",
    ];
    if let Some(r) = git_ref {
        args.push(r);
    }
    let output = run_git(path, &args, GIT_INFO_TIMEOUT).await?;

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

fn parse_decorations(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|d| d.trim().trim_start_matches("HEAD -> ").trim().to_string())
        .filter(|d| !d.is_empty() && d != "HEAD")
        .collect()
}

fn valid_hash(hash: &str) -> bool {
    !hash.is_empty() && hash.len() <= 40 && hash.chars().all(|c| c.is_ascii_hexdigit())
}

pub async fn checkout_commit(path: &str, hash: &str) -> Result<GitRepoInfo, GitError> {
    if !valid_hash(hash) {
        return Err(GitError::Failed("hash commit non valido".to_string()));
    }
    let info = repo_info(path).await?;
    if info.dirty {
        return Err(GitError::Failed(
            "working tree non pulito: committa o stasha prima del checkout".to_string(),
        ));
    }
    run_git(path, &["checkout", hash], GIT_INFO_TIMEOUT).await?;
    repo_info(path).await
}

pub async fn delete_branch(
    path: &str,
    branch: &str,
    force: bool,
    remote: Option<&str>,
) -> Result<Vec<GitBranch>, GitError> {
    if branch.is_empty() || branch.starts_with('-') {
        return Err(GitError::Failed("nome branch non valido".to_string()));
    }
    let current = run_git(path, &["rev-parse", "--abbrev-ref", "HEAD"], GIT_INFO_TIMEOUT)
        .await
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if branch == current {
        return Err(GitError::Failed(
            "non puoi eliminare il branch corrente".to_string(),
        ));
    }
    let flag = if force { "-D" } else { "-d" };
    run_git(path, &["branch", flag, "--", branch], GIT_INFO_TIMEOUT).await?;

    if let Some(rem) = remote {
        let rem = rem.trim();
        if rem.is_empty()
            || rem.starts_with('-')
            || !rem.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            return Err(GitError::Failed("nome remote non valido".to_string()));
        }
        run_git(path, &["push", rem, "--delete", branch], GIT_NETWORK_TIMEOUT).await?;
    }
    branches(path).await
}

pub async fn revert_commit(path: &str, hash: &str) -> Result<GitRepoInfo, GitError> {
    if !valid_hash(hash) {
        return Err(GitError::Failed("hash commit non valido".to_string()));
    }
    let info = repo_info(path).await?;
    if info.dirty {
        return Err(GitError::Failed(
            "working tree non pulito: committa o stasha prima del revert".to_string(),
        ));
    }
    match run_git(path, &["revert", "--no-edit", hash], GIT_INFO_TIMEOUT).await {
        Ok(_) => repo_info(path).await,
        Err(e) => {
            let _ = run_git(path, &["revert", "--abort"], GIT_INFO_TIMEOUT).await;
            Err(conflict_hint(e, "revert"))
        }
    }
}

pub async fn cherry_pick_commit(path: &str, hash: &str) -> Result<GitRepoInfo, GitError> {
    if !valid_hash(hash) {
        return Err(GitError::Failed("hash commit non valido".to_string()));
    }
    let info = repo_info(path).await?;
    if info.dirty {
        return Err(GitError::Failed(
            "working tree non pulito: committa o stasha prima del cherry-pick".to_string(),
        ));
    }
    match run_git(path, &["cherry-pick", hash], GIT_INFO_TIMEOUT).await {
        Ok(_) => repo_info(path).await,
        Err(e) => {
            let _ = run_git(path, &["cherry-pick", "--abort"], GIT_INFO_TIMEOUT).await;
            Err(conflict_hint(e, "cherry-pick"))
        }
    }
}

fn conflict_hint(e: GitError, op: &str) -> GitError {
    match e {
        GitError::Failed(msg) if msg.to_lowercase().contains("conflict") => {
            GitError::Failed(format!("{op} annullato: conflitto con le modifiche correnti"))
        }
        other => other,
    }
}

pub async fn checkout(path: &str, branch: &str) -> Result<GitRepoInfo, GitError> {
    let info = repo_info(path).await?;
    if info.dirty {
        return Err(GitError::Failed(
            "working tree non pulito: committa o stasha prima del checkout".to_string(),
        ));
    }
    let target = branch.trim_start_matches("origin/");
    run_git(path, &["checkout", target], GIT_INFO_TIMEOUT).await?;
    repo_info(path).await
}

pub async fn fetch(path: &str) -> Result<GitRepoInfo, GitError> {
    run_git(path, &["fetch", "--prune", "--quiet"], GIT_NETWORK_TIMEOUT).await?;
    repo_info(path).await
}

pub async fn pull(path: &str) -> Result<(GitRepoInfo, String), GitError> {
    let output = run_git(path, &["pull", "--ff-only"], GIT_NETWORK_TIMEOUT).await?;
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
        assert_eq!(info.ahead, None);
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

        let commits = commits(path, None, 10, 0).await.expect("commits");
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].subject, "secondo commit");
        assert!(commits[0].refs.iter().any(|r| r == "main"));

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

    #[tokio::test]
    async fn delete_branch_elimina_e_rifiuta_corrente() {
        let dir = init_repo().await;
        let path = dir.path().to_str().unwrap();
        git_in(dir.path(), &["branch", "feature"]).await;

        let branches = delete_branch(path, "feature", false, None).await.expect("delete");
        assert!(!branches.iter().any(|b| b.name == "feature"));

        let err = delete_branch(path, "main", false, None).await;
        assert!(matches!(err, Err(GitError::Failed(_))));
        assert!(delete_branch(path, "-rf", false, None).await.is_err());
    }

    #[tokio::test]
    async fn revert_crea_commit_che_annulla() {
        let dir = init_repo().await;
        let path = dir.path().to_str().unwrap();
        std::fs::write(dir.path().join("f.txt"), "contenuto").unwrap();
        git_in(dir.path(), &["add", "."]).await;
        git_in(dir.path(), &["commit", "-m", "aggiunge f"]).await;
        assert!(dir.path().join("f.txt").exists());

        let head = commits(path, None, 1, 0).await.unwrap()[0].hash.clone();
        let info = revert_commit(path, &head).await.expect("revert");
        assert!(!info.dirty);
        assert!(!dir.path().join("f.txt").exists());
        assert_eq!(commits(path, None, 10, 0).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn cherry_pick_applica_commit_da_altro_branch() {
        let dir = init_repo().await;
        let path = dir.path().to_str().unwrap();
        git_in(dir.path(), &["checkout", "-b", "feature"]).await;
        std::fs::write(dir.path().join("nuovo.txt"), "x").unwrap();
        git_in(dir.path(), &["add", "."]).await;
        git_in(dir.path(), &["commit", "-m", "feature commit"]).await;
        let feat_hash = commits(path, None, 1, 0).await.unwrap()[0].hash.clone();

        git_in(dir.path(), &["checkout", "main"]).await;
        assert!(!dir.path().join("nuovo.txt").exists());

        let info = cherry_pick_commit(path, &feat_hash).await.expect("cherry-pick");
        assert!(!info.dirty);
        assert!(dir.path().join("nuovo.txt").exists());
        assert_eq!(info.current_branch.as_deref(), Some("main"));
    }

    #[tokio::test]
    async fn commits_di_un_ref_specifico_non_di_head() {
        let dir = init_repo().await;
        let path = dir.path().to_str().unwrap();
        git_in(dir.path(), &["checkout", "-b", "altro"]).await;
        std::fs::write(dir.path().join("solo-altro.txt"), "x").unwrap();
        git_in(dir.path(), &["add", "."]).await;
        git_in(dir.path(), &["commit", "-m", "solo su altro"]).await;
        git_in(dir.path(), &["checkout", "main"]).await;

        let head = commits(path, None, 10, 0).await.unwrap();
        assert_eq!(head.len(), 1);
        let altro = commits(path, Some("altro"), 10, 0).await.unwrap();
        assert_eq!(altro.len(), 2);
        assert_eq!(altro[0].subject, "solo su altro");

        assert!(commits(path, Some("--all"), 10, 0).await.is_err());
    }

    #[tokio::test]
    async fn remote_ref_popolato_e_delete_remoto() {
        let remote = tempfile::tempdir().unwrap();
        let remote_path = remote.path().to_str().unwrap();
        git_in(remote.path(), &["init", "--bare", "-b", "main"]).await;

        let dir = init_repo().await;
        let path = dir.path().to_str().unwrap();
        git_in(dir.path(), &["remote", "add", "origin", remote_path]).await;
        git_in(dir.path(), &["push", "-u", "origin", "main"]).await;
        git_in(dir.path(), &["checkout", "-b", "feature"]).await;
        git_in(dir.path(), &["push", "-u", "origin", "feature"]).await;
        git_in(dir.path(), &["checkout", "main"]).await;

        let list = branches(path).await.expect("branches");
        let feature = list.iter().find(|b| b.name == "feature").unwrap();
        assert_eq!(feature.remote_ref.as_deref(), Some("origin/feature"));

        delete_branch(path, "feature", false, Some("origin")).await.expect("delete");
        let after = branches(path).await.expect("branches");
        assert!(!after.iter().any(|b| b.name == "feature"));
        assert!(!after.iter().any(|b| b.name == "origin/feature"));
    }

    #[tokio::test]
    async fn vetusta_calcolata_sul_remoto_non_sul_locale() {
        let remote = tempfile::tempdir().unwrap();
        git_in(remote.path(), &["init", "--bare", "-b", "main"]).await;
        let dir = init_repo().await;
        let path = dir.path().to_str().unwrap();
        git_in(dir.path(), &["remote", "add", "origin", remote.path().to_str().unwrap()]).await;
        git_in(dir.path(), &["checkout", "-b", "topic"]).await;

        let old = "2020-01-01T00:00:00";
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["commit", "--allow-empty", "-m", "vecchio (remoto)"])
            .env("GIT_AUTHOR_DATE", old)
            .env("GIT_COMMITTER_DATE", old)
            .output()
            .unwrap();
        assert!(out.status.success());
        git_in(dir.path(), &["push", "-u", "origin", "topic"]).await;
        git_in(dir.path(), &["commit", "--allow-empty", "-m", "fresco (locale)"]).await;

        let list = branches(path).await.unwrap();
        let topic = list.iter().find(|b| b.name == "topic").unwrap();
        assert!(topic.stale_weeks > 100, "stale_weeks={}", topic.stale_weeks);
        assert_eq!(topic.last_commit.subject, "fresco (locale)");
    }
}
