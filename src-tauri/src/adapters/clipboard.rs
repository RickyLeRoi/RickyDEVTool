//! Accesso alla clipboard di sistema via CLI del SO (niente FFI: coerente con
//! gli altri adapter). Lettura e scrittura passano da `pbpaste`/`pbcopy` su
//! macOS e da PowerShell su Windows; su altri OS non è supportato.
//!
//! La scrittura passa il testo via **stdin** (mai come argomento) così testo
//! arbitrario non può iniettare comandi.

use std::io::Write;
use std::process::{Command, Stdio};

/// Legge il testo attualmente negli appunti. `None` se vuoto, non testo, o SO
/// non supportato.
pub fn read_text() -> Option<String> {
    let mut cmd = read_command()?;
    let output = cmd
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Scrive `text` negli appunti di sistema.
pub fn write_text(text: &str) -> Result<(), String> {
    let mut cmd = write_command().ok_or("clipboard non supportata su questo sistema")?;
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("avvio comando clipboard fallito: {e}"))?;
    {
        let mut stdin = child.stdin.take().ok_or("stdin non disponibile")?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("scrittura clipboard fallita: {e}"))?;
        // stdin viene chiuso qui (fine scope): il comando può completare.
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("comando clipboard terminato con errore".to_string())
    }
}

pub fn supported() -> bool {
    read_command().is_some()
}

#[cfg(target_os = "macos")]
fn read_command() -> Option<Command> {
    Some(Command::new("pbpaste"))
}

#[cfg(target_os = "macos")]
fn write_command() -> Option<Command> {
    Some(Command::new("pbcopy"))
}

#[cfg(target_os = "windows")]
fn read_command() -> Option<Command> {
    let mut c = Command::new("powershell");
    // -Raw preserva il testo esatto (niente split/rejoin delle righe).
    c.args(["-NoProfile", "-Command", "Get-Clipboard -Raw"]);
    Some(c)
}

#[cfg(target_os = "windows")]
fn write_command() -> Option<Command> {
    let mut c = Command::new("powershell");
    c.args(["-NoProfile", "-Command", "$input | Set-Clipboard"]);
    Some(c)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn read_command() -> Option<Command> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn write_command() -> Option<Command> {
    None
}
