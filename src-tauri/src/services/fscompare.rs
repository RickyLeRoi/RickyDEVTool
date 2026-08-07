use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::constants::FSCOMPARE_MAX_ENTRIES;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffStatus {
    OnlyLeft,
    OnlyRight,
    Different,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffEntry {
    pub rel_path: String,
    pub status: DiffStatus,
    pub is_dir: bool,
    pub left_size: Option<u64>,
    pub right_size: Option<u64>,
    pub left_mtime: Option<u64>,
    pub right_mtime: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareResult {
    pub left: String,
    pub right: String,
    pub entries: Vec<DiffEntry>,
    pub compared: usize,
    pub identical: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    Left,
    Right,
}

struct Meta {
    is_dir: bool,
    size: u64,
    mtime: Option<u64>,
}

fn mtime_ms(meta: &std::fs::Metadata) -> Option<u64> {
    meta.modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)
}

fn read_children(dir: &Path, excludes: &[String]) -> BTreeMap<String, Meta> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else { continue };
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if excludes.iter().any(|e| e.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let meta = entry.metadata().ok();
        out.insert(
            name,
            Meta {
                is_dir: file_type.is_dir(),
                size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                mtime: meta.as_ref().and_then(mtime_ms),
            },
        );
    }
    out
}

struct Walk<'a> {
    excludes: &'a [String],
    entries: Vec<DiffEntry>,
    compared: usize,
    identical: usize,
    truncated: bool,
}

impl Walk<'_> {
    fn push(&mut self, entry: DiffEntry) {
        if self.entries.len() >= FSCOMPARE_MAX_ENTRIES {
            self.truncated = true;
            return;
        }
        self.entries.push(entry);
    }

    fn dirs(&mut self, left: &Path, right: &Path, rel: &str) {
        if self.truncated {
            return;
        }
        let left_children = read_children(left, self.excludes);
        let right_children = read_children(right, self.excludes);

        let mut names: Vec<&String> = left_children.keys().collect();
        names.extend(right_children.keys().filter(|n| !left_children.contains_key(*n)));
        names.sort();

        for name in names {
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            self.compared += 1;
            match (left_children.get(name), right_children.get(name)) {
                (Some(l), Some(r)) => {
                    if l.is_dir && r.is_dir {
                        self.dirs(&left.join(name), &right.join(name), &child_rel);
                    } else if l.is_dir != r.is_dir || l.size != r.size {
                        self.push(DiffEntry {
                            rel_path: child_rel,
                            status: DiffStatus::Different,
                            is_dir: l.is_dir || r.is_dir,
                            left_size: Some(l.size),
                            right_size: Some(r.size),
                            left_mtime: l.mtime,
                            right_mtime: r.mtime,
                        });
                    } else {
                        self.identical += 1;
                    }
                }
                (Some(l), None) => self.push(DiffEntry {
                    rel_path: child_rel,
                    status: DiffStatus::OnlyLeft,
                    is_dir: l.is_dir,
                    left_size: Some(l.size),
                    right_size: None,
                    left_mtime: l.mtime,
                    right_mtime: None,
                }),
                (None, Some(r)) => self.push(DiffEntry {
                    rel_path: child_rel,
                    status: DiffStatus::OnlyRight,
                    is_dir: r.is_dir,
                    left_size: None,
                    right_size: Some(r.size),
                    left_mtime: None,
                    right_mtime: r.mtime,
                }),
                (None, None) => {}
            }
            if self.truncated {
                return;
            }
        }
    }
}

fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy().to_string();
    #[cfg(windows)]
    {
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return match rest.strip_prefix("UNC\\") {
                Some(unc) => format!(r"\\{unc}"),
                None => rest.to_string(),
            };
        }
    }
    text
}

fn root(path: &str, label: &str) -> Result<PathBuf, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(format!("percorso {label} mancante"));
    }
    let resolved = PathBuf::from(trimmed)
        // 20260704 RG su Windows canonicalize() dà la forma verbatim (\\?\C:\…), da normalizzare.
        .canonicalize()
        .map_err(|e| format!("percorso {label} non valido: {e}"))?;
    if !resolved.is_dir() {
        return Err(format!("il percorso {label} non è una cartella"));
    }
    Ok(resolved)
}

