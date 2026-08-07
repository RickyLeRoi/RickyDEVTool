use serde::Serialize;

use crate::exec;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SchedEntry {
    pub schedule: String,
    pub command: String,
    pub source: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedListing {
    pub supported: bool,
    pub entries: Vec<SchedEntry>,
    pub note: Option<String>,
}

pub async fn list() -> SchedListing {
    #[cfg(unix)]
    {
        unix_list().await
    }
    #[cfg(windows)]
    {
        windows_schtasks().await
    }
    #[cfg(not(any(unix, windows)))]
    {
        SchedListing { supported: false, entries: Vec::new(), note: Some("piattaforma non supportata".into()) }
    }
}

#[cfg(unix)]
async fn unix_list() -> SchedListing {
    let mut entries = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    match crontab_entries().await {
        Ok(mut e) => entries.append(&mut e),
        Err(n) => notes.push(n),
    }

    #[cfg(target_os = "macos")]
    {
        let mut la = launchd_entries().await;
        entries.append(&mut la);
    }

    let note = if !entries.is_empty() {
        None
    } else if !notes.is_empty() {
        Some(notes.join(" · "))
    } else {
        Some("Nessuna voce pianificata trovata".into())
    };
    SchedListing { supported: true, entries, note }
}

#[cfg(unix)]
async fn crontab_entries() -> Result<Vec<SchedEntry>, String> {
    let out = exec::cmd("crontab")
        .arg("-l")
        .output()
        .await
        .map_err(|_| "comando crontab non disponibile".to_string())?;
    if out.status.success() {
        Ok(parse_crontab(&String::from_utf8_lossy(&out.stdout)))
    } else {
        let err = String::from_utf8_lossy(&out.stderr).to_lowercase();
        if err.contains("no crontab") {
            Err("Nessun crontab per l'utente".to_string())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
}

#[cfg(unix)]
pub fn parse_crontab(text: &str) -> Vec<SchedEntry> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('@') {
            let mut it = rest.splitn(2, char::is_whitespace);
            let kw = it.next().unwrap_or("");
            let cmd = it.next().unwrap_or("").trim().to_string();
            if !cmd.is_empty() {
                out.push(SchedEntry {
                    schedule: format!("@{kw}"),
                    command: cmd,
                    source: "crontab".into(),
                    detail: None,
                });
            }
            continue;
        }
        let first = line.split_whitespace().next().unwrap_or("");
        if first.contains('=') {
            continue;
        }
        let mut in_field = false;
        let mut field_count = 0;
        let mut cmd_start = None;
        for (i, ch) in line.char_indices() {
            if ch.is_whitespace() {
                in_field = false;
            } else if !in_field {
                in_field = true;
                field_count += 1;
                if field_count == 6 {
                    cmd_start = Some(i);
                    break;
                }
            }
        }
        if let Some(start) = cmd_start {
            let schedule = line[..start].split_whitespace().collect::<Vec<_>>().join(" ");
            out.push(SchedEntry {
                schedule,
                command: line[start..].trim().to_string(),
                source: "crontab".into(),
                detail: None,
            });
        }
    }
    out
}

#[cfg(target_os = "macos")]
async fn launchd_entries() -> Vec<SchedEntry> {
    let Some(text) = exec::text(exec::cmd("launchctl").arg("list")).await else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for line in text.lines().skip(1) {
        let mut cols = line.split('\t');
        let pid = cols.next().unwrap_or("").trim();
        let status = cols.next().unwrap_or("").trim();
        let label = cols.next().unwrap_or("").trim();
        if label.is_empty() || label.starts_with("com.apple.") || label.starts_with("application.") {
            continue;
        }
        let detail = if pid == "-" {
            format!("non in esecuzione (exit {status})")
        } else {
            format!("PID {pid}")
        };
        entries.push(SchedEntry {
            schedule: "launchd".into(),
            command: label.to_string(),
            source: "launchd".into(),
            detail: Some(detail),
        });
    }
    entries.truncate(200);
    entries
}

#[cfg(windows)]
async fn windows_schtasks() -> SchedListing {
    let out = exec::cmd("schtasks")
        .args(["/query", "/fo", "CSV", "/nh"])
        .output()
        .await;
    match out {
        Ok(o) if o.status.success() => {
            let entries = parse_schtasks_csv(&String::from_utf8_lossy(&o.stdout));
            let note = entries.is_empty().then(|| "Nessuna attività pianificata".to_string());
            SchedListing { supported: true, entries, note }
        }
        Ok(o) => SchedListing {
            supported: true,
            entries: Vec::new(),
            note: Some(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        },
        Err(_) => SchedListing {
            supported: false,
            entries: Vec::new(),
            note: Some("schtasks non disponibile".into()),
        },
    }
}

#[cfg(windows)]
fn parse_schtasks_csv(text: &str) -> Vec<SchedEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let cols = split_csv(line);
        if cols.len() < 3 {
            continue;
        }
        let name = cols[0].trim();
        if name.is_empty() || name.eq_ignore_ascii_case("TaskName") {
            continue;
        }
        out.push(SchedEntry {
            schedule: cols[1].trim().to_string(),
            command: name.to_string(),
            source: "schtasks".into(),
            detail: Some(format!("stato: {}", cols[2].trim())),
        });
    }
    out.truncate(500);
    out
}

#[cfg(windows)]
fn split_csv(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                if in_quotes && chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
            }
            ',' if !in_quotes => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

pub async fn detail(source: &str, id: &str) -> Vec<String> {
    #[cfg(target_os = "macos")]
    if source == "launchd" {
        return launchd_detail(id);
    }
    #[cfg(windows)]
    if source == "schtasks" {
        return schtasks_detail(id).await;
    }
    let _ = (source, id);
    Vec::new()
}

