//! GPU monitor best-effort. Con `nvidia-smi` (Windows/Linux, GPU NVIDIA) si
//! ottengono utilizzo, memoria e temperatura live; su macOS si legge almeno il
//! modello e la VRAM da `system_profiler` (l'utilizzo live non è esposto senza
//! permessi elevati). Quando non c'è nulla, torna una lista vuota.

use serde::Serialize;

use crate::exec;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub name: String,
    pub utilization_pct: Option<f32>,
    pub mem_used_mb: Option<u64>,
    pub mem_total_mb: Option<u64>,
    pub temp_c: Option<f32>,
    /// Da dove viene il dato: utile alla UI per spiegare i campi mancanti.
    pub source: String,
}

pub async fn read() -> Vec<GpuInfo> {
    if let Some(gpus) = nvidia().await {
        return gpus;
    }
    #[cfg(target_os = "macos")]
    {
        return macos().await;
    }
    #[allow(unreachable_code)]
    Vec::new()
}

async fn nvidia() -> Option<Vec<GpuInfo>> {
    let text = exec::text(exec::cmd("nvidia-smi").args([
        "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu",
        "--format=csv,noheader,nounits",
    ]))
    .await?;
    let gpus: Vec<GpuInfo> = text.lines().filter_map(parse_nvidia_line).collect();
    if gpus.is_empty() {
        None
    } else {
        Some(gpus)
    }
}

/// Riga CSV di `nvidia-smi --query-gpu=...`. Testato su fixture.
pub fn parse_nvidia_line(line: &str) -> Option<GpuInfo> {
    let cols: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    if cols.len() < 5 || cols[0].is_empty() {
        return None;
    }
    let f = |s: &str| s.parse::<f32>().ok();
    Some(GpuInfo {
        name: cols[0].to_string(),
        utilization_pct: f(cols[1]),
        mem_used_mb: cols[2].parse::<u64>().ok(),
        mem_total_mb: cols[3].parse::<u64>().ok(),
        temp_c: f(cols[4]),
        source: "nvidia-smi".to_string(),
    })
}

/// Su macOS l'unico dato disponibile (modello, VRAM) è statico: `system_profiler`
/// è lento, quindi lo si interroga una sola volta e si riusa il risultato.
#[cfg(target_os = "macos")]
async fn macos() -> Vec<GpuInfo> {
    let mut gpus = macos_static_cached().await;
    // Utilizzo live: ioreg espone "Device Utilization %" su molte GPU (Intel/AMD
    // e alcune Apple). Quando manca — tipico su Apple Silicon — resta None e la
    // UI lo dichiara. Abbinamento per ordine di comparsa (ok nel caso 1 GPU).
    let utils = macos_utilization().await;
    for (i, g) in gpus.iter_mut().enumerate() {
        if let Some(u) = utils.get(i).copied() {
            g.utilization_pct = Some(u);
        }
    }
    gpus
}

/// Parte statica (modello, VRAM): lenta da `system_profiler`, quindi in cache.
#[cfg(target_os = "macos")]
async fn macos_static_cached() -> Vec<GpuInfo> {
    use tokio::sync::OnceCell;
    static CACHE: OnceCell<Vec<GpuInfo>> = OnceCell::const_new();
    if let Some(v) = CACHE.get() {
        return v.clone();
    }
    let v = macos_probe().await;
    // Solo un risultato valido va in cache: un errore transitorio non deve
    // "congelare" una lista vuota per sempre.
    if !v.is_empty() {
        let _ = CACHE.set(v.clone());
    }
    v
}

#[cfg(target_os = "macos")]
async fn macos_utilization() -> Vec<f32> {
    let Some(text) = exec::text(exec::cmd("ioreg").args(["-r", "-c", "IOAccelerator", "-w", "0"])).await
    else {
        return Vec::new();
    };
    parse_ioreg_utilization(&text)
}

/// Estrae i valori `"Device Utilization %"` dall'output di ioreg, in ordine.
#[cfg(target_os = "macos")]
fn parse_ioreg_utilization(text: &str) -> Vec<f32> {
    let key = "\"Device Utilization %\"=";
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find(key) {
        let after = &rest[pos + key.len()..];
        let num: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(v) = num.parse::<f32>() {
            out.push(v);
        }
        rest = after;
    }
    out
}

#[cfg(target_os = "macos")]
async fn macos_probe() -> Vec<GpuInfo> {
    let Some(out) = exec::text(exec::cmd("system_profiler").args(["SPDisplaysDataType", "-json"])).await
    else {
        return Vec::new();
    };
    let v: serde_json::Value = match serde_json::from_str(&out) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(arr) = v.get("SPDisplaysDataType").and_then(|x| x.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .map(|g| {
            let name = g
                .get("sppci_model")
                .or_else(|| g.get("_name"))
                .and_then(|x| x.as_str())
                .unwrap_or("GPU")
                .to_string();
            let mem_total_mb = g
                .get("spdisplays_vram")
                .or_else(|| g.get("spdisplays_vram_shared"))
                .and_then(|x| x.as_str())
                .and_then(parse_vram_mb);
            GpuInfo {
                name,
                utilization_pct: None,
                mem_used_mb: None,
                mem_total_mb,
                temp_c: None,
                source: "system_profiler".to_string(),
            }
        })
        .collect()
}

/// "8 GB" / "1536 MB" → MB.
#[cfg(target_os = "macos")]
fn parse_vram_mb(s: &str) -> Option<u64> {
    let s = s.trim();
    let num: f64 = s.split_whitespace().next()?.parse().ok()?;
    if s.to_lowercase().contains("gb") {
        Some((num * 1024.0) as u64)
    } else {
        Some(num as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_nvidia() {
        let g = parse_nvidia_line("NVIDIA GeForce RTX 3080, 17, 2048, 10240, 54").expect("parse");
        assert_eq!(g.name, "NVIDIA GeForce RTX 3080");
        assert_eq!(g.utilization_pct, Some(17.0));
        assert_eq!(g.mem_used_mb, Some(2048));
        assert_eq!(g.mem_total_mb, Some(10240));
        assert_eq!(g.temp_c, Some(54.0));
        assert!(parse_nvidia_line("").is_none());
        assert!(parse_nvidia_line("solo, tre, colonne").is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parse_ioreg_util() {
        let text = r#"
          | | | "PerformanceStatistics" = {"Device Utilization %"=37,"In use system memory"=123}
          | | | "PerformanceStatistics" = {"Device Utilization %"=0}
        "#;
        assert_eq!(parse_ioreg_utilization(text), vec![37.0, 0.0]);
        assert!(parse_ioreg_utilization("nessuna statistica qui").is_empty());
    }
}
