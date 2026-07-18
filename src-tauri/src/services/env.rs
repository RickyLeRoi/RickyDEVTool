use std::path::PathBuf;

use serde::Serialize;

/// Gestione dei file .env* di un progetto: lista, lettura parsata, attivazione
/// (copia su .env con backup). I valori sono segreti: gli endpoint che usano
/// questo modulo sono riservati a localhost o al controllo remoto attivo.

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvFile {
    pub name: String,
    pub size_bytes: u64,
    pub modified_at: Option<u64>,
    /// true per ".env": è il file che i tool leggono davvero.
    pub is_active: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvEntry {
    pub key: String,
    pub value: String,
    /// Riga di commento o non parsabile, mostrata com'è.
    pub raw: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvContent {
    pub file: String,
    pub entries: Vec<EnvEntry>,
}

/// Un nome è accettato solo se è esattamente ".env" o ".env.qualcosa":
/// niente path, niente traversal, niente altri dotfile.
pub fn valid_env_name(name: &str) -> bool {
    if name == ".env" {
        return true;
    }
    let Some(rest) = name.strip_prefix(".env.") else { return false };
    !rest.is_empty()
        && !rest.contains(['/', '\\'])
        && rest != "."
        && rest != ".."
}

fn project_dir(path: &str) -> Result<PathBuf, String> {
    let dir = PathBuf::from(path)
        .canonicalize()
        .map_err(|e| format!("percorso non valido: {e}"))?;
    if !dir.is_dir() {
        return Err("il percorso non è una cartella".to_string());
    }
    Ok(dir)
}

pub fn list(path: &str) -> Result<Vec<EnvFile>, String> {
    let dir = project_dir(path)?;
    let mut files = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !valid_env_name(&name) || !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let meta = entry.metadata().ok();
        files.push(EnvFile {
            is_active: name == ".env",
            size_bytes: meta.as_ref().map(|m| m.len()).unwrap_or(0),
            modified_at: meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64),
            name,
        });
    }
    // .env per primo, poi alfabetico: l'ordine è stabile nella UI.
    files.sort_by(|a, b| b.is_active.cmp(&a.is_active).then(a.name.cmp(&b.name)));
    Ok(files)
}

pub fn read(path: &str, file: &str) -> Result<EnvContent, String> {
    if !valid_env_name(file) {
        return Err(format!("nome file non valido: {file}"));
    }
    let dir = project_dir(path)?;
    let content = std::fs::read_to_string(dir.join(file))
        .map_err(|e| format!("lettura fallita: {e}"))?;
    Ok(EnvContent {
        file: file.to_string(),
        entries: parse_env(&content),
    })
}

/// Attiva `file` copiandolo su `.env`. Il .env corrente, se diverso,
/// viene salvato in `.env.bak` prima della sovrascrittura.
pub fn activate(path: &str, file: &str) -> Result<(), String> {
    if !valid_env_name(file) || file == ".env" {
        return Err(format!("file non attivabile: {file}"));
    }
    let dir = project_dir(path)?;
    let source = dir.join(file);
    if !source.is_file() {
        return Err(format!("{file} non esiste"));
    }
    let target = dir.join(".env");
    if target.is_file() {
        let current = std::fs::read(&target).map_err(|e| e.to_string())?;
        let incoming = std::fs::read(&source).map_err(|e| e.to_string())?;
        if current != incoming {
            std::fs::copy(&target, dir.join(".env.bak"))
                .map_err(|e| format!("backup di .env fallito: {e}"))?;
        }
    }
    std::fs::copy(&source, &target).map_err(|e| format!("copia fallita: {e}"))?;
    Ok(())
}

/// Parser minimale in stile dotenv: KEY=VALUE, commenti con #, quote esterne
/// rimosse. Le righe non parsabili restano visibili come `raw`.
fn parse_env(content: &str) -> Vec<EnvEntry> {
    let mut entries = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            entries.push(EnvEntry { key: String::new(), value: String::new(), raw: Some(line.to_string()) });
            continue;
        }
        let without_export = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        match without_export.split_once('=') {
            Some((key, value)) if !key.trim().is_empty() => {
                let mut value = value.trim().to_string();
                if (value.starts_with('"') && value.ends_with('"') && value.len() >= 2)
                    || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2)
                {
                    value = value[1..value.len() - 1].to_string();
                }
                entries.push(EnvEntry { key: key.trim().to_string(), value, raw: None });
            }
            _ => entries.push(EnvEntry {
                key: String::new(),
                value: String::new(),
                raw: Some(line.to_string()),
            }),
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nomi_env_validi() {
        assert!(valid_env_name(".env"));
        assert!(valid_env_name(".env.local"));
        assert!(valid_env_name(".env.production"));
        assert!(!valid_env_name("env"));
        assert!(!valid_env_name(".envrc"));
        assert!(!valid_env_name(".env."));
        assert!(!valid_env_name(".env.../../etc/passwd"));
        assert!(!valid_env_name(".env./secret"));
    }

    #[test]
    fn parse_e_quote() {
        let entries = parse_env("# commento\nA=1\nexport B=\"due parole\"\nC='x'\nriga rotta\n");
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[1].key, "A");
        assert_eq!(entries[2].value, "due parole");
        assert_eq!(entries[3].value, "x");
        assert!(entries[4].raw.is_some());
    }

    #[test]
    fn activate_con_backup() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        std::fs::write(dir.path().join(".env"), "A=vecchio").unwrap();
        std::fs::write(dir.path().join(".env.staging"), "A=nuovo").unwrap();

        activate(&root, ".env.staging").expect("activate");
        assert_eq!(std::fs::read_to_string(dir.path().join(".env")).unwrap(), "A=nuovo");
        assert_eq!(std::fs::read_to_string(dir.path().join(".env.bak")).unwrap(), "A=vecchio");

        // Attivare .env stesso o nomi strani è rifiutato.
        assert!(activate(&root, ".env").is_err());
        assert!(activate(&root, "../.env.staging").is_err());
    }

    #[test]
    fn lista_ordinata() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env.local"), "").unwrap();
        std::fs::write(dir.path().join(".env"), "").unwrap();
        std::fs::write(dir.path().join("altro.txt"), "").unwrap();
        let files = list(&dir.path().to_string_lossy()).expect("list");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, ".env");
        assert!(files[0].is_active);
    }
}
