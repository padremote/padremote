//! Best-effort LAN identity for display grouping, never authentication.
use std::net::IpAddr;
use std::time::Duration;

pub(super) async fn mac_for(ip: IpAddr) -> Option<String> {
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return None;
    }
    let mut command;
    #[cfg(target_os = "macos")]
    {
        command = tokio::process::Command::new(if ip.is_ipv4() {
            "/usr/sbin/arp"
        } else {
            "/usr/sbin/ndp"
        });
        command.args(["-n", &ip.to_string()]);
    }
    #[cfg(target_os = "linux")]
    {
        command = tokio::process::Command::new("ip");
        command.args(["neigh", "show", "to", &ip.to_string()]);
    }
    #[cfg(target_os = "windows")]
    {
        if ip.is_ipv6() {
            return None;
        }
        command = tokio::process::Command::new("arp");
        command.args(["-a", &ip.to_string()]);
        use std::os::windows::process::CommandExt;
        command.as_std_mut().creation_flags(0x08000000);
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        return None;
    }
    command.kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_millis(500), command.output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse(&String::from_utf8_lossy(&output.stdout), ip)
}

fn normalize(value: &str) -> Option<String> {
    let parts: Vec<_> = value.split([':', '-']).collect();
    if parts.len() != 6 {
        return None;
    }
    let bytes: Option<Vec<u8>> = parts
        .iter()
        .map(|part| {
            if part.is_empty() || part.len() > 2 {
                None
            } else {
                u8::from_str_radix(part, 16).ok()
            }
        })
        .collect();
    let bytes = bytes?;
    if bytes.iter().all(|b| *b == 0) || bytes[0] & 1 != 0 {
        return None;
    }
    Some(
        bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":"),
    )
}

fn parse(output: &str, ip: IpAddr) -> Option<String> {
    for line in output.lines() {
        let words: Vec<_> = line.split_whitespace().collect();
        // Never use the router or another neighbour's address as this device.
        if !words.iter().any(|word| {
            word.trim_matches(['(', ')'])
                .split('%')
                .next()
                .and_then(|v| v.parse::<IpAddr>().ok())
                == Some(ip)
        }) {
            continue;
        }
        if let Some(mac) = words.iter().find_map(|word| normalize(word)) {
            return Some(mac);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_only_the_requested_neighbour() {
        let ip = "192.168.1.8".parse().unwrap();
        for row in [
            "? (192.168.1.8) at 2:a:b:c:d:e on en0 ifscope [ethernet]",
            "192.168.1.8 dev wlan0 lladdr 02:0a:0b:0c:0d:0e REACHABLE",
            "  192.168.1.8  02-0a-0b-0c-0d-0e dynamic",
        ] {
            assert_eq!(parse(row, ip).as_deref(), Some("02:0a:0b:0c:0d:0e"));
        }
        for row in [
            "? (192.168.1.1) at 02:0a:0b:0c:0d:0e on en0",
            "? (192.168.1.8) at (incomplete) on en0",
            "192.168.1.8 ff:ff:ff:ff:ff:ff",
            "192.168.1.8 00:00:00:00:00:00",
        ] {
            assert_eq!(parse(row, ip), None);
        }
        assert_eq!(
            parse(
                "fe80::123%en0 02:0a:0b:0c:0d:0e en0",
                "fe80::123".parse().unwrap()
            )
            .as_deref(),
            Some("02:0a:0b:0c:0d:0e")
        );
    }
}
