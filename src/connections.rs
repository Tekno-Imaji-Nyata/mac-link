use crate::command::{Evidence, run};
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, TcpStream, ToSocketAddrs},
    sync::mpsc,
    thread,
    time::Duration,
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Mac {
    pub name: String,
    pub host: String,
    pub user: String,
}

pub fn host(input: &str) -> Result<String, String> {
    let input = input.trim();
    let input = input
        .strip_prefix("vnc://")
        .unwrap_or(input)
        .trim_end_matches('/');
    let input = input
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(input);
    if input.parse::<IpAddr>().is_ok() {
        return Ok(input.to_lowercase());
    }
    let input = input.trim_end_matches('.');
    if input.is_empty()
        || input.len() > 253
        || !input.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.as_bytes()[0].is_ascii_alphanumeric()
                && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                && label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
    {
        return Err("Enter an IP address or hostname, without a username, port, or path.".into());
    }
    Ok(input.to_lowercase())
}

pub fn username(input: &str) -> Result<String, String> {
    let s = input.trim();
    if s.is_empty()
        || s.len() > 64
        || s.starts_with('-')
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err("Enter the remote Mac’s short account name, such as alex.".into());
    }
    Ok(s.to_owned())
}

pub fn vnc_url(input: &str) -> Result<String, String> {
    let h = host(input)?;
    Ok(if h.contains(':') {
        format!("vnc://[{h}]")
    } else {
        format!("vnc://{h}")
    })
}

pub fn ssh_command(mac: &Mac) -> Result<String, String> {
    Ok(format!("ssh {}@{}", username(&mac.user)?, host(&mac.host)?))
}

pub fn open_screen(mac: &Mac) -> Result<Evidence, String> {
    let url = vnc_url(&mac.host)?;
    // Explicitly select Apple's client, even if another app owns the vnc URL scheme.
    Ok(run(
        "/usr/bin/open",
        &["-b", "com.apple.ScreenSharing", &url],
        8,
    ))
}

pub fn open_ssh(mac: &Mac) -> Result<Evidence, String> {
    let h = host(&mac.host)?;
    let u = username(&mac.user)?;
    let h = if h.contains(':') { format!("[{h}]") } else { h };
    Ok(run(
        "/usr/bin/open",
        &["-b", "com.apple.Terminal", &format!("ssh://{u}@{h}")],
        8,
    ))
}

pub fn port(hostname: &str, port: u16) -> Result<(), String> {
    // DNS resolution gets its own deadline; an unreachable resolver cannot freeze the UI.
    let h = host(hostname)?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = (h.as_str(), port)
            .to_socket_addrs()
            .map(|a| a.take(4).collect::<Vec<_>>())
            .map_err(|e| e.to_string());
        let _ = tx.send(result);
    });
    let addresses = rx
        .recv_timeout(Duration::from_secs(4))
        .map_err(|_| "DNS lookup timed out".to_string())??;
    let mut error = "No resolved addresses".to_string();
    for addr in addresses {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
            Ok(_) => return Ok(()),
            Err(e) => error = e.to_string(),
        }
    }
    Err(error)
}

pub fn test_ports(input: &str) -> Vec<Evidence> {
    [5900,22].into_iter().map(|p| {
        let r = port(input, p);
        Evidence { command: format!("TCP connect {input}:{p}"), success: r.is_ok(), output: match r {
            Ok(_) => "VERIFIED: TCP connection accepted. Authentication and GUI access are NOT TESTED.".into(),
            Err(e) => format!("NOT TESTED: Service unreachable from this Mac ({e}). This does not prove the service is disabled."),
        }}
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_injection_and_urls_with_credentials() {
        for bad in [
            "-oProxyCommand=sh",
            "mac;id",
            "$(id)",
            "a\nb",
            "user@host",
            "host:5900",
            "https://mac",
            "vnc://a/b",
            "a?x",
            "a%20b",
            "",
            "a..b",
        ] {
            assert!(host(bad).is_err(), "accepted {bad}");
        }
        for bad in ["-root", "a;id", "a b", "x@mac", "", "$(id)"] {
            assert!(username(bad).is_err());
        }
    }
    #[test]
    fn normalize_dns_ipv4_and_ipv6() {
        assert_eq!(host(" vnc://Mini.Local/ ").unwrap(), "mini.local");
        assert_eq!(
            vnc_url("[fd7a:115c:a1e0::1]").unwrap(),
            "vnc://[fd7a:115c:a1e0::1]"
        );
        assert_eq!(host("100.64.0.10").unwrap(), "100.64.0.10");
    }
    #[test]
    fn distinguish_open_and_closed_ports() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let p = listener.local_addr().unwrap().port();
        assert!(port("127.0.0.1", p).is_ok());
        // Drain the accepted socket before closing the listener. macOS can retain
        // the listening PCB while an unaccepted connection remains queued.
        drop(listener.accept().unwrap());
        drop(listener);
        // Other tests spawn processes concurrently. A child can briefly inherit
        // descriptors between fork and exec, so wait for that bounded handoff.
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while port("127.0.0.1", p).is_ok() {
            assert!(
                std::time::Instant::now() < deadline,
                "closed port stayed reachable"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}
