//! Docker awareness: rileva se Docker gira ed elenca i container, con azioni
//! base (start/stop/restart, logs in streaming). Tutto via la CLI `docker`
//! (nessun accesso diretto al socket): niente dipendenze, cross-platform, e
//! rispetta il contesto dell'utente (Docker Desktop, colima, remote host).

use serde::Serialize;

/// ID/nome container accettabile per una riga di comando: niente spazi, flag o
/// metacaratteri di shell. I nomi Docker sono `[a-zA-Z0-9][a-zA-Z0-9_.-]*`, gli
/// ID sono esadecimali; questo set li copre entrambi.
fn valid_ref(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && !s.starts_with('-')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// Host Docker remoto accettabile per `-H`: deve avere uno schema noto (così non
/// può iniziare con `-` e farsi interpretare come flag). Vuoto = daemon locale.
pub fn valid_host(s: &str) -> bool {
    const SCHEMES: &[&str] = &["tcp://", "ssh://", "unix://", "npipe://", "http://", "https://"];
    s.len() <= 255 && SCHEMES.iter().any(|scheme| s.starts_with(scheme))
}

/// Costruisce `docker [-H <host>]` pronto per aggiungere i suoi argomenti. La CLI
/// resta locale: l'host cambia solo il daemon a cui si connette (es. la VM Docker).
fn docker_cmd(host: Option<&str>) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("docker");
    if let Some(h) = host.filter(|h| valid_host(h)) {
        cmd.arg("-H").arg(h);
    }
    cmd
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    /// running | exited | paused | created | restarting | dead | …
    pub state: String,
    /// Stringa umana ("Up 3 hours", "Exited (0) 2 days ago").
    pub status: String,
    pub ports: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerState {
    pub available: bool,
    /// Presente ma il demone non risponde (Docker Desktop spento): CLI c'è,
    /// `docker ps` fallisce con "Cannot connect to the Docker daemon".
    pub daemon_down: bool,
    pub containers: Vec<Container>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

async fn docker_available() -> bool {
    #[cfg(windows)]
    let (cmd, arg) = ("where", "docker");
    #[cfg(not(windows))]
    let (cmd, arg) = ("/usr/bin/which", "docker");
    tokio::process::Command::new(cmd)
        .arg(arg)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub async fn state(host: Option<&str>) -> DockerState {
    if !docker_available().await {
        return DockerState {
            available: false,
            daemon_down: false,
            containers: Vec::new(),
            error: None,
        };
    }

    let output = docker_cmd(host)
        .args(["ps", "-a", "--no-trunc", "--format", "{{json .}}"])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let containers = text.lines().filter_map(parse_ps_line).collect();
            DockerState { available: true, daemon_down: false, containers, error: None }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).to_lowercase();
            // Varie formulazioni a seconda del runtime: Docker Desktop
            // ("cannot connect to the docker daemon"), colima/rootless
            // ("failed to connect to the docker api ... docker.sock").
            let daemon_down = stderr.contains("cannot connect to the docker daemon")
                || stderr.contains("is the docker daemon running")
                || stderr.contains("failed to connect to the docker api")
                || stderr.contains("docker.sock");
            DockerState {
                available: true,
                daemon_down,
                containers: Vec::new(),
                error: Some(String::from_utf8_lossy(&out.stderr).trim().to_string()),
            }
        }
        Err(e) => DockerState {
            available: true,
            daemon_down: false,
            containers: Vec::new(),
            error: Some(e.to_string()),
        },
    }
}

/// Parser di una riga `docker ps --format '{{json .}}'`. Testato su fixture.
pub fn parse_ps_line(line: &str) -> Option<Container> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let get = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let id = get("ID");
    if id.is_empty() {
        return None;
    }
    // "Names" può contenere più nomi separati da virgola: si tiene il primo.
    let name = get("Names");
    let name = name.split(',').next().unwrap_or(&name).trim().to_string();
    // "Ports": "0.0.0.0:8080->80/tcp, :::8080->80/tcp" → lista pulita.
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
    /// Non referenziata da nessun container (dangling `<none>` o tag non più
    /// usato): candidata al prune. Docker comunque non rimuove mai immagini in uso.
    pub unused: bool,
}

