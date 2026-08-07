use rand::RngCore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub cwd: String,
}

impl Snippet {
    pub fn sanitized(mut self) -> Result<Self, String> {
        self.name = self.name.trim().to_string();
        self.command = self.command.trim().to_string();
        self.cwd = self.cwd.trim().to_string();
        if self.name.is_empty() {
            return Err("dai un nome allo snippet".to_string());
        }
        if self.command.is_empty() {
            return Err("inserisci il comando da eseguire".to_string());
        }
        Ok(self)
    }
}

pub fn new_id() -> String {
    let mut buf = [0u8; 4];
    rand::rng().fill_bytes(&mut buf);
    let suffix: String = buf.iter().map(|b| format!("{b:02x}")).collect();
    format!("sn-{}-{}", crate::events::now_ms(), suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitized_richiede_nome_e_comando() {
        let ok = Snippet {
            id: "x".into(),
            name: "  build  ".into(),
            command: "  npm run build  ".into(),
            cwd: "  /tmp  ".into(),
        }
        .sanitized()
        .unwrap();
        assert_eq!(ok.name, "build");
        assert_eq!(ok.command, "npm run build");
        assert_eq!(ok.cwd, "/tmp");

        let no_name = Snippet { id: "x".into(), name: " ".into(), command: "ls".into(), cwd: "".into() };
        assert!(no_name.sanitized().is_err());
        let no_cmd = Snippet { id: "x".into(), name: "n".into(), command: "  ".into(), cwd: "".into() };
        assert!(no_cmd.sanitized().is_err());
    }
}
