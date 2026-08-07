use serde::Serialize;

use crate::constants::{
    DOCKER_AVAILABLE_TTL, DOCKER_CMD_TIMEOUT, DOCKER_HOST_MAX_LEN, DOCKER_HOST_SCHEMES,
    DOCKER_PS_TTL, DOCKER_REF_MAX_LEN,
};
use crate::exec;

fn valid_ref(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= DOCKER_REF_MAX_LEN
        && !s.starts_with('-')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

pub fn valid_host(s: &str) -> bool {
    s.len() <= DOCKER_HOST_MAX_LEN && DOCKER_HOST_SCHEMES.iter().any(|scheme| s.starts_with(scheme))
}

fn docker_cmd(host: Option<&str>) -> tokio::process::Command {
    let mut cmd = exec::cmd("docker");
    cmd.stdin(std::process::Stdio::null());
    if let Some(h) = host.filter(|h| valid_host(h)) {
        cmd.arg("-H").arg(h);
    }
    cmd
}

// 20260806 ++ RG #Security ogni invocazione è uno spawn di processo, e su host ssh:// anche un
// round-trip SSH: due cache lo tengono giù. DOCKER_PS_TTL sta appena sotto i 5s di poll della UI, così i
// pannelli container e immagini riusano la stessa lettura invece di farne una a testa, e le
// richieste che arrivano mentre una chiamata lenta è in corso si fondono in quella. Le azioni
// fatte da qui invalidano esplicitamente.

struct TtlCache<T> {
    ttl: std::time::Duration,
    entry: Option<(String, std::time::Instant, T)>,
}

impl<T: Clone> TtlCache<T> {
    const fn new(ttl: std::time::Duration) -> Self {
        Self { ttl, entry: None }
    }

    fn get(&self, key: &str, now: std::time::Instant) -> Option<T> {
        match &self.entry {
            Some((k, at, v)) if k == key && now.duration_since(*at) < self.ttl => Some(v.clone()),
            _ => None,
        }
    }

    fn put(&mut self, key: &str, value: T, now: std::time::Instant) {
        self.entry = Some((key.to_string(), now, value));
    }

    fn clear(&mut self) {
        self.entry = None;
    }
}

static AVAILABLE_CACHE: std::sync::LazyLock<tokio::sync::Mutex<TtlCache<bool>>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(TtlCache::new(DOCKER_AVAILABLE_TTL)));
static PS_CACHE: std::sync::LazyLock<tokio::sync::Mutex<TtlCache<DockerState>>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(TtlCache::new(DOCKER_PS_TTL)));

// 20260806 ++ RG #Security dopo un'azione la UI ricarica subito: il TTL non deve mostrarle
// il container ancora nello stato di prima.
pub async fn invalidate_state() {
    PS_CACHE.lock().await.clear();
}