fn clean_excludes(excludes: Vec<String>) -> Vec<String> {
    excludes
        .into_iter()
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .collect()
}

pub async fn compare(
    left: String,
    right: String,
    excludes: Vec<String>,
) -> Result<CompareResult, String> {
    let left_root = root(&left, "di sinistra")?;
    let right_root = root(&right, "di destra")?;
    if left_root == right_root {
        return Err("i due percorsi sono la stessa cartella".to_string());
    }

    tokio::task::spawn_blocking(move || {
        let excludes = clean_excludes(excludes);
        let mut walk = Walk {
            excludes: &excludes,
            entries: Vec::new(),
            compared: 0,
            identical: 0,
            truncated: false,
        };
        walk.dirs(&left_root, &right_root, "");
        CompareResult {
            left: display_path(&left_root),
            right: display_path(&right_root),
            entries: walk.entries,
            compared: walk.compared,
            identical: walk.identical,
            truncated: walk.truncated,
        }
    })
    .await
    .map_err(|e| e.to_string())
}

pub async fn children(
    left: String,
    right: String,
    rel: String,
    excludes: Vec<String>,
) -> Result<Vec<DiffEntry>, String> {
    let left_root = root(&left, "di sinistra")?;
    let right_root = root(&right, "di destra")?;
    let rel_path = safe_rel(&rel).ok_or("percorso relativo non valido")?;
    let left_dir = left_root.join(&rel_path);
    let right_dir = right_root.join(&rel_path);
    if !left_dir.is_dir() && !right_dir.is_dir() {
        return Err("la voce non è una cartella".to_string());
    }
    let prefix = rel.trim().replace('\\', "/").trim_end_matches('/').to_string();

    tokio::task::spawn_blocking(move || {
        let excludes = clean_excludes(excludes);
        let mut walk = Walk {
            excludes: &excludes,
            entries: Vec::new(),
            compared: 0,
            identical: 0,
            truncated: false,
        };
        walk.dirs(&left_dir, &right_dir, &prefix);
        walk.entries
    })
    .await
    .map_err(|e| e.to_string())
}

