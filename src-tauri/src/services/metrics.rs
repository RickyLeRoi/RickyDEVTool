//! Storico metriche 24h su SQLite.
//!
//! A differenza del collector `stats` (che pubblica sul bus solo quando la
//! dashboard è aperta), qui il campionamento è **sempre attivo** finché l'app
//! gira, così lo storico è continuo anche se non hai mai aperto la UI. Gira su
//! un thread OS dedicato (sysinfo e rusqlite sono entrambi sincroni e la
//! frequenza è bassa); le query dagli handler async passano da
//! `spawn_blocking` sullo stesso `Mutex<Connection>`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;
use serde::Serialize;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

use crate::events::now_ms;

/// Un campione ogni 30s → 2880 punti/24h: leggero per SQLite e per il grafico.
const SAMPLE_INTERVAL: Duration = Duration::from_secs(30);
/// Si tiene un po' più di 24h per avere sempre una finestra piena.
const RETENTION_MS: u64 = 25 * 3_600_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricSample {
    pub ts: u64,
    pub cpu_pct: f32,
    pub mem_pct: f32,
    pub disk_pct: Option<f32>,
}

pub struct MetricsService {
    conn: Option<Arc<Mutex<Connection>>>,
}

impl MetricsService {
    pub fn start() -> Arc<Self> {
        let conn = match open_db() {
            Ok(conn) => Some(Arc::new(Mutex::new(conn))),
            Err(e) => {
                tracing::error!(%e, "metriche: DB non apribile, storico disattivato");
                None
            }
        };
        let service = Arc::new(Self { conn: conn.clone() });

        if let Some(conn) = conn {
            std::thread::Builder::new()
                .name("metrics-sampler".into())
                .spawn(move || sampler_loop(conn))
                .ok();
        }
        service
    }

    /// Campioni delle ultime `hours` ore, dal più vecchio al più recente.
    pub fn history(&self, hours: u32) -> Vec<MetricSample> {
        let Some(conn) = &self.conn else { return Vec::new() };
        let since = now_ms().saturating_sub(hours.clamp(1, 48) as u64 * 3_600_000);
        let conn = conn.lock().expect("metrics lock");
        let mut stmt = match conn
            .prepare("SELECT ts, cpu_pct, mem_pct, disk_pct FROM samples WHERE ts >= ?1 ORDER BY ts ASC")
        {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(%e, "metriche: query fallita");
                return Vec::new();
            }
        };
        let rows = stmt.query_map([since], |row| {
            Ok(MetricSample {
                ts: row.get(0)?,
                cpu_pct: row.get(1)?,
                mem_pct: row.get(2)?,
                disk_pct: row.get(3)?,
            })
        });
        match rows {
            Ok(mapped) => mapped.filter_map(Result::ok).collect(),
            Err(_) => Vec::new(),
        }
    }
}

fn open_db() -> rusqlite::Result<Connection> {
    let path = crate::config::data_dir().join("metrics.db");
    let conn = Connection::open(path)?;
    // WAL: letture concorrenti con la scrittura del sampler senza bloccarsi.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS samples (
            ts INTEGER PRIMARY KEY,
            cpu_pct REAL NOT NULL,
            mem_pct REAL NOT NULL,
            disk_pct REAL
        )",
        [],
    )?;
    Ok(conn)
}

fn sampler_loop(conn: Arc<Mutex<Connection>>) {
    let refresh = RefreshKind::nothing()
        .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
        .with_memory(MemoryRefreshKind::everything());
    let mut sys = System::new_with_specifics(refresh);

    // Prima lettura CPU inaffidabile (serve un intervallo tra due refresh):
    // si scalda prima di entrare nel loop.
    sys.refresh_cpu_usage();
    std::thread::sleep(Duration::from_millis(300));

    loop {
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        let cpu_pct = sys.global_cpu_usage();
        let total = sys.total_memory();
        let mem_pct = if total > 0 {
            sys.used_memory() as f32 / total as f32 * 100.0
        } else {
            0.0
        };
        let disk_pct = crate::adapters::disks::list()
            .into_iter()
            .find(|d| d.is_system)
            .map(|d| d.used_pct);

        let now = now_ms();
        if let Ok(conn) = conn.lock() {
            let _ = conn.execute(
                "INSERT OR REPLACE INTO samples (ts, cpu_pct, mem_pct, disk_pct) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![now, cpu_pct, mem_pct, disk_pct],
            );
            let _ = conn.execute(
                "DELETE FROM samples WHERE ts < ?1",
                [now.saturating_sub(RETENTION_MS)],
            );
        }

        std::thread::sleep(SAMPLE_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_e_query_su_db_in_memoria() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE samples (ts INTEGER PRIMARY KEY, cpu_pct REAL NOT NULL, mem_pct REAL NOT NULL, disk_pct REAL)",
            [],
        )
        .unwrap();
        let now = now_ms();
        conn.execute(
            "INSERT INTO samples VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![now, 12.5_f32, 40.0_f32, Some(60.0_f32)],
        )
        .unwrap();

        let service = MetricsService {
            conn: Some(Arc::new(Mutex::new(conn))),
        };
        let hist = service.history(24);
        assert_eq!(hist.len(), 1);
        assert!((hist[0].cpu_pct - 12.5).abs() < 0.01);
        assert_eq!(hist[0].disk_pct, Some(60.0));
    }

    #[test]
    fn senza_db_history_vuota() {
        let service = MetricsService { conn: None };
        assert!(service.history(24).is_empty());
    }
}
