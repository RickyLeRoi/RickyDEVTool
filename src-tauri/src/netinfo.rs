use std::net::IpAddr;

pub fn lan_ips() -> Vec<String> {
    let primary = local_ip_address::local_ip().ok();
    let mut ips: Vec<IpAddr> = Vec::new();

    if let Some(ip) = primary {
        ips.push(ip);
    }
    if let Ok(ifas) = local_ip_address::list_afinet_netifas() {
        for (_name, ip) in ifas {
            if ip.is_loopback() || ips.contains(&ip) {
                continue;
            }
            match ip {
                IpAddr::V4(v4) => {
                    if v4.is_link_local() {
                        continue;
                    }
                    ips.push(ip);
                }
                IpAddr::V6(_) => continue,
            }
        }
    }
    ips.iter().map(|ip| ip.to_string()).collect()
}