#[cfg(target_os = "macos")]
fn launchd_detail(label: &str) -> Vec<String> {
    if !label.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
        return vec!["Identificativo non valido.".into()];
    }
    let Some(text) = read_launchd_plist(label) else {
        return vec!["Plist non trovato o non leggibile (forse un job di sistema).".into()];
    };
    let mut lines = Vec::new();
    if let Some(p) = xml_string_for_key(&text, "Program") {
        lines.push(format!("Programma: {p}"));
    } else if let Some(args) = xml_program_arguments(&text) {
        lines.push(format!("Comando: {args}"));
    }
    if let Some(sec) = xml_integer_for_key(&text, "StartInterval") {
        lines.push(format!("Esecuzione ogni {sec} secondi"));
    }
    if let Some(cal) = xml_calendar_interval(&text) {
        lines.push(format!("Pianificato: {cal}"));
    }
    if lines.is_empty() {
        lines.push("Nessuna pianificazione a tempo (probabile on-demand / KeepAlive).".into());
    }
    lines
}

#[cfg(target_os = "macos")]
fn read_launchd_plist(label: &str) -> Option<String> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Some(h) = dirs::home_dir() {
        dirs.push(h.join("Library/LaunchAgents"));
    }
    dirs.push("/Library/LaunchAgents".into());
    dirs.push("/Library/LaunchDaemons".into());
    for d in &dirs {
        if let Ok(s) = std::fs::read_to_string(d.join(format!("{label}.plist"))) {
            return Some(s);
        }
    }
    let needle = format!("<string>{label}</string>");
    for d in &dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("plist") {
                if let Ok(s) = std::fs::read_to_string(&p) {
                    if s.contains(&needle) {
                        return Some(s);
                    }
                }
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn xml_string_for_key(text: &str, key: &str) -> Option<String> {
    let pos = text.find(&format!("<key>{key}</key>"))?;
    let after = &text[pos..];
    let s = after.find("<string>")? + "<string>".len();
    let e = after[s..].find("</string>")?;
    Some(after[s..s + e].trim().to_string())
}

#[cfg(target_os = "macos")]
fn xml_integer_for_key(text: &str, key: &str) -> Option<i64> {
    let pos = text.find(&format!("<key>{key}</key>"))?;
    let after = &text[pos..];
    let s = after.find("<integer>")? + "<integer>".len();
    let e = after[s..].find("</integer>")?;
    after[s..s + e].trim().parse().ok()
}

#[cfg(target_os = "macos")]
fn xml_program_arguments(text: &str) -> Option<String> {
    let pos = text.find("<key>ProgramArguments</key>")?;
    let after = &text[pos..];
    let a = after.find("<array>")? + "<array>".len();
    let end = after[a..].find("</array>")?;
    let arr = &after[a..a + end];
    let mut parts = Vec::new();
    let mut rest = arr;
    while let Some(s) = rest.find("<string>") {
        let s2 = s + "<string>".len();
        let Some(e) = rest[s2..].find("</string>") else { break };
        parts.push(rest[s2..s2 + e].trim().to_string());
        rest = &rest[s2 + e..];
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

#[cfg(target_os = "macos")]
fn xml_calendar_interval(text: &str) -> Option<String> {
    let pos = text.find("<key>StartCalendarInterval</key>")?;
    let after = &text[pos..];
    let d = after.find("<dict>")? + "<dict>".len();
    let end = after[d..].find("</dict>")?;
    let dict = &after[d..d + end];
    let mut parts = Vec::new();
    if let Some(w) = xml_integer_for_key(dict, "Weekday") {
        parts.push(format!("giorno sett. {w}"));
    }
    if let Some(day) = xml_integer_for_key(dict, "Day") {
        parts.push(format!("giorno {day}"));
    }
    if let Some(m) = xml_integer_for_key(dict, "Month") {
        parts.push(format!("mese {m}"));
    }
    match (xml_integer_for_key(dict, "Hour"), xml_integer_for_key(dict, "Minute")) {
        (Some(h), Some(m)) => parts.push(format!("alle {h:02}:{m:02}")),
        (Some(h), None) => parts.push(format!("all'ora {h}")),
        (None, Some(m)) => parts.push(format!("al minuto {m} di ogni ora")),
        (None, None) => {}
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

#[cfg(windows)]
async fn schtasks_detail(taskname: &str) -> Vec<String> {
    let cmd = &mut exec::cmd("schtasks");
    cmd.args(["/query", "/tn", taskname, "/v", "/fo", "LIST"]);
    let Some(text) = exec::text(cmd).await else {
        return vec!["Dettagli non disponibili.".into()];
    };
    use crate::constants::SCHTASKS_DETAIL_FIELDS;

    let mut lines = Vec::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once(':') {
            let (k, v) = (k.trim(), v.trim());
            if !v.is_empty() && SCHTASKS_DETAIL_FIELDS.iter().any(|w| k.eq_ignore_ascii_case(w)) {
                lines.push(format!("{k}: {v}"));
            }
        }
    }
    if lines.is_empty() {
        lines.push("Nessun dettaglio di pianificazione.".into());
    }
    lines
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn crontab_standard_e_speciali() {
        let text = "# commento\nMAILTO=me@x.com\n*/5 * * * * /usr/bin/backup.sh --now\n@reboot /opt/app/start.sh\n";
        let e = parse_crontab(text);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].schedule, "*/5 * * * *");
        assert_eq!(e[0].command, "/usr/bin/backup.sh --now");
        assert_eq!(e[1].schedule, "@reboot");
        assert_eq!(e[1].command, "/opt/app/start.sh");
    }

    #[test]
    fn crontab_vuoto() {
        assert!(parse_crontab("# solo commenti\n\n").is_empty());
    }
}
