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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsListing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<FsEntry>,
}

/// Cartelle e file (per il picker del tail log). A differenza di list_dirs
/// include anche i nascosti: su macOS i log stanno spesso sotto ~/Library
/// o in dotdir.
pub async fn list_entries(path: Option<String>) -> Result<FsListing, String> {
    let base = match path {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => dirs::home_dir().ok_or("home directory non trovata")?,
    };
    let base = base.canonicalize().map_err(|e| format!("percorso non valido: {e}"))?;
    if !base.is_dir() {
        return Err("il percorso non è una cartella".to_string());
    }

    let listing = tokio::task::spawn_blocking(move || {
        let mut entries_found = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&base) {
            for entry in entries.flatten() {
                let Ok(file_type) = entry.file_type() else { continue };
                if file_type.is_symlink() {
                    continue;
                }
                entries_found.push(FsEntry {
                    name: entry.file_name().to_string_lossy().to_string(),
                    path: entry.path().to_string_lossy().to_string(),
                    is_dir: file_type.is_dir(),
                    size_bytes: entry.metadata().map(|m| m.len()).unwrap_or(0),
                });
            }
        }
        // Cartelle prima, poi file, alfabetico dentro i gruppi.
        entries_found.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        FsListing {
            parent: base.parent().map(|p| p.to_string_lossy().to_string()),
            path: base.to_string_lossy().to_string(),
            entries: entries_found,
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
        let mut raw: Vec<(ProjectRef, Vec<PathBuf>)> = Vec::new();
        let mut slns: Vec<PathBuf> = Vec::new();
        let mut visited: usize = 0;
        let truncated = walk(&base, 0, &mut raw, &mut slns, &mut visited);

        // I csproj referenziati da una solution trovata appartengono a lei:
        // le loro cartelle non vanno elencate come progetti a sé (né annidate
        // né sorelle della cartella della .sln).
        let referenced = sln_referenced_csprojs(&slns);
        let mut projects: Vec<ProjectRef> = Vec::new();
        for (mut project, csprojs) in raw {
            let only_referenced = !csprojs.is_empty()
                && csprojs.iter().all(|c| {
                    c.canonicalize()
                        .map(|c| referenced.contains(&c))
                        .unwrap_or(false)
                });
            if only_referenced {
                project.kinds.retain(|k| *k != ProjectKind::Dotnet);
            }
            if !project.kinds.is_empty() {
                projects.push(project);
            }
        }
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

fn sln_referenced_csprojs(slns: &[PathBuf]) -> std::collections::HashSet<PathBuf> {
    let mut referenced = std::collections::HashSet::new();
    for sln in slns {
        let Ok(content) = std::fs::read_to_string(sln) else { continue };
        let Some(sln_dir) = sln.parent() else { continue };
        for rel in crate::services::dotnet::parse_sln(&content) {
            let path = sln_dir.join(rel.replace('\\', "/"));
            if let Ok(canonical) = path.canonicalize() {
                referenced.insert(canonical);
            }
        }
    }
    referenced
}

/// Limite di sicurezza contro cartelle enormi.
const MAX_VISITED: usize = 5000;

fn walk(
    dir: &Path,
    depth: usize,
    projects: &mut Vec<(ProjectRef, Vec<PathBuf>)>,
    slns: &mut Vec<PathBuf>,
    visited: &mut usize,
) -> bool {
    *visited += 1;
    if *visited > MAX_VISITED {
        return true;
    }

    let detected = detect_dir(dir);
    let is_git_root = detected.kinds.contains(&ProjectKind::Git);
    slns.extend(detected.slns);
    if !detected.kinds.is_empty() {
        projects.push((
            ProjectRef {
                path: dir.to_string_lossy().to_string(),
                name: dir
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| dir.to_string_lossy().to_string()),
                kinds: detected.kinds,
            },
            detected.csprojs,
        ));
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
            truncated |= walk(&entry.path(), depth + 1, projects, slns, visited);
        }
    }
    truncated
}

struct DirDetect {
    kinds: Vec<ProjectKind>,
    slns: Vec<PathBuf>,
    /// Csproj diretti della cartella; vuoto se la cartella ha una propria .sln
    /// (in quel caso è lei l'unità di progetto e non va mai filtrata).
    csprojs: Vec<PathBuf>,
}

fn detect_dir(dir: &Path) -> DirDetect {
    let mut kinds = Vec::new();
    // .git può essere directory (repo normale) o file (worktree/submodule).
    if dir.join(".git").exists() {
        kinds.push(ProjectKind::Git);
    }
    if dir.join("package.json").is_file() {
        kinds.push(ProjectKind::Node);
    }
    let mut slns = Vec::new();
    let mut csprojs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.ends_with(".sln") || name.ends_with(".slnx") {
                slns.push(entry.path());
            } else if name.ends_with(".csproj") {
                csprojs.push(entry.path());
            }
        }
    }
    if !slns.is_empty() || !csprojs.is_empty() {
        kinds.push(ProjectKind::Dotnet);
    }
    if !slns.is_empty() {
        csprojs.clear();
    }
    DirDetect { kinds, slns, csprojs }
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
    async fn i_progetti_di_una_solution_non_sono_elencati_a_parte() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Layout come nel repo reale: Share/ contiene la .sln, i csproj sono
        // in cartelle sorelle referenziate dalla solution.
        std::fs::create_dir_all(root.join("Share")).unwrap();
        std::fs::create_dir_all(root.join("Share.Algorithms")).unwrap();
        std::fs::create_dir_all(root.join("Share.Dto")).unwrap();
        std::fs::create_dir_all(root.join("Indipendente")).unwrap();
        std::fs::write(
            root.join("Share/Share.sln"),
            r#"Project("{FAE04EC0-301F-11D3-BF4B-00C04F79EFBC}") = "Share.Algorithms", "..\Share.Algorithms\Share.Algorithms.csproj", "{1}"
EndProject
Project("{FAE04EC0-301F-11D3-BF4B-00C04F79EFBC}") = "Share.Dto", "..\Share.Dto\Share.Dto.csproj", "{2}"
EndProject
"#,
        )
        .unwrap();
        std::fs::write(root.join("Share.Algorithms/Share.Algorithms.csproj"), "<Project/>").unwrap();
        std::fs::write(root.join("Share.Dto/Share.Dto.csproj"), "<Project/>").unwrap();
        // csproj NON referenziato dalla sln: deve restare visibile.
        std::fs::write(root.join("Indipendente/Indipendente.csproj"), "<Project/>").unwrap();

        let scan = scan(root.to_string_lossy().to_string()).await.expect("scan");
        let names: Vec<&str> = scan.projects.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"Share"), "{names:?}");
        assert!(names.contains(&"Indipendente"), "{names:?}");
        assert!(!names.contains(&"Share.Algorithms"), "{names:?}");
        assert!(!names.contains(&"Share.Dto"), "{names:?}");
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
