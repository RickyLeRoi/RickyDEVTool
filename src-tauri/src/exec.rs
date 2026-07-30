//! Unico punto di costruzione dei processi esterni.
//!
//! Nessun modulo deve chiamare `Command::new` direttamente: si passa da
//! [`cmd`] (async) o [`sync_cmd`] (sync). Il motivo è Windows — un processo
//! console avviato da una GUI senza `CREATE_NO_WINDOW` si porta dietro una
//! finestra di terminale visibile, e con gli adapter che pollano ogni 200ms
//! il risultato è uno sfarfallio continuo di console. Centralizzando la
//! costruzione, il flag non si può più dimenticare: non è disciplina, è
//! l'unica strada disponibile.
//!
//! Sopra ai costruttori ci sono le due forme d'uso ricorrenti — [`text`] e
//! [`text_within`] — che coprono il caso "lancia, pretendi successo, prendi
//! stdout": da sole valgono la maggior parte delle chiamate del progetto.

use std::ffi::OsStr;
use std::time::Duration;

/// Flag Windows che sopprime la finestra di console del processo figlio.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Comando asincrono pronto all'uso: già configurato per non aprire finestre.
/// Restituisce la `Command` vera di tokio, così restano disponibili tutte le
/// sue opzioni (`env`, `current_dir`, `stdin`, `spawn`, …).
pub fn cmd(program: impl AsRef<OsStr>) -> tokio::process::Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut c = tokio::process::Command::new(program);
    #[cfg(windows)]
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

/// Variante sincrona di [`cmd`], per i contesti non-async (Drop, callback del
/// tray, codice che deve restare bloccante).
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

/// Unica eccezione consentita a [`sync_cmd`]: i comandi la cui finestra di
/// console **è** la funzionalità richiesta (aprire un terminale per l'utente).
/// Esiste come funzione a sé, invece che come `Command::new` sparso, perché
/// così l'eccezione è dichiarata e si trova con un grep — se un giorno tornano
/// finestre indesiderate, i sospettati sono solo i chiamanti di questa.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn sync_cmd_with_console(program: impl AsRef<OsStr>) -> std::process::Command {
    std::process::Command::new(program)
}

/// Lancia il comando e ne restituisce lo stdout **solo se è uscito con
/// successo**. `None` copre indistintamente "binario assente", "exit code
/// diverso da zero" e "output illeggibile": per gli adapter best-effort è
/// esattamente la distinzione che serve.
pub async fn text(cmd: &mut tokio::process::Command) -> Option<String> {
    let out = cmd.output().await.ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Come [`text`], ma abbandona il comando dopo `timeout`. Serve ai binari che
/// possono restare appesi (rete, filesystem remoti): senza tetto una singola
/// chiamata bloccherebbe il poller che la ospita.
pub async fn text_within(cmd: &mut tokio::process::Command, timeout: Duration) -> Option<String> {
    tokio::time::timeout(timeout, text(cmd)).await.ok()?
}

#[cfg(test)]
mod tests {
    /// Il flag anti-finestra vale solo se *tutti* passano di qui: un singolo
    /// `Command::new` sparso rimette una console lampeggiante su Windows, e non
    /// se ne accorge nessuno finché non lo si prova su quel SO. Questo test è la
    /// rete: fallisce in CI su qualsiasi macchina, non solo su Windows.
    ///
    /// Il codice di test è escluso — le fixture girano da terminale, dove una
    /// console in più non disturba — così come [`super::sync_cmd_with_console`],
    /// che è l'eccezione dichiarata.
    #[test]
    fn nessuno_costruisce_command_fuori_da_exec() {
        let mut colpevoli = Vec::new();
        visita_sorgenti(std::path::Path::new("src"), &mut |path, contenuto| {
            if path.ends_with("exec.rs") {
                return;
            }
            // Per convenzione i test stanno in fondo al file: si guarda solo ciò
            // che li precede.
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
