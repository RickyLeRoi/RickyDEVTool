use serde::{Deserialize, Serialize};
use sysinfo::Disks;

use crate::exec;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub file_system: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_pct: f32,
    pub is_removable: bool,
    pub is_system: bool,
}

pub fn list() -> Vec<DiskInfo> {
    let disks = Disks::new_with_refreshed_list();
    let mut result: Vec<DiskInfo> = disks
        .iter()
        .filter(|d| relevant_mount(&d.mount_point().to_string_lossy()))
        .map(|d| {
            let total = d.total_space();
            let available = d.available_space();
            let mount = d.mount_point().to_string_lossy().to_string();
            DiskInfo {
                is_system: is_system_mount(&mount),
                name: {
                    let n = d.name().to_string_lossy().to_string();
                    if n.is_empty() { mount.clone() } else { n }
                },
                file_system: d.file_system().to_string_lossy().to_string(),
                total_bytes: total,
                available_bytes: available,
                used_pct: if total > 0 {
                    (total.saturating_sub(available)) as f32 / total as f32 * 100.0
                } else {
                    0.0
                },
                is_removable: d.is_removable(),
                mount_point: mount,
            }
        })
        .filter(|d| d.total_bytes > 0)
        .collect();

    result.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
    result.dedup_by(|a, b| a.mount_point == b.mount_point);
    result
}

#[cfg(target_os = "macos")]
fn relevant_mount(mount: &str) -> bool {
    mount == "/" || mount.starts_with("/Volumes/")
}
#[cfg(target_os = "macos")]
fn is_system_mount(mount: &str) -> bool {
    mount == "/"
}

#[cfg(target_os = "windows")]
fn relevant_mount(_mount: &str) -> bool {
    true
}
#[cfg(target_os = "windows")]
fn is_system_mount(mount: &str) -> bool {
    let sys = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    mount.to_uppercase().starts_with(&sys.to_uppercase())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn relevant_mount(mount: &str) -> bool {
    mount == "/" || mount.starts_with("/media") || mount.starts_with("/mnt") || mount.starts_with("/run/media")
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn is_system_mount(mount: &str) -> bool {
    mount == "/"
}

#[derive(Debug)]
pub enum DiskError {
    NotFound,
    NotRemovable,
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Unsupported(String),
    Failed { message: String, os_hint: Option<String> },
}

fn guard_target(mount_point: &str) -> Result<DiskInfo, DiskError> {
    let disk = list()
        .into_iter()
        .find(|d| d.mount_point == mount_point)
        .ok_or(DiskError::NotFound)?;
    if disk.is_system || !disk.is_removable {
        return Err(DiskError::NotRemovable);
    }
    Ok(disk)
}

pub async fn eject(mount_point: &str) -> Result<(), DiskError> {
    let disk = guard_target(mount_point)?;
    tracing::info!(mount = %disk.mount_point, "eject richiesto");
    eject_impl(&disk.mount_point).await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatRequest {
    pub mount_point: String,
    pub filesystem: String,
    pub label: String,
    #[serde(default)]
    pub whole_disk: bool,
    pub confirm_name: String,
}

pub async fn format(req: FormatRequest) -> Result<(), DiskError> {
    let disk = guard_target(&req.mount_point)?;
    if req.confirm_name.trim() != disk.name {
        return Err(DiskError::Failed {
            message: "Conferma non valida: digita il nome esatto del volume".into(),
            os_hint: None,
        });
    }
    let label = sanitize_label(&req.label, &disk.name);
    tracing::warn!(mount = %disk.mount_point, whole_disk = req.whole_disk, fs = %req.filesystem, "FORMAT richiesto");
    format_impl(&disk.mount_point, &req.filesystem, &label, req.whole_disk).await
}

fn sanitize_label(label: &str, current: &str) -> String {
    let cleaned: String = label
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ')
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        current.to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(target_os = "macos")]
async fn eject_impl(mount_point: &str) -> Result<(), DiskError> {
    run_diskutil(&["eject", mount_point]).await
}

#[cfg(target_os = "macos")]
async fn format_impl(
    mount_point: &str,
    filesystem: &str,
    label: &str,
    whole_disk: bool,
) -> Result<(), DiskError> {
    let fs = macos_fs(filesystem)?;
    if whole_disk {
        let device = whole_disk_device(mount_point).await?;
        run_diskutil(&["eraseDisk", fs, label, "GPT", &device]).await
    } else {
        run_diskutil(&["eraseVolume", fs, label, mount_point]).await
    }
}

#[cfg(target_os = "macos")]
fn macos_fs(filesystem: &str) -> Result<&'static str, DiskError> {
    match filesystem.to_lowercase().as_str() {
        "exfat" => Ok("ExFAT"),
        "fat32" => Ok("MS-DOS FAT32"),
        "apfs" => Ok("APFS"),
        "hfs+" => Ok("JHFS+"),
        other => Err(DiskError::Failed {
            message: format!("filesystem non supportato: {other}"),
            os_hint: None,
        }),
    }
}

#[cfg(target_os = "macos")]
async fn whole_disk_device(mount_point: &str) -> Result<String, DiskError> {
    let output = exec::cmd("diskutil")
        .args(["info", mount_point])
        .output()
        .await
        .map_err(|e| DiskError::Failed { message: e.to_string(), os_hint: None })?;
    let text = String::from_utf8_lossy(&output.stdout);
    let whole = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("Part of Whole:"))
        .map(|s| s.trim().to_string())
        .ok_or(DiskError::Failed {
            message: "impossibile determinare il disco fisico".into(),
            os_hint: None,
        })?;
    Ok(format!("/dev/{whole}"))
}

#[cfg(target_os = "macos")]
async fn run_diskutil(args: &[&str]) -> Result<(), DiskError> {
    let output = exec::cmd("diskutil")
        .args(args)
        .output()
        .await
        .map_err(|e| DiskError::Failed { message: e.to_string(), os_hint: None })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };
    Err(DiskError::Failed {
        message: format!("diskutil: {detail}"),
        os_hint: Some("Il volume potrebbe essere in uso: chiudi le app che lo stanno usando".into()),
    })
}

