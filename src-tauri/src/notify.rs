use crate::config::ConfigHandle;

pub fn severity_rank(severity: &str) -> u8 {
    match severity {
        "critical" => 2,
        "warning" => 1,
        _ => 0,
    }
}

fn ntfy_priority(severity: &str) -> u8 {
    match severity {
        "critical" => 4,
        "warning" => 3,
        _ => 2,
    }
}

pub fn push_alert(config: &ConfigHandle, severity: &'static str, title: &str, detail: &str) {
    let cfg = config.get();
    if !cfg.push_enabled
        || severity_rank(severity) < severity_rank(&cfg.push_min_severity)
        || cfg.push_topic.is_empty()
    {
        return;
    }
    let title = title.to_string();
    let detail = detail.to_string();
    tokio::spawn(async move {
        if let Err(e) = send(&cfg.push_server, &cfg.push_topic, severity, &title, &detail).await {
            tracing::warn!(%e, "push ntfy fallito");
        }
    });
}

pub async fn send(
    server: &str,
    topic: &str,
    severity: &str,
    title: &str,
    message: &str,
) -> Result<(), String> {
    let tag = match severity {
        "critical" => "rotating_light",
        "warning" => "warning",
        _ => "information_source",
    };
    let body = serde_json::json!({
        "topic": topic,
        "title": format!("RickyDEV: {title}"),
        "message": message,
        "priority": ntfy_priority(severity),
        "tags": [tag],
    });
    let response = reqwest::Client::new()
        .post(server.trim_end_matches('/'))
        .json(&body)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| if e.is_timeout() { "timeout".to_string() } else { e.to_string() })?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordine_severita() {
        assert!(severity_rank("critical") > severity_rank("warning"));
        assert!(severity_rank("warning") > severity_rank("info"));
        assert_eq!(severity_rank("sconosciuta"), severity_rank("info"));
    }
}
