use std::net::IpAddr;
use std::time::{Duration, Instant};

use serde::Serialize;

/// Toolbox di rete: ping, lookup DNS, port check, scan della LAN.
/// Tutte operazioni read-only. Il ping usa il binario di sistema (niente
/// raw socket → niente privilegi), parsato in modo tollerante alle lingue.

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PingResult {
    pub host: String,
    pub sent: u32,
    pub received: u32,
    pub times_ms: Vec<f64>,
    pub avg_ms: Option<f64>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsRecordSet {
    pub record_type: String,
    pub values: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortCheckResult {
    pub port: u16,
    pub open: bool,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanHost {
    pub ip: String,
    pub hostname: Option<String>,
    pub mac: Option<String>,
    pub latency_ms: Option<f64>,
    pub is_self: bool,
}

const DEFAULT_PING_COUNT: u32 = 4;

/// Consente hostname/IP "ragionevoli" ed esclude qualsiasi cosa che possa
/// essere interpretata come flag o shell injection nel comando ping.
pub(crate) fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() < 254
        && !host.starts_with('-')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_'))
}

/// `count`: la UI chiama con `count=1` più volte in sequenza per mostrare i
/// tentativi in tempo reale invece di aspettare il pacchetto di N; `None`
/// usa il default storico (chiamata singola, usata anche dai test).
pub async fn ping(host: &str, count: Option<u32>) -> PingResult {
    let count = count.unwrap_or(DEFAULT_PING_COUNT).clamp(1, 20);
    if !valid_host(host) {
        return PingResult {
            host: host.to_string(),
            sent: 0,
            received: 0,
            times_ms: Vec::new(),
            avg_ms: None,
            error: Some("host non valido".to_string()),
        };
    }
    let output = run_ping(host, count, 1000).await;
    match output {
        Ok(stdout) => {
            let times = parse_ping_times(&stdout);
            let avg = (!times.is_empty())
                .then(|| times.iter().sum::<f64>() / times.len() as f64);
            let error = times.is_empty().then(|| "nessuna risposta".to_string());
            PingResult {
                host: host.to_string(),
                sent: count,
                received: times.len() as u32,
                times_ms: times,
                avg_ms: avg.map(|a| (a * 10.0).round() / 10.0),
                error,
            }
        }
        Err(e) => PingResult {
            host: host.to_string(),
            sent: count,
            received: 0,
            times_ms: Vec::new(),
            avg_ms: None,
            error: Some(e),
        },
    }
}

async fn run_ping(host: &str, count: u32, per_packet_timeout_ms: u64) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new("ping");
    #[cfg(target_os = "windows")]
    cmd.args(["-n", &count.to_string(), "-w", &per_packet_timeout_ms.to_string()]);
    #[cfg(target_os = "macos")]
    cmd.args(["-c", &count.to_string(), "-W", &per_packet_timeout_ms.to_string()]);
    #[cfg(all(unix, not(target_os = "macos")))]
    cmd.args(["-c", &count.to_string(), "-W", &(per_packet_timeout_ms / 1000).max(1).to_string()]);
    cmd.arg(host);
    cmd.stdin(std::process::Stdio::null());

    let total_timeout = Duration::from_millis(2000 + count as u64 * (per_packet_timeout_ms + 1100));
    let output = tokio::time::timeout(total_timeout, cmd.output())
        .await
        .map_err(|_| "timeout".to_string())?
        .map_err(|e| format!("ping non eseguibile: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if stdout.trim().is_empty() && !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(stdout)
}

/// Estrae i tempi dalle righe di risposta. Cerca `time=`, `time<`, `tempo=`,
/// `tempo<` e varianti: regge l'output localizzato di Windows.
pub fn parse_ping_times(output: &str) -> Vec<f64> {
    let mut times = Vec::new();
    for line in output.lines() {
        let lower = line.to_lowercase();
        let Some(idx) = ["time=", "time<", "tempo=", "tempo<", "durata=", "durata<", "zeit="]
            .iter()
            .find_map(|marker| lower.find(marker).map(|i| i + marker.len()))
        else {
            continue;
        };
        let rest = &lower[idx..];
        let number: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
            .collect();
        if let Ok(value) = number.replace(',', ".").parse::<f64>() {
            times.push(value);
        }
    }
    times
}

pub async fn dns_lookup(name: &str) -> Result<Vec<DnsRecordSet>, String> {
    use hickory_resolver::config::{ResolverConfig, ResolverOpts};
    use hickory_resolver::proto::rr::RecordType;
    use hickory_resolver::TokioAsyncResolver;

    if !valid_host(name) {
        return Err("nome non valido".to_string());
    }
    // Resolver di sistema quando possibile, altrimenti Cloudflare.
    let resolver = TokioAsyncResolver::tokio_from_system_conf()
        .unwrap_or_else(|_| {
            TokioAsyncResolver::tokio(ResolverConfig::cloudflare(), ResolverOpts::default())
        });

    let types = [
        RecordType::A,
        RecordType::AAAA,
        RecordType::CNAME,
        RecordType::MX,
        RecordType::TXT,
        RecordType::NS,
        RecordType::SOA,
        RecordType::CAA,
        RecordType::SRV,
        RecordType::TLSA,
    ];
    let mut sets = Vec::new();
    for rtype in types {
        let result = tokio::time::timeout(
            Duration::from_secs(4),
            resolver.lookup(name, rtype),
        )
        .await;
        let values: Vec<String> = match result {
            Ok(Ok(lookup)) => lookup
                .iter()
                // Solo i record del tipo richiesto: il resolver può restituire
                // anche la catena CNAME dentro le risposte A/AAAA.
                .filter(|r| r.record_type() == rtype)
                .map(|r| r.to_string())
                .collect(),
            _ => Vec::new(),
        };
        if !values.is_empty() {
            sets.push(DnsRecordSet {
                record_type: rtype.to_string(),
                values,
            });
        }
    }
    if sets.is_empty() {
        return Err("nessun record trovato (nome inesistente o DNS non raggiungibile)".to_string());
    }
    Ok(sets)
}

/// Limite per chiamata: la UI spezza scansioni grandi (tutte le porte, porte
/// note) in batch di questa dimensione, così può mostrare un progresso reale
/// invece di aspettare un'unica risposta enorme.
pub const MAX_PORTS_PER_CALL: usize = 1000;
/// Connessioni TCP in volo contemporaneamente per batch: evita un burst di
/// migliaia di connect() simultanee quando la UI chiede "tutte le porte".
const MAX_CONCURRENT_PORT_CHECKS: usize = 200;

pub async fn check_ports(host: &str, ports: &[u16]) -> Result<Vec<PortCheckResult>, String> {
    if !valid_host(host) {
        return Err("host non valido".to_string());
    }
    if ports.is_empty() || ports.len() > MAX_PORTS_PER_CALL {
        return Err(format!("indica da 1 a {MAX_PORTS_PER_CALL} porte"));
    }
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_PORT_CHECKS));
    let mut join_set = tokio::task::JoinSet::new();
    for &port in ports {
        let host = host.to_string();
        let semaphore = semaphore.clone();
        join_set.spawn(async move {
            let _permit = semaphore.acquire_owned().await.ok();
            let started = Instant::now();
            let attempt = tokio::time::timeout(
                Duration::from_millis(1500),
                tokio::net::TcpStream::connect((host.as_str(), port)),
            )
            .await;
            match attempt {
                Ok(Ok(_)) => PortCheckResult {
                    port,
                    open: true,
                    latency_ms: Some(started.elapsed().as_millis() as u64),
                    error: None,
                },
                Ok(Err(e)) => PortCheckResult {
                    port,
                    open: false,
                    latency_ms: None,
                    error: Some(e.to_string()),
                },
                Err(_) => PortCheckResult {
                    port,
                    open: false,
                    latency_ms: None,
                    error: Some("timeout".to_string()),
                },
            }
        });
    }
    let mut results = Vec::new();
    while let Some(res) = join_set.join_next().await {
        if let Ok(r) = res {
            results.push(r);
        }
    }
    results.sort_by_key(|r| r.port);
    Ok(results)
}

/// Comando/argomenti per il traceroute di sistema (mac/linux `traceroute`,
/// Windows `tracert`). Il reverse DNS per hop è disattivo di default (`-n` /
/// `-d`): risolvere ogni hop può rallentare parecchio la traccia, quindi la
/// UI lo lascia disattivo a meno che l'utente non lo riattivi esplicitamente.
/// L'output va in streaming come gli altri task (vedi `tasks.rs`): niente
/// parser custom, la UI mostra le righe grezze via `TaskLog`.
pub fn traceroute_command(host: &str, resolve_hostnames: bool) -> (&'static str, Vec<String>) {
    if cfg!(windows) {
        let mut args = if resolve_hostnames { vec![] } else { vec!["-d".to_string()] };
        args.push(host.to_string());
        ("tracert", args)
    } else {
        let mut args = if resolve_hostnames { vec![] } else { vec!["-n".to_string()] };
        args.push(host.to_string());
        ("traceroute", args)
    }
}

/// Scan del /24 dell'IP LAN primario: ping sweep (1 pacchetto, 500ms)
/// con concorrenza limitata, poi hostname (reverse DNS) e MAC (tabella ARP).
pub async fn scan_lan() -> Result<Vec<LanHost>, String> {
    let my_ip = crate::netinfo::lan_ips()
        .into_iter()
        .next()
        .ok_or("nessun IP LAN rilevato")?;
    let base: Vec<&str> = my_ip.split('.').collect();
    if base.len() != 4 {
        return Err(format!("IP non IPv4: {my_ip}"));
    }
    let prefix = format!("{}.{}.{}", base[0], base[1], base[2]);

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(60));
    let mut join_set = tokio::task::JoinSet::new();
    for last in 1..=254u8 {
        let ip = format!("{prefix}.{last}");
        let semaphore = semaphore.clone();
        join_set.spawn(async move {
            let _permit = semaphore.acquire().await.ok()?;
            let output = run_ping(&ip, 1, 500).await.ok()?;
            let times = parse_ping_times(&output);
            times.first().copied().map(|t| (ip, t))
        });
    }
    let mut alive: Vec<(String, f64)> = Vec::new();
    while let Some(res) = join_set.join_next().await {
        if let Ok(Some(hit)) = res {
            alive.push(hit);
        }
    }

    let arp = arp_table().await;
    let mut hosts = Vec::new();
    for (ip, latency) in alive {
        let hostname = reverse_dns(&ip).await;
        hosts.push(LanHost {
            mac: arp.get(&ip).cloned(),
            hostname,
            is_self: ip == my_ip,
            latency_ms: Some(latency),
            ip,
        });
    }
    hosts.sort_by_key(|h| {
        h.ip.parse::<std::net::Ipv4Addr>()
            .map(u32::from)
            .unwrap_or(u32::MAX)
    });
    Ok(hosts)
}

