//! Profili di avvio composito ("compound launch"): un profilo raggruppa più
//! step (riga di comando + cartella) che si lanciano insieme con un click,
//! ciascuno come task del [`crate::tasks::TaskRegistry`]. Persistiti in config.

use rand::RngCore;
use serde::{Deserialize, Serialize};

/// Uno step: una riga di comando eseguita nella shell dentro `cwd`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LaunchStep {
    pub label: String,
    pub command: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LaunchBundle {
    pub id: String,
    pub name: String,
    pub steps: Vec<LaunchStep>,
}

const MAX_STEPS: usize = 20;

impl LaunchBundle {
    /// Ripulisce e valida un profilo in arrivo dal client: niente step vuoti,
    /// nome obbligatorio, cwd non vuoto, tetto al numero di step.
    pub fn sanitized(mut self) -> Result<Self, String> {
        self.name = self.name.trim().to_string();
        if self.name.is_empty() {
            return Err("il profilo deve avere un nome".to_string());
        }
        self.steps.retain(|s| !s.command.trim().is_empty());
        if self.steps.is_empty() {
            return Err("aggiungi almeno uno step con un comando".to_string());
        }
        if self.steps.len() > MAX_STEPS {
            return Err(format!("troppi step (max {MAX_STEPS})"));
        }
        for step in &mut self.steps {
            step.command = step.command.trim().to_string();
            step.cwd = step.cwd.trim().to_string();
            step.label = step.label.trim().to_string();
            if step.cwd.is_empty() {
                return Err("ogni step deve avere una cartella di lavoro".to_string());
            }
            if step.label.is_empty() {
                // Etichetta di ripiego: la prima parola del comando.
                step.label = step.command.split_whitespace().next().unwrap_or("step").to_string();
            }
        }
        Ok(self)
    }
}

/// Genera un id univoco per un nuovo profilo (i profili esistenti tengono il
/// loro). Il suffisso casuale evita collisioni tra profili creati nello stesso
/// millisecondo (che l'upsert scambierebbe per una modifica, sovrascrivendo).
pub fn new_id() -> String {
    let mut buf = [0u8; 4];
    rand::rng().fill_bytes(&mut buf);
    let suffix: String = buf.iter().map(|b| format!("{b:02x}")).collect();
    format!("lb-{}-{}", crate::events::now_ms(), suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(cmd: &str) -> LaunchStep {
        LaunchStep { label: String::new(), command: cmd.to_string(), cwd: "/tmp".to_string() }
    }

    #[test]
    fn sanitized_scarta_step_vuoti_e_riempie_label() {
        let b = LaunchBundle {
            id: "x".into(),
            name: "  Stack  ".into(),
            steps: vec![step("npm run dev"), step("   "), step("dotnet run")],
        }
        .sanitized()
        .unwrap();
        assert_eq!(b.name, "Stack");
        assert_eq!(b.steps.len(), 2); // lo step vuoto è sparito
        assert_eq!(b.steps[0].label, "npm"); // label di ripiego
    }

    #[test]
    fn sanitized_rifiuta_senza_nome_o_senza_step() {
        let no_name = LaunchBundle { id: "x".into(), name: "  ".into(), steps: vec![step("ls")] };
        assert!(no_name.sanitized().is_err());
        let no_steps = LaunchBundle { id: "x".into(), name: "n".into(), steps: vec![step("  ")] };
        assert!(no_steps.sanitized().is_err());
    }

    #[test]
    fn sanitized_rifiuta_cwd_vuoto() {
        let bad = LaunchBundle {
            id: "x".into(),
            name: "n".into(),
            steps: vec![LaunchStep { label: "a".into(), command: "ls".into(), cwd: "".into() }],
        };
        assert!(bad.sanitized().is_err());
    }
}
