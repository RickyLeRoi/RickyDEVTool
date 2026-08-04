use serde::Serialize;

use crate::adapters::gpu::GpuInfo;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use crate::exec;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TempReading {
    pub label: String,
    pub celsius: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub percent: f32,
    pub charging: bool,
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SensorsSnapshot {
    pub temps: Vec<TempReading>,
    pub battery: Option<Battery>,
    pub gpus: Vec<GpuInfo>,
    pub max_temp_c: Option<f32>,
}

pub async fn read() -> SensorsSnapshot {
    let temps = tokio::task::spawn_blocking(read_temps).await.unwrap_or_default();
    let battery = read_battery().await;
    let gpus = crate::adapters::gpu::read().await;
    let max_temp_c = temps
        .iter()
        .map(|t| t.celsius)
        .fold(None, |acc: Option<f32>, c| Some(acc.map_or(c, |m| m.max(c))));
    SensorsSnapshot { temps, battery, gpus, max_temp_c }
}

pub async fn read_for_alerts() -> serde_json::Value {
    let temps = tokio::task::spawn_blocking(read_temps).await.unwrap_or_default();
    let battery = read_battery().await;
    let max_temp_c = temps
        .iter()
        .map(|t| t.celsius)
        .fold(None, |acc: Option<f32>, c| Some(acc.map_or(c, |m| m.max(c))));
    serde_json::json!({ "maxTempC": max_temp_c, "battery": battery })
}

fn read_temps() -> Vec<TempReading> {
    let components = sysinfo::Components::new_with_refreshed_list();
    let mut out: Vec<TempReading> = Vec::new();
    for c in &components {
        if let Some(t) = c.temperature() {
            if t.is_finite() && t > 0.0 {
                out.push(TempReading { label: c.label().to_string(), celsius: t });
            }
        }
    }
    out.sort_by(|a, b| b.celsius.partial_cmp(&a.celsius).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(12);
    out
}

#[cfg(target_os = "macos")]
async fn read_battery() -> Option<Battery> {
    parse_pmset(&exec::text(exec::cmd("pmset").args(["-g", "batt"])).await?)
}

#[cfg(target_os = "macos")]
fn parse_pmset(text: &str) -> Option<Battery> {
    let line = text.lines().find(|l| l.contains('%'))?;
    let pct_end = line.find('%')?;
    let start = line[..pct_end]
        .rfind(|c: char| !c.is_ascii_digit())
        .map(|i| i + 1)
        .unwrap_or(0);
    let percent: f32 = line[start..pct_end].parse().ok()?;
    let state_raw = line[pct_end..].split(';').nth(1).map(str::trim).unwrap_or("");
    let discharging = state_raw.contains("discharg");
    let charging = !discharging && (state_raw.contains("charg") || text.contains("AC Power"));
    let state = if discharging {
        "in scarica"
    } else if state_raw.contains("charged") {
        "carica"
    } else if state_raw.contains("charg") {
        "in carica"
    } else {
        "alimentazione collegata"
    };
    Some(Battery { percent, charging, state: state.to_string() })
}

#[cfg(target_os = "linux")]
async fn read_battery() -> Option<Battery> {
    use std::path::Path;
    let base = ["/sys/class/power_supply/BAT0", "/sys/class/power_supply/BAT1"]
        .into_iter()
        .map(Path::new)
        .find(|p| p.exists())?;
    let percent: f32 = tokio::fs::read_to_string(base.join("capacity"))
        .await
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let status = tokio::fs::read_to_string(base.join("status"))
        .await
        .unwrap_or_default();
    let s = status.trim();
    let charging = s.eq_ignore_ascii_case("Charging") || s.eq_ignore_ascii_case("Full");
    let state = match s {
        "Charging" => "in carica",
        "Discharging" => "in scarica",
        "Full" => "carica",
        other => other,
    };
    Some(Battery { percent, charging, state: state.to_string() })
}

#[cfg(target_os = "windows")]
async fn read_battery() -> Option<Battery> {
    let text = exec::text(exec::cmd("powershell").args([
        "-NoProfile",
        "-Command",
        "$b = Get-CimInstance Win32_Battery | Select-Object -First 1; if ($b) { \"$($b.EstimatedChargeRemaining);$($b.BatteryStatus)\" }",
    ]))
    .await?;
    let line = text.trim();
    if line.is_empty() {
        return None;
    }
    let mut parts = line.split(';');
    let percent: f32 = parts.next()?.trim().parse().ok()?;
    let status: i32 = parts.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let charging = status != 1;
    let state = if status == 1 { "in scarica" } else { "alimentazione collegata" };
    Some(Battery { percent, charging, state: state.to_string() })
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
async fn read_battery() -> Option<Battery> {
    None
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn pmset_in_scarica() {
        let text = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=123)\t82%; discharging; 3:20 remaining present: true";
        let b = parse_pmset(text).expect("parse");
        assert!((b.percent - 82.0).abs() < 0.01);
        assert!(!b.charging);
        assert_eq!(b.state, "in scarica");
    }

    #[test]
    fn pmset_in_carica() {
        let text = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=123)\t95%; charging; 0:12 remaining present: true";
        let b = parse_pmset(text).expect("parse");
        assert!(b.charging);
        assert_eq!(b.state, "in carica");
    }

    #[test]
    fn pmset_senza_batteria() {
        assert!(parse_pmset("Now drawing from 'AC Power'\nNo batteries available").is_none());
    }
}