async fn reverse_dns(ip: &str) -> Option<String> {
    use hickory_resolver::TokioAsyncResolver;
    let addr: IpAddr = ip.parse().ok()?;
    let resolver = TokioAsyncResolver::tokio_from_system_conf().ok()?;
    let response = tokio::time::timeout(Duration::from_millis(800), resolver.reverse_lookup(addr))
        .await
        .ok()?
        .ok()?;
    response
        .iter()
        .next()
        .map(|name| name.to_string().trim_end_matches('.').to_string())
}

/// MAC address dalla tabella ARP di sistema (popolata dal ping sweep appena fatto).
async fn arp_table() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let Ok(output) = tokio::process::Command::new("arp").arg("-a").output().await else {
        return map;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if let Some((ip, mac)) = parse_arp_line(line) {
            map.insert(ip, mac);
        }
    }
    map
}

/// mac/linux: `? (192.168.1.5) at aa:bb:cc:dd:ee:ff on en0 ...`
/// windows:   `  192.168.1.5        aa-bb-cc-dd-ee-ff     dinamico`
pub fn parse_arp_line(line: &str) -> Option<(String, String)> {
    let ip = if let (Some(open), Some(close)) = (line.find('('), line.find(')')) {
        line.get(open + 1..close)?.to_string()
    } else {
        line.split_whitespace().next()?.to_string()
    };
    if ip.parse::<std::net::Ipv4Addr>().is_err() {
        return None;
    }
    let mac = line.split_whitespace().find(|token| {
        let sep = if token.contains(':') { ':' } else { '-' };
        let parts: Vec<&str> = token.split(sep).collect();
        parts.len() == 6 && parts.iter().all(|p| !p.is_empty() && p.len() <= 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
    })?;
    let incomplete = mac.contains("incomplet");
    (!incomplete).then(|| (ip, mac.to_lowercase().replace('-', ":")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ping_mac_e_windows() {
        let mac = "64 bytes from 1.1.1.1: icmp_seq=0 ttl=57 time=12.3 ms\n64 bytes from 1.1.1.1: icmp_seq=1 ttl=57 time=11.0 ms\n";
        assert_eq!(parse_ping_times(mac), vec![12.3, 11.0]);
        let win_it = "Risposta da 1.1.1.1: byte=32 durata=15ms TTL=57\nRisposta da 1.1.1.1: byte=32 tempo=14ms TTL=57\nRisposta da 1.1.1.1: byte=32 tempo<1ms TTL=57\n";
        assert_eq!(parse_ping_times(win_it), vec![15.0, 14.0, 1.0]);
        let win_en = "Reply from 1.1.1.1: bytes=32 time=8ms TTL=57\n";
        assert_eq!(parse_ping_times(win_en), vec![8.0]);
    }

    #[test]
    fn parse_arp() {
        assert_eq!(
            parse_arp_line("? (192.168.1.5) at aa:bb:cc:dd:ee:ff on en0 ifscope [ethernet]"),
            Some(("192.168.1.5".into(), "aa:bb:cc:dd:ee:ff".into()))
        );
        assert_eq!(
            parse_arp_line("  192.168.1.7          aa-bb-cc-dd-ee-0f     dinamico"),
            Some(("192.168.1.7".into(), "aa:bb:cc:dd:ee:0f".into()))
        );
        assert_eq!(parse_arp_line("? (192.168.1.9) at (incomplete) on en0"), None);
        assert_eq!(parse_arp_line("Interface: 192.168.1.2 --- 0x4"), None);
    }

    #[test]
    fn host_validation() {
        assert!(valid_host("google.com"));
        assert!(valid_host("192.168.1.1"));
        assert!(valid_host("fe80::1"));
        assert!(!valid_host("-c"));
        assert!(!valid_host("a b"));
        assert!(!valid_host("a;rm"));
        assert!(!valid_host(""));
    }

    #[tokio::test]
    async fn port_check_locale() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let results = check_ports("127.0.0.1", &[port]).await.expect("check");
        assert!(results[0].open);
    }
}