#[cfg(target_os = "windows")]
async fn eject_impl(mount_point: &str) -> Result<(), DiskError> {
    let letter = mount_point.trim_end_matches(['\\', '/']);
    let ps = format!(
        "(New-Object -comObject Shell.Application).Namespace(17).ParseName('{letter}').InvokeVerb('Eject')"
    );
    run_powershell(&ps).await
}

#[cfg(target_os = "windows")]
async fn format_impl(
    mount_point: &str,
    filesystem: &str,
    label: &str,
    _whole_disk: bool,
) -> Result<(), DiskError> {
    let fs = match filesystem.to_lowercase().as_str() {
        "exfat" => "exFAT",
        "fat32" => "FAT32",
        "apfs" | "hfs+" => {
            return Err(DiskError::Unsupported(
                "APFS/HFS+ non sono formattabili su Windows: usa exFAT o FAT32".into(),
            ))
        }
        other => return Err(DiskError::Failed { message: format!("filesystem non supportato: {other}"), os_hint: None }),
    };
    let letter = mount_point.trim_end_matches(['\\', '/']);
    let ps = format!(
        "Format-Volume -DriveLetter '{}' -FileSystem {fs} -NewFileSystemLabel '{label}' -Force -Confirm:$false",
        letter.trim_end_matches(':')
    );
    run_powershell(&ps).await
}

#[cfg(target_os = "windows")]
async fn run_powershell(script: &str) -> Result<(), DiskError> {
    let output = exec::cmd("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .await
        .map_err(|e| DiskError::Failed { message: e.to_string(), os_hint: None })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(DiskError::Failed {
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            os_hint: Some("Potrebbero servire privilegi di amministratore".into()),
        })
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
async fn eject_impl(mount_point: &str) -> Result<(), DiskError> {
    let output = exec::cmd("udisksctl")
        .args(["unmount", "-b", mount_point])
        .output()
        .await
        .map_err(|_| DiskError::Unsupported("udisksctl non disponibile".into()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(DiskError::Failed {
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            os_hint: None,
        })
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
async fn format_impl(_m: &str, _f: &str, _l: &str, _w: bool) -> Result<(), DiskError> {
    Err(DiskError::Unsupported(
        "La formattazione è disponibile su macOS e Windows".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lista_include_il_disco_di_sistema() {
        let disks = list();
        assert!(disks.iter().any(|d| d.is_system), "nessun disco di sistema tra {disks:?}");
        for d in &disks {
            assert!(d.total_bytes > 0);
            assert!(d.used_pct >= 0.0 && d.used_pct <= 100.0);
        }
    }

    #[tokio::test]
    async fn eject_rifiutato_su_disco_di_sistema() {
        let system = list().into_iter().find(|d| d.is_system).expect("disco di sistema");
        let result = eject(&system.mount_point).await;
        assert!(matches!(result, Err(DiskError::NotRemovable)));
    }

    #[tokio::test]
    async fn azione_su_mount_inesistente() {
        assert!(matches!(eject("/mount/inesistente").await, Err(DiskError::NotFound)));
    }

    #[test]
    fn sanitize_label_pulisce_e_ha_fallback() {
        assert_eq!(sanitize_label("Mio USB", "Vecchio"), "Mio USB");
        assert_eq!(sanitize_label("  ", "Vecchio"), "Vecchio");
        assert_eq!(sanitize_label("bad/name;rm", "X"), "badnamerm");
    }
}