/// Riferimenti immagine (nome:tag o id) in uso dai container, da `docker ps -a`.
async fn used_image_refs(host: Option<&str>) -> std::collections::HashSet<String> {
    let output = docker_cmd(host)
        .args(["ps", "-a", "--format", "{{.Image}}"])
        .output()
        .await;
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        _ => std::collections::HashSet::new(),
    }
}

/// Immagini locali (`docker images`), con il flag `unused` calcolato incrociando
/// i riferimenti dei container. Vuoto se docker manca o il demone è giù.
pub async fn images(host: Option<&str>) -> Vec<Image> {
    if !docker_available().await {
        return Vec::new();
    }
    let output = docker_cmd(host)
        .args(["images", "--format", "{{json .}}"])
        .output()
        .await;
    let mut images: Vec<Image> = match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(parse_image_line)
            .collect(),
        _ => return Vec::new(),
    };
    let used = used_image_refs(host).await;
    let used_ids = used_image_ids(host, &used).await;
    for img in &mut images {
        img.unused = !(image_in_use_by_id(img, &used_ids) || image_in_use(img, &used));
    }
    images
}

async fn used_image_ids(
    host: Option<&str>,
    refs: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    if refs.is_empty() {
        return std::collections::HashSet::new();
    }
    let mut cmd = docker_cmd(host);
    cmd.args(["image", "inspect", "--format", "{{.Id}}"]);
    for r in refs {
        cmd.arg(r);
    }
    match cmd.output().await {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().trim_start_matches("sha256:").to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        Err(_) => std::collections::HashSet::new(),
    }
}

/// Un'immagine è in uso se il suo ID compare tra quelli risolti dai container.
fn image_in_use_by_id(img: &Image, used_ids: &std::collections::HashSet<String>) -> bool {
    let short = img.id.trim_start_matches("sha256:");
    !short.is_empty()
        && used_ids
            .iter()
            .any(|u| u.starts_with(short) || short.starts_with(u.as_str()))
}

/// Un'immagine è "in uso" se un container la referenzia per nome:tag o per id.
fn image_in_use(img: &Image, used: &std::collections::HashSet<String>) -> bool {
    if img.repository != "<none>" && img.tag != "<none>" {
        let tagref = format!("{}:{}", img.repository, img.tag);
        if used.contains(&tagref) {
            return true;
        }
        // Container avviato col tag `latest` implicito (`docker run nginx`):
        // `docker ps` mostra solo "nginx", senza ":latest".
        if img.tag == "latest" && used.contains(&img.repository) {
            return true;
        }
    }
    // Riferimento per id (es. container su immagine dangling): match anche
    // parziale sui prefissi, perché `docker images` e `docker ps` possono
    // troncare l'id a lunghezze diverse.
    used.iter().any(|u| {
        u.len() >= 12 && (img.id.starts_with(u.as_str()) || u.starts_with(img.id.as_str()))
    })
}