async fn run(mut cmd: tokio::process::Command) -> Result<std::process::Output, String> {
    match tokio::time::timeout(DOCKER_CMD_TIMEOUT, cmd.output()).await {
        Ok(Ok(out)) => Ok(out),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err(format!(
            "nessuna risposta entro {}s (host irraggiungibile o in attesa di credenziali)",
            DOCKER_CMD_TIMEOUT.as_secs()
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub ports: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerState {
    pub available: bool,
    pub daemon_down: bool,
    pub containers: Vec<Container>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

async fn which_docker() -> bool {
    #[cfg(windows)]
    let (cmd, arg) = ("where", "docker");
    #[cfg(not(windows))]
    let (cmd, arg) = ("/usr/bin/which", "docker");
    exec::cmd(cmd)
        .arg(arg)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn docker_available() -> bool {
    let mut cache = AVAILABLE_CACHE.lock().await;
    if let Some(hit) = cache.get("", std::time::Instant::now()) {
        return hit;
    }
    let fresh = which_docker().await;
    cache.put("", fresh, std::time::Instant::now());
    fresh
}

// 20260806 ++ RG #Security il lock è tenuto anche durante la chiamata: chi arriva mentre un
// `docker ps` lento è in corso aspetta quello invece di aprirne un altro.
pub async fn state(host: Option<&str>) -> DockerState {
    let key = host.unwrap_or_default();
    let mut cache = PS_CACHE.lock().await;
    if let Some(hit) = cache.get(key, std::time::Instant::now()) {
        return hit;
    }
    let fresh = state_uncached(host).await;
    cache.put(key, fresh.clone(), std::time::Instant::now());
    fresh
}

async fn state_uncached(host: Option<&str>) -> DockerState {
    if !docker_available().await {
        return DockerState {
            available: false,
            daemon_down: false,
            containers: Vec::new(),
            error: None,
        };
    }

    let mut cmd = docker_cmd(host);
    cmd.args(["ps", "-a", "--no-trunc", "--format", "{{json .}}"]);

    match run(cmd).await {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let containers = text.lines().filter_map(parse_ps_line).collect();
            DockerState { available: true, daemon_down: false, containers, error: None }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).to_lowercase();
            DockerState {
                available: true,
                daemon_down: looks_like_daemon_down(&stderr),
                containers: Vec::new(),
                error: Some(clean_error(&String::from_utf8_lossy(&out.stderr))),
            }
        }
        Err(e) => DockerState {
            available: true,
            daemon_down: true,
            containers: Vec::new(),
            error: Some(e),
        },
    }
}

fn looks_like_daemon_down(stderr_lower: &str) -> bool {
    crate::constants::DOCKER_DAEMON_DOWN_MARKERS
        .iter()
        .any(|m| stderr_lower.contains(m))
}

fn clean_error(stderr: &str) -> String {
    let text = stderr.trim();
    let cut = text.find("\nRun 'docker").or_else(|| text.find("\nUsage:"));
    match cut {
        Some(i) => text[..i].trim().to_string(),
        None => text.to_string(),
    }
}

pub async fn probe(host: Option<&str>) -> Result<(), String> {
    if !docker_available().await {
        return Err("la CLI docker non è nel PATH di questo computer".to_string());
    }
    let mut cmd = docker_cmd(host);
    cmd.args(["version", "--format", "{{.Server.Version}}"]);
    let out = run(cmd).await?;
    if out.status.success() {
        Ok(())
    } else {
        Err(clean_error(&String::from_utf8_lossy(&out.stderr)))
    }
}

pub fn parse_ps_line(line: &str) -> Option<Container> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let get = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let id = get("ID");
    if id.is_empty() {
        return None;
    }
    let name = get("Names");
    let name = name.split(',').next().unwrap_or(&name).trim().to_string();
    let ports_raw = get("Ports");
    let ports = ports_raw
        .split(',')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    Some(Container {
        id,
        name,
        image: get("Image"),
        state: get("State"),
        status: get("Status"),
        ports,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub id: String,
    pub repository: String,
    pub tag: String,
    pub size: String,
    pub created: String,
    pub unused: bool,
}

// 20260806 ++ RG #Security i riferimenti immagine si ricavano dal `docker ps` che state() ha già
// fatto: era una seconda invocazione identica alla prima.
async fn used_image_refs(host: Option<&str>) -> std::collections::HashSet<String> {
    state(host)
        .await
        .containers
        .iter()
        .map(|c| c.image.trim().to_string())
        .filter(|i| !i.is_empty())
        .collect()
}

pub async fn images(host: Option<&str>) -> Vec<Image> {
    if !docker_available().await {
        return Vec::new();
    }
    let mut cmd = docker_cmd(host);
    cmd.args(["images", "--format", "{{json .}}"]);
    let mut images: Vec<Image> = match run(cmd).await {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(parse_image_line)
            .collect(),
        _ => return Vec::new(),
    };
    let used = used_image_refs(host).await;

    // 20260806 ++ RG #Security la risposta di `docker image inspect` sta già nell'elenco appena
    // scaricato: si interroga il daemon solo per ciò che non si risolve in casa (ref per digest).
    let (mut used_ids, unresolved) = resolve_refs_locally(&images, &used);
    if !unresolved.is_empty() {
        used_ids.extend(used_image_ids(host, &unresolved).await);
    }

    for img in &mut images {
        img.unused = !(image_in_use_by_id(img, &used_ids) || image_in_use(img, &used));
    }
    images
}

fn ref_matches_image(img: &Image, r: &str) -> bool {
    if img.repository != "<none>" && img.tag != "<none>" {
        if format!("{}:{}", img.repository, img.tag) == r {
            return true;
        }
        if img.tag == "latest" && img.repository == r {
            return true;
        }
    }
    let short = img.id.trim_start_matches("sha256:");
    !short.is_empty() && r.len() >= 12 && (short.starts_with(r) || r.starts_with(short))
}

fn resolve_refs_locally(
    images: &[Image],
    refs: &std::collections::HashSet<String>,
) -> (std::collections::HashSet<String>, Vec<String>) {
    let mut ids = std::collections::HashSet::new();
    let mut unresolved = Vec::new();
    for r in refs {
        match images.iter().find(|img| ref_matches_image(img, r)) {
            Some(img) => {
                ids.insert(img.id.trim_start_matches("sha256:").to_string());
            }
            None => unresolved.push(r.clone()),
        }
    }
    (ids, unresolved)
}

async fn used_image_ids(host: Option<&str>, refs: &[String]) -> std::collections::HashSet<String> {
    if refs.is_empty() {
        return std::collections::HashSet::new();
    }
    let mut cmd = docker_cmd(host);
    cmd.args(["image", "inspect", "--format", "{{.Id}}"]);
    for r in refs {
        cmd.arg(r);
    }
    match run(cmd).await {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().trim_start_matches("sha256:").to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        Err(_) => std::collections::HashSet::new(),
    }
}

fn image_in_use_by_id(img: &Image, used_ids: &std::collections::HashSet<String>) -> bool {
    let short = img.id.trim_start_matches("sha256:");
    !short.is_empty()
        && used_ids
            .iter()
            .any(|u| u.starts_with(short) || short.starts_with(u.as_str()))
}

fn image_in_use(img: &Image, used: &std::collections::HashSet<String>) -> bool {
    if img.repository != "<none>" && img.tag != "<none>" {
        let tagref = format!("{}:{}", img.repository, img.tag);
        if used.contains(&tagref) {
            return true;
        }
        if img.tag == "latest" && used.contains(&img.repository) {
            return true;
        }
    }
    used.iter().any(|u| {
        u.len() >= 12 && (img.id.starts_with(u.as_str()) || u.starts_with(img.id.as_str()))
    })
}

pub async fn prune_images(host: Option<&str>) -> Result<String, DockerError> {
    let mut cmd = docker_cmd(host);
    cmd.args(["image", "prune", "-a", "-f"]);
    let output = run(cmd).await.map_err(DockerError::Failed)?;
    invalidate_state().await;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(DockerError::Failed(clean_error(&String::from_utf8_lossy(
            &output.stderr,
        ))))
    }
}

pub fn parse_image_line(line: &str) -> Option<Image> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let get = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let id = get("ID");
    if id.is_empty() {
        return None;
    }
    Some(Image {
        id,
        repository: get("Repository"),
        tag: get("Tag"),
        size: get("Size"),
        created: get("CreatedSince"),
        unused: false,
    })
}

#[derive(Debug)]
pub enum DockerError {
    InvalidRef,
    InvalidAction,
    Failed(String),
}

pub async fn action(host: Option<&str>, id: &str, action: &str) -> Result<(), DockerError> {
    if !valid_ref(id) {
        return Err(DockerError::InvalidRef);
    }
    let verb = match action {
        "start" | "stop" | "restart" => action,
        _ => return Err(DockerError::InvalidAction),
    };
    tracing::info!(container = id, action = verb, "docker action");
    let mut cmd = docker_cmd(host);
    cmd.args([verb, id]);
    let output = run(cmd).await.map_err(DockerError::Failed)?;
    invalidate_state().await;
    if output.status.success() {
        Ok(())
    } else {
        Err(DockerError::Failed(clean_error(&String::from_utf8_lossy(
            &output.stderr,
        ))))
    }
}

pub fn logs_command(host: Option<&str>, id: &str) -> Option<(&'static str, Vec<String>)> {
    if !valid_ref(id) {
        return None;
    }
    let mut args = Vec::new();
    if let Some(h) = host.filter(|h| valid_host(h)) {
        args.push("-H".into());
        args.push(h.to_string());
    }
    args.extend(["logs".into(), "--tail".into(), "200".into(), "-f".into(), id.to_string()]);
    Some(("docker", args))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStat {
    pub id: String,
    pub name: String,
    pub cpu_pct: f32,
    pub mem_pct: f32,
    pub mem_usage: String,
}

pub async fn stats(host: Option<&str>) -> Vec<ContainerStat> {
    if !docker_available().await {
        return Vec::new();
    }
    let mut cmd = docker_cmd(host);
    cmd.args(["stats", "--no-stream", "--no-trunc", "--format", "{{json .}}"]);
    match run(cmd).await {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(parse_stat_line)
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_pct(s: &str) -> f32 {
    s.trim().trim_end_matches('%').trim().parse::<f32>().unwrap_or(0.0)
}

pub fn parse_stat_line(line: &str) -> Option<ContainerStat> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let get = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let id = get("ID");
    let name = get("Name");
    if id.is_empty() && name.is_empty() {
        return None;
    }
    Some(ContainerStat {
        id,
        name,
        cpu_pct: parse_pct(&get("CPUPerc")),
        mem_pct: parse_pct(&get("MemPerc")),
        mem_usage: get("MemUsage"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stat_riga() {
        let line = r#"{"BlockIO":"0B / 0B","CPUPerc":"1.53%","Container":"abc","ID":"abc123","MemPerc":"2.10%","MemUsage":"41.2MiB / 2GiB","Name":"web","NetIO":"1kB / 2kB","PIDs":"7"}"#;
        let s = parse_stat_line(line).expect("parse");
        assert_eq!(s.id, "abc123");
        assert_eq!(s.name, "web");
        assert!((s.cpu_pct - 1.53).abs() < 0.001);
        assert!((s.mem_pct - 2.10).abs() < 0.001);
        assert_eq!(s.mem_usage, "41.2MiB / 2GiB");
        assert!(parse_stat_line("garbage").is_none());
    }

    #[test]
    fn parse_container_running() {
        let line = r#"{"ID":"abc123","Names":"web","Image":"nginx:latest","State":"running","Status":"Up 3 hours","Ports":"0.0.0.0:8080->80/tcp, :::8080->80/tcp"}"#;
        let c = parse_ps_line(line).expect("parse");
        assert_eq!(c.id, "abc123");
        assert_eq!(c.name, "web");
        assert_eq!(c.image, "nginx:latest");
        assert_eq!(c.state, "running");
        assert_eq!(c.ports, vec!["0.0.0.0:8080->80/tcp", ":::8080->80/tcp"]);
    }

    #[test]
    fn parse_container_senza_porte() {
        let line = r#"{"ID":"def456","Names":"db,db-alias","Image":"postgres:16","State":"exited","Status":"Exited (0) 2 days ago","Ports":""}"#;
        let c = parse_ps_line(line).expect("parse");
        assert_eq!(c.name, "db");
        assert!(c.ports.is_empty());
        assert_eq!(c.state, "exited");
    }

    #[test]
    fn parse_riga_non_json() {
        assert!(parse_ps_line("non e' json").is_none());
        assert!(parse_ps_line("").is_none());
    }

    #[test]
    fn ref_validation() {
        assert!(valid_ref("abc123"));
        assert!(valid_ref("my_container-1.0"));
        assert!(!valid_ref("-rm"));
        assert!(!valid_ref("a b"));
        assert!(!valid_ref("a;rm -rf"));
        assert!(!valid_ref(""));
    }

    #[tokio::test]
    async fn action_rifiuta_ref_e_azioni_invalide() {
        assert!(matches!(action(None, "bad id", "stop").await, Err(DockerError::InvalidRef)));
        assert!(matches!(action(None, "valid", "delete").await, Err(DockerError::InvalidAction)));
    }

    #[test]
    fn logs_command_valida_ref() {
        assert!(logs_command(None, "abc123").is_some());
        assert!(logs_command(None, "-rf").is_none());
    }

    #[test]
    fn logs_command_include_host_remoto() {
        let (_, args) = logs_command(Some("ssh://ricky@192.168.1.50"), "abc123").unwrap();
        assert_eq!(&args[0], "-H");
        assert_eq!(&args[1], "ssh://ricky@192.168.1.50");
        assert!(args.contains(&"abc123".to_string()));
    }

    #[test]
    fn daemon_down_riconosce_host_remoto_irraggiungibile() {
        let ssh = "error during connect: get \"http://docker.example.com/v1.51/containers/json\": command [ssh -l ricky -- 192.168.1.248 docker system dial-stdio] has exited with exit status 255";
        assert!(looks_like_daemon_down(ssh));
        assert!(looks_like_daemon_down("ssh: connect to host 192.168.1.248 port 22: connection refused"));
        assert!(looks_like_daemon_down("cannot connect to the docker daemon at unix:///var/run/docker.sock"));
        assert!(!looks_like_daemon_down("error: no such container: web"));
    }

    #[test]
    fn errore_senza_help_della_cli() {
        let stderr = "error during connect: dial tcp 192.168.1.248:2375: connectex: no connection\n\nRun 'docker ps --help' for more information";
        assert_eq!(
            clean_error(stderr),
            "error during connect: dial tcp 192.168.1.248:2375: connectex: no connection"
        );
        assert_eq!(clean_error("  boom  "), "boom");
    }

    #[test]
    fn host_validation() {
        assert!(valid_host("tcp://192.168.1.50:2375"));
        assert!(valid_host("ssh://ricky@192.168.1.50"));
        assert!(!valid_host("192.168.1.50:2375"));
        assert!(!valid_host("--privileged"));
        assert!(!valid_host(""));
    }

    #[test]
    fn parse_image() {
        let line = r#"{"ID":"sha256:abc","Repository":"nginx","Tag":"latest","Size":"187MB","CreatedSince":"3 weeks ago","CreatedAt":"2026-06-30"}"#;
        let img = parse_image_line(line).expect("parse");
        assert_eq!(img.repository, "nginx");
        assert_eq!(img.tag, "latest");
        assert_eq!(img.size, "187MB");
        assert_eq!(img.created, "3 weeks ago");
        assert!(!img.unused);
        assert!(parse_image_line("garbage").is_none());
    }

    #[test]
    fn immagine_in_uso_per_tag_e_id() {
        let img = |repo: &str, tag: &str, id: &str| Image {
            id: id.into(),
            repository: repo.into(),
            tag: tag.into(),
            size: "1MB".into(),
            created: "now".into(),
            unused: false,
        };
        let used: std::collections::HashSet<String> =
            ["nginx:latest", "abc123def456", "redis"].iter().map(|s| s.to_string()).collect();
        assert!(image_in_use(&img("nginx", "latest", "zzz999888777"), &used));
        assert!(image_in_use(&img("<none>", "<none>", "abc123def456aa"), &used));
        assert!(image_in_use(&img("redis", "latest", "111122223333"), &used));
        assert!(!image_in_use(&img("redis", "6", "111122223333"), &used));
        assert!(!image_in_use(&img("nginx", "1.0", "0000ffff1111"), &used));
        assert!(!image_in_use(&img("<none>", "<none>", "deadbeef0000"), &used));
    }

    // 20260806 ++ RG #Security contract test: serve la CLI docker installata, non il daemon acceso.
    #[tokio::test]
    #[ignore]
    async fn state_non_rilancia_docker_ps_entro_il_ttl() {
        use std::time::Instant;

        invalidate_state().await;
        let _ = docker_available().await;

        let t = Instant::now();
        let _ = state(None).await;
        let con_spawn = t.elapsed();

        let t = Instant::now();
        let _ = state(None).await;
        let da_cache = t.elapsed();

        assert!(
            da_cache * 4 < con_spawn,
            "la seconda lettura entro il TTL non deve rilanciare docker ({con_spawn:?} contro {da_cache:?})"
        );

        invalidate_state().await;
        let t = Instant::now();
        let _ = state(None).await;
        let dopo_invalidazione = t.elapsed();
        assert!(
            dopo_invalidazione > da_cache * 4,
            "dopo un'azione si deve tornare al daemon ({dopo_invalidazione:?} contro {da_cache:?})"
        );
    }

    #[test]
    fn la_cache_scade_e_cambia_con_lhost() {
        use std::time::{Duration, Instant};

        let base = Instant::now();
        let mut cache: TtlCache<u8> = TtlCache::new(Duration::from_millis(1500));

        assert_eq!(cache.get("", base), None, "cache vuota");
        cache.put("", 7, base);

        assert_eq!(cache.get("", base + Duration::from_millis(1499)), Some(7));
        assert_eq!(cache.get("", base + Duration::from_millis(1500)), None, "scaduta al TTL");
        assert_eq!(
            cache.get("ssh://altro", base),
            None,
            "cambiare host Docker non deve riusare la voce del precedente"
        );

        cache.put("ssh://altro", 9, base);
        assert_eq!(cache.get("ssh://altro", base), Some(9));
        assert_eq!(cache.get("", base), None, "una sola voce: la nuova sostituisce la vecchia");

        cache.clear();
        assert_eq!(cache.get("ssh://altro", base), None, "invalidata dopo un'azione");
    }

    #[test]
    fn i_riferimenti_si_risolvono_senza_interrogare_il_daemon() {
        let img = |repo: &str, tag: &str, id: &str| Image {
            id: id.into(),
            repository: repo.into(),
            tag: tag.into(),
            size: "1MB".into(),
            created: "now".into(),
            unused: false,
        };
        let images = vec![
            img("nginx", "latest", "aaaa111122223333"),
            img("nginx", "1.25", "aaaa111122223333"),
            img("redis", "6", "bbbb444455556666"),
            img("<none>", "<none>", "cccc777788889999"),
        ];
        let refs: std::collections::HashSet<String> =
            ["nginx:latest", "redis:6", "cccc777788889999"]
                .iter()
                .map(|s| s.to_string())
                .collect();

        let (ids, unresolved) = resolve_refs_locally(&images, &refs);
        assert!(
            unresolved.is_empty(),
            "nel caso normale non serve nessun `docker image inspect`: {unresolved:?}"
        );
        assert!(ids.contains("aaaa111122223333"), "risolto per tag");
        assert!(ids.contains("bbbb444455556666"), "risolto per tag non-latest");
        assert!(ids.contains("cccc777788889999"), "risolto per id");

        let nudo: std::collections::HashSet<String> = ["redis".to_string()].into_iter().collect();
        let (ids, unresolved) = resolve_refs_locally(&images, &nudo);
        assert!(ids.is_empty());
        assert_eq!(unresolved, vec!["redis".to_string()]);

        let per_digest: std::collections::HashSet<String> =
            ["repo/app@sha256:0123456789abcdef".to_string()].into_iter().collect();
        let (ids, unresolved) = resolve_refs_locally(&images, &per_digest);
        assert!(ids.is_empty());
        assert_eq!(unresolved.len(), 1, "quel che non si risolve va chiesto al daemon");
    }

    #[test]
    fn risolvere_un_tag_marca_usati_tutti_i_tag_della_stessa_immagine() {
        let img = |repo: &str, tag: &str, id: &str| Image {
            id: id.into(),
            repository: repo.into(),
            tag: tag.into(),
            size: "1MB".into(),
            created: "now".into(),
            unused: false,
        };
        let images = vec![
            img("nginx", "latest", "aaaa111122223333"),
            img("nginx", "1.25", "aaaa111122223333"),
        ];
        let refs: std::collections::HashSet<String> =
            ["nginx:latest".to_string()].into_iter().collect();

        let (ids, _) = resolve_refs_locally(&images, &refs);
        assert!(image_in_use_by_id(&images[1], &ids), "l'altro tag della stessa immagine");
    }

    #[test]
    fn images_non_lancia_un_secondo_docker_ps() {
        let sorgente = std::fs::read_to_string("src/adapters/docker.rs").expect("sorgente docker");
        let inizio = sorgente.find("async fn used_image_refs").expect("used_image_refs");
        let fine = sorgente[inizio..].find("\npub async fn images").expect("fine") + inizio;
        assert!(
            !sorgente[inizio..fine].contains("\"ps\""),
            "i riferimenti immagine devono venire dal `docker ps` condiviso da state(), non da \
             una seconda invocazione identica"
        );
    }

    #[test]
    fn immagine_in_uso_per_id_risolto() {
        let img = |id: &str| Image {
            id: id.into(),
            repository: "whatever".into(),
            tag: "1".into(),
            size: "1MB".into(),
            created: "now".into(),
            unused: false,
        };
        let used_ids: std::collections::HashSet<String> = [
            "abc123def456789000000000000000000000000000000000000000000000aaaa",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert!(image_in_use_by_id(&img("abc123def456"), &used_ids));
        assert!(image_in_use_by_id(&img("sha256:abc123def456"), &used_ids));
        assert!(!image_in_use_by_id(&img("ffff00001111"), &used_ids));
    }
}
