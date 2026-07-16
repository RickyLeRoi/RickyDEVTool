use std::path::{Path, PathBuf};

use serde::Serialize;

const MAX_DEPTH: usize = 3;
const IGNORED_DIRS: &[&str] = &[
    "node_modules", ".git", "bin", "obj", "dist", "build", "target", "Library",
    ".venv", "venv", "vendor", ".next", ".nuxt", "coverage", "DerivedData",
];

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ProjectKind {
    Git,
    Node,
    Dotnet,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRef {
    pub path: String,
    pub name: String,
    pub kinds: Vec<ProjectKind>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderScan {
    pub path: String,
    pub projects: Vec<ProjectRef>,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntryInfo {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirListing {
    pub path: String,
    pub parent: Option<String>,
    pub dirs: Vec<DirEntryInfo>,
}

/// Elenco delle sottocartelle per il picker (niente file, niente nascoste).
pub async fn list_dirs(path: Option<String>) -> Result<DirListing, String> {
    let base = match path {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => dirs::home_dir().ok_or("home directory non trovata")?,
    };
    let base = base.canonicalize().map_err(|e| format!("percorso non valido: {e}"))?;
    if !base.is_dir() {
        return Err("il percorso non è una cartella".to_string());
    }

    let listing = tokio::task::spawn_blocking(move || {
        let mut dirs_found = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&base) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    dirs_found.push(DirEntryInfo {
                        path: entry.path().to_string_lossy().to_string(),
                        name,
                    });
                }
            }
        }
        dirs_found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        DirListing {
            parent: base.parent().map(|p| p.to_string_lossy().to_string()),
            path: base.to_string_lossy().to_string(),
            dirs: dirs_found,
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(listing)
}

/// Riconosce i progetti dentro `path` (inclusa la cartella stessa), profondità max 3.
pub async fn scan(path: String) -> Result<FolderScan, String> {
    let base = PathBuf::from(&path)
        .canonicalize()
        .map_err(|e| format!("percorso non valido: {e}"))?;
    if !base.is_dir() {
        return Err("il percorso non è una cartella".to_string());
    }

    tokio::task::spawn_blocking(move || {
        let mut projects = Vec::new();
        let mut visited: usize = 0;
        let truncated = walk(&base, 0, &mut projects, &mut visited);
        projects.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(FolderScan {
            path: base.to_string_lossy().to_string(),
            projects,
            truncated,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Limite di sicurezza contro cartelle enormi.
const MAX_VISITED: usize = 5000;

fn walk(dir: &Path, depth: usize, projects: &mut Vec<ProjectRef>, visited: &mut usize) -> bool {
    *visited += 1;
    if *visited > MAX_VISITED {
        return true;
    }

    let kinds = detect_kinds(dir);
    let is_git_root = kinds.contains(&ProjectKind::Git);
    if !kinds.is_empty() {
        projects.push(ProjectRef {
            path: dir.to_string_lossy().to_string(),
            name: dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| dir.to_string_lossy().to_string()),
            kinds,
        });
    }
    if depth >= MAX_DEPTH {
        return false;
    }
    // Dentro un repo git non si cercano altri progetti: il repo è l'unità.
    if is_git_root && depth > 0 {
        return false;
    }

    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    let mut truncated = false;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || IGNORED_DIRS.contains(&name.as_str()) {
            continue;
        }
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            truncated |= walk(&entry.path(), depth + 1, projects, visited);
        }
    }
    truncated
}

pub fn detect_kinds(dir: &Path) -> Vec<ProjectKind> {
    let mut kinds = Vec::new();
    // .git può essere directory (repo normale) o file (worktree/submodule).
    if dir.join(".git").exists() {
        kinds.push(ProjectKind::Git);
    }
    if dir.join("package.json").is_file() {
        kinds.push(ProjectKind::Node);
    }
    let has_dotnet = std::fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|e| {
                let name = e.file_name().to_string_lossy().to_lowercase();
                name.ends_with(".sln") || name.ends_with(".slnx") || name.ends_with(".csproj")
            })
        })
        .unwrap_or(false);
    if has_dotnet {
        kinds.push(ProjectKind::Dotnet);
    }
    kinds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rileva_progetti_annidati() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // progetto node
        std::fs::create_dir_all(root.join("webapp")).unwrap();
        std::fs::write(root.join("webapp/package.json"), "{}").unwrap();
        // repo git + dotnet nella stessa cartella
        std::fs::create_dir_all(root.join("api/.git")).unwrap();
        std::fs::write(root.join("api/Servizio.sln"), "").unwrap();
        // node_modules va ignorato
        std::fs::create_dir_all(root.join("webapp/node_modules/x")).unwrap();
        std::fs::write(root.join("webapp/node_modules/x/package.json"), "{}").unwrap();

        let scan = scan(root.to_string_lossy().to_string()).await.expect("scan");
        assert_eq!(scan.projects.len(), 2, "{:?}", scan.projects);
        let api = scan.projects.iter().find(|p| p.name == "api").unwrap();
        assert!(api.kinds.contains(&ProjectKind::Git));
        assert!(api.kinds.contains(&ProjectKind::Dotnet));
        let web = scan.projects.iter().find(|p| p.name == "webapp").unwrap();
        assert_eq!(web.kinds, vec![ProjectKind::Node]);
    }

    #[tokio::test]
    async fn list_dirs_esclude_nascoste() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("visibile")).unwrap();
        std::fs::create_dir(dir.path().join(".nascosta")).unwrap();
        std::fs::write(dir.path().join("file.txt"), "").unwrap();
        let listing = list_dirs(Some(dir.path().to_string_lossy().to_string()))
            .await
            .expect("list");
        assert_eq!(listing.dirs.len(), 1);
        assert_eq!(listing.dirs[0].name, "visibile");
    }
}
