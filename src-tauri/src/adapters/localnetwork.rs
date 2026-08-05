use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalNetworkStatus {
    pub supported: bool,
    pub granted: bool,
}

pub fn status() -> LocalNetworkStatus {
    LocalNetworkStatus {
        supported: imp::supported(),
        granted: imp::granted(),
    }
}

pub fn open_settings() -> Result<(), String> {
    imp::open_settings()
}

#[cfg(target_os = "macos")]
mod imp {
    use std::net::UdpSocket;

    // 20260805 ++ RG #ReteLocale macOS non espone un equivalente di AXIsProcessTrusted per la
    // rete locale: l'unico modo di leggere il permesso è provare a usarlo.
    const TRIGGER: &str = "224.0.0.251:5353";

    pub fn supported() -> bool {
        true
    }

    // EHOSTUNREACH: è così che si presenta il diniego, e solo così. Un host spento dà timeout,
    // un servizio chiuso dà ECONNREFUSED: nessuno dei due può essere scambiato per un diniego.
    const EHOSTUNREACH: i32 = 65;

    // Il dubbio si risolve sempre a favore del "concesso": un banner che non si può far sparire
    // è peggio di un banner assente, e chi è offline ha comunque il servizio irraggiungibile.
    pub fn granted() -> bool {
        // Senza un'interfaccia di rete attiva la sonda non dice niente sul permesso: qualunque
        // invio fallirebbe comunque, quindi non c'è nulla da accusare.
        if crate::netinfo::lan_ips().is_empty() {
            return true;
        }
        match UdpSocket::bind("0.0.0.0:0").and_then(|s| s.send_to(&[], TRIGGER)) {
            Ok(_) => true,
            Err(e) => e.raw_os_error() != Some(EHOSTUNREACH),
        }
    }

    pub fn open_settings() -> Result<(), String> {
        crate::exec::sync_cmd("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork")
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn supported() -> bool {
        false
    }
    pub fn granted() -> bool {
        true
    }
    pub fn open_settings() -> Result<(), String> {
        Err("Il permesso Rete locale esiste solo su macOS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuori_da_macos_non_blocca_nulla() {
        let s = status();
        // 20260805 ++ RG #ReteLocale dove il permesso non esiste `granted` deve restare true,
        // altrimenti Windows e Linux si vedrebbero un banner che non possono risolvere.
        assert!(s.supported || s.granted);
    }
}
