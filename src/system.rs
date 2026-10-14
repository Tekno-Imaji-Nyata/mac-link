use crate::{
    command::{Evidence, apple_string, run},
    connections,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum State {
    Verified,
    Configured,
    Action,
    NotTested,
    Unsupported,
}
impl State {
    pub fn label(self) -> &'static str {
        match self {
            Self::Verified => "VERIFIED",
            Self::Configured => "CONFIGURED BUT NOT TESTED",
            Self::Action => "REQUIRES USER ACTION",
            Self::NotTested => "NOT TESTED",
            Self::Unsupported => "UNSUPPORTED",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub state: State,
    pub detail: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Peer {
    pub name: String,
    pub host: String,
    pub ip: String,
    pub online: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Tailnet {
    pub binary: Option<String>,
    pub state: String,
    pub ip: String,
    pub dns: String,
    pub identity: String,
    pub magic_dns: bool,
    pub peers: Vec<Peer>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub timestamp: u64,
    pub name: String,
    pub local_name: String,
    pub os: String,
    pub build: String,
    pub model: String,
    pub chip: String,
    pub arch: String,
    pub user: String,
    pub lan: Vec<(String, String)>,
    pub tailnet: Tailnet,
    pub power: BTreeMap<String, u32>,
    pub power_caps: Vec<String>,
    pub checks: Vec<Check>,
    pub evidence: Vec<Evidence>,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn tailscale_binary() -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    [
        "/Applications/Tailscale.app/Contents/MacOS/Tailscale".into(),
        format!("{home}/Applications/Tailscale.app/Contents/MacOS/Tailscale"),
        "/opt/homebrew/bin/tailscale".into(),
        "/usr/local/bin/tailscale".into(),
    ]
    .into_iter()
    .find(|p: &String| Path::new(p).is_file())
}
pub fn parse_tailnet(text: &str) -> Result<Tailnet, String> {
    let d: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let string = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    let ipv4 = |v: &Value| {
        v.as_array()
            .and_then(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .find(|s| s.parse::<std::net::Ipv4Addr>().is_ok())
            })
            .unwrap_or_default()
            .to_owned()
    };
    let mut peers = Vec::new();
    if let Some(map) = d["Peer"].as_object() {
        for p in map.values().filter(|p| {
            p["OS"]
                .as_str()
                .is_some_and(|os| os.eq_ignore_ascii_case("macos"))
        }) {
            let ip = ipv4(&p["TailscaleIPs"]);
            let dns = string(&p["DNSName"]).trim_end_matches('.').to_owned();
            peers.push(Peer {
                name: string(&p["HostName"]),
                host: if ip.is_empty() { dns } else { ip.clone() },
                ip,
                online: p["Online"].as_bool().unwrap_or(false),
            });
        }
    }
    peers.sort_by(|a, b| b.online.cmp(&a.online).then(a.name.cmp(&b.name)));
    Ok(Tailnet {
        binary: None,
        state: string(&d["BackendState"]),
        ip: ipv4(&d["TailscaleIPs"]),
        dns: string(&d["Self"]["DNSName"])
            .trim_end_matches('.')
            .to_owned(),
        identity: string(&d["CurrentTailnet"]["Name"]),
        magic_dns: d["CurrentTailnet"]["MagicDNSEnabled"]
            .as_bool()
            .unwrap_or(false),
        peers,
    })
}
pub fn parse_power(text: &str) -> BTreeMap<String, u32> {
    let mut ac = false;
    let mut result = BTreeMap::new();
    for line in text.lines() {
        if line.trim_end().ends_with(':') {
            ac = line.trim() == "AC Power:";
            continue;
        }
        if ac {
            let mut words = line.split_whitespace();
            if let (Some(key), Some(value)) = (words.next(), words.next())
                && let Ok(value) = value.parse()
            {
                result.insert(key.to_string(), value);
            }
        }
    }
    result
}
pub fn parse_lan(text: &str) -> Vec<(String, String)> {
    let mut interface = "";
    let mut result = Vec::new();
    for line in text.lines() {
        if !line.starts_with(char::is_whitespace) {
            interface = line.split(':').next().unwrap_or_default();
        }
        let words: Vec<_> = line.split_whitespace().collect();
        if words.first() == Some(&"inet")
            && (interface.starts_with("en") || interface.starts_with("bridge"))
            && let Some(ip) = words.get(1)
        {
            result.push((interface.to_string(), ip.to_string()));
        }
    }
    result
}
pub fn funnel_enabled(v: &Value) -> bool {
    v.get("AllowFunnel")
        .and_then(Value::as_object)
        .is_some_and(|m| m.values().any(|v| v.as_bool() == Some(true)))
}

impl Snapshot {
    fn check(&mut self, name: &str, state: State, detail: impl Into<String>) {
        self.checks.push(Check {
            name: name.into(),
            state,
            detail: detail.into(),
        });
    }
    fn inspect(&mut self, program: &str, args: &[&str]) -> Evidence {
        let result = run(program, args, 8);
        self.evidence.push(result.clone());
        result
    }
    pub fn discover() -> Self {
        let mut s = Self {
            timestamp: now(),
            ..Self::default()
        };
        s.os = s.inspect("/usr/bin/sw_vers", &["-productVersion"]).output;
        s.build = s.inspect("/usr/bin/sw_vers", &["-buildVersion"]).output;
        s.arch = s.inspect("/usr/bin/uname", &["-m"]).output;
        s.model = s.inspect("/usr/sbin/sysctl", &["-n", "hw.model"]).output;
        s.chip = s
            .inspect("/usr/sbin/sysctl", &["-n", "machdep.cpu.brand_string"])
            .output;
        s.name = s
            .inspect("/usr/sbin/scutil", &["--get", "ComputerName"])
            .output;
        s.local_name = s
            .inspect("/usr/sbin/scutil", &["--get", "LocalHostName"])
            .output;
        s.user = s.inspect("/usr/bin/whoami", &[]).output;
        s.inspect("/usr/bin/id", &[]);
        s.inspect("/usr/bin/who", &[]);
        s.inspect("/usr/bin/stat", &["-f", "%Su", "/dev/console"]);
        s.inspect("/usr/sbin/networksetup", &["-listallhardwareports"]);
        s.lan = parse_lan(&s.inspect("/sbin/ifconfig", &[]).output);
        s.power = parse_power(&s.inspect("/usr/bin/pmset", &["-g", "custom"]).output);
        s.power_caps = s
            .inspect("/usr/bin/pmset", &["-g", "cap"])
            .output
            .lines()
            .map(str::trim)
            .filter(|l| !l.contains(':'))
            .map(str::to_string)
            .collect();
        s.inspect("/bin/launchctl", &["print-disabled", "system"]);
        for (service, label) in [
            ("com.apple.screensharing", "Screen Sharing service"),
            ("com.openssh.sshd", "SSH service"),
        ] {
            let r = s.inspect("/bin/launchctl", &["print", &format!("system/{service}")]);
            s.check(label, if r.success {State::Configured} else {State::Action},
                if r.success {"launchd job is loaded (it may activate on demand). This does not verify access permissions or authentication."}
                else {"The launchd job could not be inspected or is not loaded. Check System Settings → General → Sharing."});
        }
        let remote = s.inspect(
            "/usr/bin/defaults",
            &[
                "read",
                "/Library/Preferences/com.apple.RemoteManagement",
                "ARD_AllLocalUsers",
            ],
        );
        s.inspect(
            "/usr/bin/pgrep",
            &["-lf", "ARDAgent|screensharingd|sshd|tailscaled"],
        );
        s.check("Remote Management", State::Action, format!("Inspect its toggle in Sharing before enabling Screen Sharing; never enable both. ARD preference observation: {}", remote.output));
        for (p, label) in [(5900, "Screen Sharing TCP 5900"), (22, "SSH TCP 22")] {
            let r = connections::port("127.0.0.1", p);
            s.evidence.push(Evidence {
                command: format!("TCP connect 127.0.0.1:{p}"),
                success: r.is_ok(),
                output: format!("{r:?}"),
            });
            s.inspect(
                "/usr/sbin/lsof",
                &["-nP", &format!("-iTCP:{p}"), "-sTCP:LISTEN"],
            );
            s.check(label, if r.is_ok() {State::Verified} else {State::Action},
                if r.is_ok() {"Local TCP connection accepted. LAN/Tailscale reachability and authentication need separate tests."}
                else {"No connection on loopback. Enable the service in Sharing, then refresh. Non-admin lsof may omit system-owned listeners."});
        }
        for (group, label) in [
            (
                "com.apple.access_screensharing",
                "Screen Sharing authorized users",
            ),
            ("com.apple.access_ssh", "SSH authorized users"),
        ] {
            let r = s.inspect(
                "/usr/bin/dscl",
                &[
                    ".",
                    "-read",
                    &format!("/Groups/{group}"),
                    "GroupMembership",
                    "NestedGroups",
                ],
            );
            let direct_members = r
                .output
                .lines()
                .any(|l| l.starts_with("GroupMembership:") && l.split_whitespace().count() > 1);
            let nested_members = r
                .output
                .lines()
                .any(|l| l.starts_with("NestedGroups:") && l.split_whitespace().count() > 1);
            s.check(label,if r.success && direct_members && !nested_members {State::Configured} else {State::Action},format!("{}\nConfirm ‘Only these users’ in Sharing and add only the designated account ({}). Nested groups may include more users; group presence alone does not prove effective restrictions or authentication.",r.output,s.user));
        }
        let legacy = s.inspect(
            "/usr/bin/defaults",
            &[
                "read",
                "/Library/Preferences/com.apple.RemoteManagement",
                "VNCLegacyConnectionsEnabled",
            ],
        );
        s.check("Legacy VNC authentication",State::Action,if legacy.success && legacy.output == "1" {
            "A legacy VNC preference is enabled. Turn off ‘VNC viewers may control screen with password’ in Sharing.".into()
        } else {"Confirm ‘VNC viewers may control screen with password’ and ‘Anyone may request permission’ are off in Screen Sharing. An absent preference is not proof.".to_owned()});
        s.inspect("/usr/sbin/systemsetup", &["-getremotelogin"]);
        let keys = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|p| p.join(".ssh/authorized_keys"));
        let key_status = match keys.as_ref().map(std::fs::read_to_string) {
            Some(Ok(text)) => format!(
                "{} non-comment entries observed in this account’s authorized_keys; no key content is logged. Other AuthorizedKeysFile paths and commands are not inspected.",
                text.lines()
                    .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
                    .count()
            ),
            Some(Err(e)) => format!(
                "Default authorized_keys could not be read: {e}. Custom key locations are unverified."
            ),
            None => "Home directory unavailable.".into(),
        };
        s.check("SSH keys & authentication",State::NotTested,format!("{key_status} Keys and sshd settings are preserved. Test a key login before considering password changes."));
        let firewall = s.inspect(
            "/usr/libexec/ApplicationFirewall/socketfilterfw",
            &["--getglobalstate"],
        );
        s.check(
            "macOS firewall",
            if firewall.success && firewall.output.contains("State = 1") {
                State::Verified
            } else {
                State::Action
            },
            firewall.output,
        );
        let vault = s.inspect("/usr/bin/fdesetup", &["status"]);
        s.check(
            "FileVault (unchanged)",
            if vault.success {
                State::Verified
            } else {
                State::NotTested
            },
            vault.output,
        );
        if let Some(binary) = tailscale_binary() {
            let mut status = run(&binary, &["status", "--json"], 10);
            if status.success {
                match parse_tailnet(&status.output) {
                    Ok(t) => {
                        s.tailnet = t;
                        status.output = format!(
                            "State: {}; IPv4: {}; DNS: {}; Tailnet: {}; Mac peers: {}. Raw identity/key data omitted.",
                            s.tailnet.state,
                            s.tailnet.ip,
                            s.tailnet.dns,
                            s.tailnet.identity,
                            s.tailnet.peers.len()
                        );
                    }
                    Err(_) => {
                        status.success = false;
                        status.output =
                            "Unrecognized Tailscale status response; raw response omitted.".into();
                    }
                }
            } else {
                status.output = "Tailscale status failed. Open the existing application to check login. Raw output omitted to protect authentication links.".into();
            }
            s.evidence.push(status);
            s.tailnet.binary = Some(binary.clone());
            let ip = s.inspect(&binary, &["ip", "-4"]);
            s.check("Tailscale identity",if s.tailnet.state == "Running" && !s.tailnet.ip.is_empty() && ip.success && ip.output.lines().any(|i| i == s.tailnet.ip) {State::Verified} else {State::Action},
                format!("State: {} · IPv4: {} · Tailnet: {}. A control-plane connection does not prove peer connectivity.",s.tailnet.state,s.tailnet.ip,s.tailnet.identity));
            let mut funnel = run(&binary, &["funnel", "status", "--json"], 8);
            let parsed = if funnel.success {
                serde_json::from_str::<Value>(&funnel.output)
                    .ok()
                    .filter(Value::is_object)
            } else {
                None
            };
            s.check("Tailscale Funnel",match &parsed {Some(v) if !funnel_enabled(v) => State::Verified,Some(_) => State::Action,None=>State::NotTested},
                match &parsed {Some(v) if funnel_enabled(v) => "Existing public Funnel exposure detected. Review it; Mac Link does not remove unrelated configuration.",Some(_)=>"No enabled AllowFunnel entries in the observed configuration. Existing private Serve configuration is preserved.",None=>"Funnel status could not be verified. Review the existing installation manually."});
            funnel.output = parsed
                .map(|v| {
                    format!(
                        "Public Funnel enabled: {}. Private proxy details omitted.",
                        funnel_enabled(&v)
                    )
                })
                .unwrap_or_else(|| "Funnel status unavailable; raw response omitted.".into());
            s.evidence.push(funnel);
        } else {
            s.check("Tailscale identity",State::Action,"Tailscale CLI was not found. Open the installed app or install the official macOS app. Reuse its existing identity; do not install a second variant.");
        }
        s.check("AC power",if s.power.get("sleep") == Some(&0) {State::Verified} else {State::Action},
            format!("AC sleep: {:?} minutes · Wake on network: {:?} · Display sleep: {:?} minutes. Keeping the system awake consumes more power; display sleep is preserved.",s.power.get("sleep"),s.power.get("womp"),s.power.get("displaysleep")));
        s.inspect(
            "/usr/sbin/system_profiler",
            &["SPDisplaysDataType", "-detailLevel", "mini"],
        );
        let capable = s.arch == "arm64"
            && s.os
                .split('.')
                .next()
                .and_then(|x| x.parse::<u32>().ok())
                .is_some_and(|x| x >= 14);
        s.check("High Performance & virtual display",if capable {State::NotTested} else {State::Unsupported},
            if capable {"This host meets the basic Apple silicon / macOS 14+ requirements. Client hardware, network quality, UDP 5900–5902, and a real High Performance session remain unverified. Prefer wired LAN; fall back to Standard if unstable."}
            else {"This host does not meet the observed Apple silicon / macOS 14+ requirements. Use Standard Screen Sharing."});
        s.check("Headless desktop",State::NotTested,"With a trusted client connected, safely disconnect the host monitor and reconnect. Keep local recovery available. No dummy plug or virtual display software is required without testing.");
        s.check("Public network exposure",State::NotTested,"Mac Link creates no port forwarding, firewall rules, public listeners, routes, or Funnel configuration. Existing router rules, global IPv6 reachability and tailnet ACLs require separate review. Limit tailnet access to trusted devices; no public SSH/VNC forwarding is needed.");
        s.check("Authenticated desktop access",State::NotTested,"From another Mac, open the LAN and Tailscale VNC addresses and sign in with the authorized macOS account. A listening port is not proof of a usable desktop.");
        s
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PowerBackup {
    pub previous: BTreeMap<String, u32>,
    pub applied: BTreeMap<String, u32>,
    pub timestamp: u64,
}

pub fn power_plan(s: &Snapshot) -> Result<PowerBackup, String> {
    if !s.power.contains_key("sleep") || !s.power_caps.iter().any(|x| x == "sleep") {
        return Err("AC system-sleep capability is not confirmed on this Mac.".into());
    }
    // Exclude laptops even when plugged in; this action is intended for a desktop host.
    let hardware = run(
        "/usr/sbin/system_profiler",
        &["SPHardwareDataType", "-json"],
        8,
    );
    let data: Value =
        serde_json::from_str(&hardware.output).map_err(|_| "Cannot identify desktop hardware")?;
    let model = data["SPHardwareDataType"][0]["machine_name"]
        .as_str()
        .unwrap_or_default();
    if !["Mac mini", "Mac Studio", "Mac Pro", "iMac", "iMac Pro"].contains(&model) {
        return Err("Automatic power changes are limited to identified desktop Macs. Use System Settings on this model.".into());
    }
    let mut previous = BTreeMap::new();
    let mut applied = BTreeMap::new();
    for (key, target) in [("sleep", 0), ("womp", 1)] {
        if s.power_caps.iter().any(|x| x == key)
            && let Some(old) = s.power.get(key)
            && *old != target
        {
            previous.insert(key.into(), *old);
            applied.insert(key.into(), target);
        }
    }
    Ok(PowerBackup {
        previous,
        applied,
        timestamp: now(),
    })
}
pub fn apply_power(values: &BTreeMap<String, u32>) -> Result<Evidence, String> {
    if values.is_empty() {
        return Err("The selected AC settings are already applied.".into());
    }
    if values.keys().any(|k| k != "sleep" && k != "womp") {
        return Err("Unsupported power setting".into());
    }
    let before = run("/usr/bin/pmset", &["-g", "custom"], 8);
    let current = parse_power(&before.output);
    if before.success && values.iter().all(|(k, v)| current.get(k) == Some(v)) {
        return Ok(Evidence {
            command: before.command,
            output:
                "VERIFIED: requested AC values are already set; no change or authorization needed."
                    .into(),
            success: true,
        });
    }
    let command = format!(
        "/usr/bin/pmset -c {}",
        values
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let script = format!(
        "do shell script {} with administrator privileges",
        apple_string(&command)
    );
    let mut result = run("/usr/bin/osascript", &["-e", &script], 300);
    result.command = format!("Administrator authorization: {command}");
    let after = run("/usr/bin/pmset", &["-g", "custom"], 8);
    let actual = parse_power(&after.output);
    result.success =
        result.success && after.success && values.iter().all(|(k, v)| actual.get(k) == Some(v));
    result.output = format!(
        "{}\nRead-back verification: {}\n{}",
        result.output, result.success, after.output
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_only_ac_power_and_lan() {
        let p = parse_power(
            "Battery Power:\n sleep 5\nAC Power:\n sleep 0\n displaysleep 10\n womp 1\nUPS Power:\n sleep 3\n",
        );
        assert_eq!(p["sleep"], 0);
        assert_eq!(p["displaysleep"], 10);
        assert_eq!(
            parse_lan(
                "lo0: flags\n inet 127.0.0.1 netmask x\nen0: flags\n inet 192.168.1.3 netmask x\nutun0: flags\n inet 100.64.0.1 netmask x"
            ),
            vec![("en0".into(), "192.168.1.3".into())]
        );
    }
    #[test]
    fn tolerate_null_offline_tailscale_and_omit_identity_keys() {
        let t = parse_tailnet(r#"{"BackendState":"NeedsLogin","Peer":null,"Self":null}"#).unwrap();
        assert_eq!(t.state, "NeedsLogin");
        assert!(t.peers.is_empty());
        let t = parse_tailnet(r#"{"BackendState":"Running","TailscaleIPs":["fd00::1","100.64.0.3"],"Self":{"DNSName":"mini.tail.ts.net."},"Peer":{"one":{"OS":"macOS","HostName":"Mini","TailscaleIPs":["100.64.0.4"],"Online":true},"two":{"OS":"linux"}}}"#).unwrap();
        assert_eq!(t.ip, "100.64.0.3");
        assert_eq!(t.dns, "mini.tail.ts.net");
        assert_eq!(t.peers.len(), 1);
    }
    #[test]
    fn private_serve_is_not_funnel() {
        assert!(!funnel_enabled(
            &serde_json::json!({"TCP":{"443":{"HTTPS":true}}})
        ));
        assert!(!funnel_enabled(
            &serde_json::json!({"AllowFunnel":{"a:443":false}})
        ));
        assert!(funnel_enabled(
            &serde_json::json!({"AllowFunnel":{"a:443":true}})
        ));
    }
}
