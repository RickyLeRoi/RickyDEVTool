// 20260704 RG unico punto di costruzione dei processi: mai `Command::new` altrove.
// Senza CREATE_NO_WINDOW ogni processo lampeggia una console su Windows.
use std::ffi::OsStr;
use std::time::Duration;

#[cfg(windows)]
use crate::constants::CREATE_NO_WINDOW;

pub fn cmd(program: impl AsRef<OsStr>) -> tokio::process::Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut c = tokio::process::Command::new(program);
    #[cfg(windows)]
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

pub fn sync_cmd(program: impl AsRef<OsStr>) -> std::process::Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut c = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn sync_cmd_with_console(program: impl AsRef<OsStr>) -> std::process::Command {
    std::process::Command::new(program)
}

pub async fn text(cmd: &mut tokio::process::Command) -> Option<String> {
    let out = cmd.output().await.ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub async fn text_within(cmd: &mut tokio::process::Command, timeout: Duration) -> Option<String> {
    tokio::time::timeout(timeout, text(cmd)).await.ok()?
}

#[cfg(test)]
mod tests {
    #[test]
    fn nessuno_costruisce_command_fuori_da_exec() {
        let mut colpevoli = Vec::new();
        visita_sorgenti(std::path::Path::new("src"), &mut |path, contenuto| {
            if path.ends_with("exec.rs") {
                return;
            }
            let produzione = match contenuto.find("#[cfg(test)]") {
                Some(i) => &contenuto[..i],
                None => contenuto,
            };
            for (n, riga) in produzione.lines().enumerate() {
                if riga.contains("Command::new") {
                    colpevoli.push(format!("{}:{}", path.display(), n + 1));
                }
            }
        });
        assert!(
            colpevoli.is_empty(),
            "questi punti costruiscono un Command senza passare da exec::cmd/sync_cmd \
             (su Windows aprirebbero una finestra di console): {colpevoli:#?}"
        );
    }

    fn visita_sorgenti(dir: &std::path::Path, f: &mut impl FnMut(&std::path::Path, &str)) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visita_sorgenti(&path, f);
            } else if path.extension().is_some_and(|e| e == "rs") {
                if let Ok(contenuto) = std::fs::read_to_string(&path) {
                    f(&path, &contenuto);
                }
            }
        }
    }
}
