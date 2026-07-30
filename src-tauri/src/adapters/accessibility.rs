use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessibilityStatus {
    /// true solo dove il permesso è un concetto reale (macOS).
    pub supported: bool,
    /// true se il processo può sintetizzare input (muovere il mouse).
    pub trusted: bool,
}

pub fn status() -> AccessibilityStatus {
    AccessibilityStatus {
        supported: imp::supported(),
        trusted: imp::trusted(),
    }
}

pub fn open_settings() -> Result<(), String> {
    imp::open_settings()
}

/// Apre il Colorimetro digitale (solo macOS): usato dal color picker come
/// alternativa all'EyeDropper, che WKWebView non espone.
pub fn open_color_meter() -> Result<(), String> {
    imp::open_color_meter()
}

#[cfg(target_os = "macos")]
mod imp {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }

    pub fn supported() -> bool {
        true
    }

    pub fn trusted() -> bool {
        unsafe { AXIsProcessTrusted() != 0 }
    }

    pub fn open_settings() -> Result<(), String> {
        crate::exec::sync_cmd("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn open_color_meter() -> Result<(), String> {
        crate::exec::sync_cmd("open")
            .args(["-a", "Digital Color Meter"])
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

// Windows: SendInput non richiede permessi speciali. Linux: jiggler non attivo.
#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn supported() -> bool {
        false
    }
    pub fn trusted() -> bool {
        true
    }
    pub fn open_settings() -> Result<(), String> {
        Err("Il permesso Accessibilità esiste solo su macOS".into())
    }
    pub fn open_color_meter() -> Result<(), String> {
        Err("Il Colorimetro digitale esiste solo su macOS".into())
    }
}