pub fn safe_rel(rel: &str) -> Option<PathBuf> {
    let rel = rel.trim().replace('\\', "/");
    if rel.is_empty() {
        return None;
    }
    let mut out = PathBuf::new();
    for component in Path::new(&rel).components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

fn resolve(root_dir: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = safe_rel(rel).ok_or("percorso relativo non valido")?;
    Ok(root_dir.join(rel))
}

fn copy_recursive(from: &Path, to: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(from).map_err(|e| format!("{}: {e}", from.display()))?;
    if meta.file_type().is_symlink() {
        return Err("i collegamenti simbolici non vengono copiati".to_string());
    }
    if meta.is_dir() {
        std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
        let entries = std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?;
        for entry in entries.flatten() {
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::copy(from, to).map_err(|e| format!("{}: {e}", to.display()))?;
    Ok(())
}

pub async fn copy_entry(
    from_root: String,
    to_root: String,
    rel: String,
    from_label: &'static str,
    to_label: &'static str,
) -> Result<(), String> {
    let from_root = root(&from_root, from_label)?;
    let to_root = root(&to_root, to_label)?;
    let source = resolve(&from_root, &rel)?;
    let target = resolve(&to_root, &rel)?;
    if !source.exists() {
        return Err(format!("{} non esiste più", source.display()));
    }
    tokio::task::spawn_blocking(move || {
        tracing::info!(from = %source.display(), to = %target.display(), "confronto: copia");
        copy_recursive(&source, &target)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn delete_entry(root_path: String, rel: String, label: &'static str) -> Result<(), String> {
    let root_dir = root(&root_path, label)?;
    let target = resolve(&root_dir, &rel)?;
    let meta = std::fs::symlink_metadata(&target)
        .map_err(|e| format!("{}: {e}", target.display()))?;
    tokio::task::spawn_blocking(move || {
        tracing::warn!(path = %target.display(), "confronto: eliminazione");
        let outcome = if meta.is_dir() {
            std::fs::remove_dir_all(&target)
        } else {
            std::fs::remove_file(&target)
        };
        outcome.map_err(|e| format!("{}: {e}", target.display()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rdt-fscompare-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn rel_path_non_esce_dalla_radice() {
        assert!(safe_rel("sub/file.txt").is_some());
        assert!(safe_rel("sub\\file.txt").is_some());
        assert!(safe_rel("./file.txt").is_some());
        assert!(safe_rel("../fuori.txt").is_none());
        assert!(safe_rel("sub/../../fuori.txt").is_none());
        assert!(safe_rel("/etc/passwd").is_none());
        assert!(safe_rel("").is_none());
        assert!(safe_rel("   ").is_none());
        #[cfg(windows)]
        {
            assert!(safe_rel("C:\\Windows\\system32").is_none());
            assert!(safe_rel("\\\\server\\share\\x").is_none());
        }
    }

    #[tokio::test]
    async fn confronto_trova_solo_le_differenze() {
        let base = temp_dir("diff");
        let left = base.join("a");
        let right = base.join("b");
        write(&left.join("uguale.txt"), "xxx");
        write(&right.join("uguale.txt"), "xxx");
        write(&left.join("diverso.txt"), "lungo lungo");
        write(&right.join("diverso.txt"), "corto");
        write(&left.join("solo-sx.txt"), "a");
        write(&right.join("solo-dx.txt"), "b");
        write(&left.join("sub/dentro.txt"), "ciao");
        write(&left.join("saltata/x.txt"), "x");
        write(&right.join("saltata/y.txt"), "y");

        let result = compare(
            left.to_string_lossy().to_string(),
            right.to_string_lossy().to_string(),
            vec!["saltata".to_string()],
        )
        .await
        .expect("confronto");

        let by_path: std::collections::HashMap<&str, &DiffEntry> = result
            .entries
            .iter()
            .map(|e| (e.rel_path.as_str(), e))
            .collect();

        assert_eq!(by_path["diverso.txt"].status, DiffStatus::Different);
        assert_eq!(by_path["solo-sx.txt"].status, DiffStatus::OnlyLeft);
        assert_eq!(by_path["solo-dx.txt"].status, DiffStatus::OnlyRight);
        assert_eq!(by_path["sub"].status, DiffStatus::OnlyLeft);
        assert!(by_path["sub"].is_dir);
        assert!(!by_path.contains_key("sub/dentro.txt"));
        assert!(!by_path.contains_key("uguale.txt"));
        assert!(!by_path.contains_key("saltata"));
        assert_eq!(result.identical, 1);
        assert!(!result.truncated);

        let kids = children(
            left.to_string_lossy().to_string(),
            right.to_string_lossy().to_string(),
            "sub".to_string(),
            Vec::new(),
        )
        .await
        .expect("figli");
        assert_eq!(kids.len(), 1);
        assert_eq!(kids[0].rel_path, "sub/dentro.txt");
        assert_eq!(kids[0].status, DiffStatus::OnlyLeft);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn copia_ed_elimina_restano_dentro_le_radici() {
        let base = temp_dir("azioni");
        let left = base.join("a");
        let right = base.join("b");
        write(&left.join("sub/file.txt"), "contenuto");
        std::fs::create_dir_all(&right).unwrap();
        let fuori = base.join("fuori.txt");
        std::fs::write(&fuori, "non toccare").unwrap();

        copy_entry(
            left.to_string_lossy().to_string(),
            right.to_string_lossy().to_string(),
            "sub".to_string(),
            "di sinistra",
            "di destra",
        )
        .await
        .expect("copia");
        assert_eq!(std::fs::read_to_string(right.join("sub/file.txt")).unwrap(), "contenuto");

        let escape = delete_entry(
            right.to_string_lossy().to_string(),
            "../fuori.txt".to_string(),
            "di destra",
        )
        .await;
        assert!(escape.is_err());
        assert!(fuori.exists());

        delete_entry(
            right.to_string_lossy().to_string(),
            "sub".to_string(),
            "di destra",
        )
        .await
        .expect("elimina");
        assert!(!right.join("sub").exists());

        let _ = std::fs::remove_dir_all(&base);
    }
}
