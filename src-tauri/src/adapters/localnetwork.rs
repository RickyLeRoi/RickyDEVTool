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

    use crate::constants::{EHOSTUNREACH, MDNS_PROBE_ADDR};

    pub fn supported() -> bool {
        true
    }

    // 20260805 RG nel dubbio si concede: un banner che non si può far sparire è peggio di uno assente.
    pub fn granted() -> bool {
        if crate::netinfo::lan_ips().is_empty() {
            return true;
        }
        match UdpSocket::bind("0.0.0.0:0").and_then(|s| s.send_to(&[], MDNS_PROBE_ADDR)) {
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
        // 20260805 RG dove il permesso non esiste `granted` resta true, o Windows e Linux si
        // vedrebbero un banner che non possono risolvere.
        assert!(s.supported || s.granted);
    }
}