/// Rimuove tutte le immagini non usate da nessun container (`docker image prune -a`).
/// Docker non tocca mai immagini in uso, quindi l'operazione è sempre sicura.
pub async fn prune_images(host: Option<&str>) -> Result<String, DockerError> {
    let output = docker_cmd(host)
        .args(["image", "prune", "-a", "-f"])
        .output()
        .await
        .map_err(|e| DockerError::Failed(e.to_string()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(DockerError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

/// Parser di una riga `docker images --format '{{json .}}'`. Testato su fixture.
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
        unused: false, // calcolato in `images()` dopo aver letto i container
    })
}

#[derive(Debug)]
pub enum DockerError {
    InvalidRef,
    InvalidAction,
    Failed(String),
}

/// start | stop | restart su un container. Azione di scrittura.
pub async fn action(host: Option<&str>, id: &str, action: &str) -> Result<(), DockerError> {
    if !valid_ref(id) {
        return Err(DockerError::InvalidRef);
    }
    let verb = match action {
        "start" | "stop" | "restart" => action,
        _ => return Err(DockerError::InvalidAction),
    };
    tracing::info!(container = id, action = verb, "docker action");
    let output = docker_cmd(host)
        .args([verb, id])
        .output()
        .await
        .map_err(|e| DockerError::Failed(e.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(DockerError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

/// Comando per lo streaming dei log (ultime 200 righe + follow), avviato via
/// il task runner come traceroute/npm. Ritorna None se l'id non è valido.
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

// ---------- stats live (CPU/RAM per container) ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStat {
    pub id: String,
    pub name: String,
    /// Percentuale CPU (può superare 100% con più core).
    pub cpu_pct: f32,
    pub mem_pct: f32,
    /// Uso memoria in forma umana ("120MiB / 2GiB").
    pub mem_usage: String,
}

/// Snapshot istantaneo dell'uso risorse dei container attivi
/// (`docker stats --no-stream`). Vuoto se docker manca o il demone è giù: il
/// poller non deve andare in backoff solo perché Docker è spento.
pub async fn stats(host: Option<&str>) -> Vec<ContainerStat> {
    if !docker_available().await {
        return Vec::new();
    }
    let output = docker_cmd(host)
        .args(["stats", "--no-stream", "--no-trunc", "--format", "{{json .}}"])
        .output()
        .await;
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(parse_stat_line)
            .collect(),
        _ => Vec::new(),
    }
}

/// "12.34%" → 12.34; robusto a spazi e formati mancanti.
fn parse_pct(s: &str) -> f32 {
    s.trim().trim_end_matches('%').trim().parse::<f32>().unwrap_or(0.0)
}

/// Parser di una riga `docker stats --format '{{json .}}'`. Testato su fixture.
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
        assert_eq!(c.name, "db"); // solo il primo nome
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
    fn host_validation() {
        assert!(valid_host("tcp://192.168.1.50:2375"));
        assert!(valid_host("ssh://ricky@192.168.1.50"));
        assert!(!valid_host("192.168.1.50:2375")); // senza schema
        assert!(!valid_host("--privileged")); // niente flag travestiti da host
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
        assert!(!img.unused); // default finché images() non lo calcola
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
        // Referenziata per nome:tag → in uso.
        assert!(image_in_use(&img("nginx", "latest", "zzz999888777"), &used));
        // Dangling referenziata per id (prefisso) → in uso.
        assert!(image_in_use(&img("<none>", "<none>", "abc123def456aa"), &used));
        // Container avviato come "redis" (tag latest implicito) → l'immagine
        // "redis:latest" è in uso anche se il riferimento non ha il tag.
        assert!(image_in_use(&img("redis", "latest", "111122223333"), &used));
        // Tag esplicito diverso da quello usato → non in uso.
        assert!(!image_in_use(&img("redis", "6", "111122223333"), &used));
        // Tag non usato e id assente → non in uso.
        assert!(!image_in_use(&img("nginx", "1.0", "0000ffff1111"), &used));
        // Dangling non referenziata → non in uso (candidata al prune).
        assert!(!image_in_use(&img("<none>", "<none>", "deadbeef0000"), &used));
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
        // ID risolti dai container (full sha, senza prefisso "sha256:").
        let used_ids: std::collections::HashSet<String> = [
            "abc123def456789000000000000000000000000000000000000000000000aaaa",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        // `docker images` mostra l'ID corto: match sul prefisso → in uso.
        assert!(image_in_use_by_id(&img("abc123def456"), &used_ids));
        // Anche col prefisso "sha256:" davanti.
        assert!(image_in_use_by_id(&img("sha256:abc123def456"), &used_ids));
        // ID diverso → non in uso.
        assert!(!image_in_use_by_id(&img("ffff00001111"), &used_ids));
    }
}
