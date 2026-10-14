use crate::{
    connections::{Mac, ssh_command, vnc_url},
    storage::Preferences,
    system::Snapshot,
};
use std::fmt::Write;

pub const SCREEN_STEPS: &str = "System Settings → General → Sharing → Screen Sharing (ⓘ)\n1. Confirm Remote Management is off. If an active remote session uses it, arrange local access before changing it.\n2. Set Allow access for to ‘Only these users’. Remove broad groups and add your designated local account.\n3. Leave ‘VNC viewers may control screen with password’ and ‘Anyone may request permission to control screen’ off.\n4. Turn on Screen Sharing, then refresh Mac Link. Authenticate from another Mac to complete verification.";
pub const SSH_STEPS: &str = "System Settings → General → Sharing → Remote Login (ⓘ)\n1. Choose ‘Only these users’ and add the designated local account.\n2. Turn on Remote Login. Leave full disk access for remote users off unless your work needs it.\n3. Preserve existing authorized_keys and sshd configuration. Prefer public keys.\n4. Test ssh <username>@localhost, then test from your other Mac over LAN and Tailscale. Review host key fingerprints before trusting them.\nDo not disable password authentication until a key login and alternative administrative access are verified.";
pub const RECOVERY: &str = "Screen lock — NOT TESTED: lock the host while a trusted client is connected, then reconnect.\nSession disconnect — NOT TESTED: quit Screen Sharing on the client and reconnect.\nInternet interruption — NOT TESTED: during a safe maintenance window, interrupt internet access, restore it, and reconnect over Tailscale.\nLAN interruption — NOT TESTED: with local recovery available, disconnect/reconnect the host’s network and retry.\nUser logout — NOT TESTED: test only with local recovery and explicit approval; user services and some Tailscale variants may stop.\nSystem restart — NOT TESTED: requires explicit approval and a recovery plan. GUI Tailscale variants may need a user login. FileVault pre-boot authentication can prevent remote access before unlocking.\n\nFor unattended access after reboot, assess Tailscale’s documented system-level tailscaled deployment with an administrator. It changes installation/maintenance responsibility and can conflict with the existing GUI variant. Do not replace an installation or install a daemon without approval.\n\nDevelopment services: use an appropriate launchd LaunchAgent for a logged-in user, or a narrowly scoped LaunchDaemon when work must survive logout. Screen Sharing disconnects do not stop launchd jobs. Mac Link does not install background services.\n\nService restart persistence — NOT TESTED: do not restart remote-access services during a session that depends on them. Repeat discovery and real client tests after an approved restart.";
pub const PERFORMANCE: &str = "High Performance requires Apple silicon and macOS Sonoma 14 or later on both Macs, plus communication over UDP 5900, 5901 and 5902. Apple recommends a wired, low-latency network and about 75 Mbps for one 4K display. The local hardware check cannot prove client compatibility, throughput, or UDP connectivity.\n\nChoose High Performance in Apple’s Screen Sharing app when offered. Test a real session over each network; use Standard if unavailable or unstable. A Tailscale ping only checks the peer path, not these UDP ports. Native virtual displays and operation with the monitor disconnected must be tested on the actual pair of Macs.";

pub fn guide(s: &Snapshot) -> String {
    let mut text = format!(
        "## Mac mini Remote Access\n\nComputer name:\n    {}\n\nmacOS:\n    {} ({})\n\nHardware:\n    {} · {} · {}\n\nUsername:\n    {}\n\nLAN IP:\n    {}\n\nTailscale IP:\n    {}\n\nMagicDNS:\n    {}\n\n",
        s.name,
        s.os,
        s.build,
        s.model,
        s.chip,
        s.arch,
        s.user,
        s.lan
            .iter()
            .map(|(i, a)| format!("{a} ({i})"))
            .collect::<Vec<_>>()
            .join(", "),
        if s.tailnet.ip.is_empty() {
            "Unavailable"
        } else {
            &s.tailnet.ip
        },
        if s.tailnet.magic_dns {
            &s.tailnet.dns
        } else {
            "Not confirmed enabled"
        }
    );
    let mut hosts = Vec::new();
    for (i, a) in &s.lan {
        hosts.push((format!("LAN ({i})"), a.clone()));
    }
    if !s.tailnet.ip.is_empty() {
        hosts.push(("Tailscale".into(), s.tailnet.ip.clone()));
    }
    if s.tailnet.magic_dns && !s.tailnet.dns.is_empty() {
        hosts.push(("MagicDNS".into(), s.tailnet.dns.clone()));
    }
    text.push_str("### Connect using native Screen Sharing\n\n");
    for (label, h) in &hosts {
        if let Ok(url) = vnc_url(h) {
            let _ = writeln!(text, "{label}:\n\n    open '{url}'\n");
        }
    }
    text.push_str("### Connect using SSH\n\n");
    for (label, h) in &hosts {
        if let Ok(cmd) = ssh_command(&Mac {
            host: h.clone(),
            user: s.user.clone(),
            ..Mac::default()
        }) {
            let _ = writeln!(text, "{label}:\n\n    {cmd}\n");
        }
    }
    text.push_str("### Native macOS connection\n\nOn the client Mac:\n\n1. Open Spotlight and launch Screen Sharing.\n2. Enter this Mac mini’s Tailscale address.\n3. Authenticate with the authorized macOS account.\n4. Select Standard or High Performance if available.\n\nAlternatively use Finder → Go → Connect to Server and enter its vnc:// address. Keep these addresses private. These commands are connection instructions, not evidence of successful login.\n");
    text
}

pub fn full(s: &Snapshot, prefs: &Preferences) -> String {
    let mut text = guide(s);
    let _ = writeln!(
        text,
        "\n## Discovery & verification\n\nSnapshot time: Unix {}. All findings apply to this Mac, not to a selected peer.\n",
        s.timestamp
    );
    for c in &s.checks {
        let _ = writeln!(
            text,
            "### {} — {}\n\n{}\n",
            c.name,
            c.state.label(),
            c.detail
        );
    }
    let _ = writeln!(
        text,
        "## Required setup\n\n{SCREEN_STEPS}\n\n{SSH_STEPS}\n\n## Headless & High Performance\n\n{PERFORMANCE}\n\n## Restart & recovery\n\n{RECOVERY}\n"
    );
    text.push_str("## Changes & rollback\n\nMac Link never modifies sharing permissions, SSH configuration/keys, FileVault, automatic login, firewall/router rules, Tailscale identity/routes/Funnel, or launchd jobs. Sharing changes are made through System Settings and require recording their previous state. It can change supported AC sleep and wake settings only after an explicit action and native administrator authorization.\n\n");
    if let Some(b) = &prefs.power_backup {
        let _ = writeln!(
            text,
            "Power change attempted at Unix {}. Previous settings: {:?}. Intended settings: {:?}. Check the read-back evidence below to establish success.\n\nRestore the recorded AC values with:\n\n    sudo /usr/bin/pmset -c {}\n\nThen verify with `pmset -g custom`. Display sleep and unrelated power settings are not modified.\n",
            b.timestamp,
            b.previous,
            b.applied,
            b.previous
                .iter()
                .map(|(k, v)| format!("{k} {v}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    } else {
        text.push_str("No power modification has been recorded by this app.\n\n");
    }
    text.push_str("To undo manual Sharing setup, return only the toggles and user lists you changed to their recorded previous state. Do this with local access if disabling the connection you depend on. Remove saved Macs in the app if desired. No runtime daemon is installed, so closing the app does not interrupt Apple Screen Sharing or SSH.\n\n");
    text.push_str("## Commands & observations\n\nExit status is recorded independently from feature verification. Permission failures and unavailable services are retained. No passwords, private keys or raw Tailscale status identities are stored.\n\n");
    for e in s.evidence.iter().chain(prefs.actions.iter()) {
        let _ = writeln!(
            text,
            "### {}\n\nProcess/operation success: {}\n\n```text\n{}\n```\n",
            e.command.replace('`', ""),
            e.success,
            e.output.replace("```", "'''")
        );
    }
    text.push_str("## References\n\n- [Apple: Turn Screen Sharing on or off](https://support.apple.com/guide/mac-help/mh11848/mac)\n- [Apple: Remote Login](https://support.apple.com/guide/mac-help/mchlp1066/mac)\n- [Apple: High Performance Screen Sharing](https://support.apple.com/guide/remote-desktop/apdf8e09f5a9/mac)\n- [Tailscale: macOS variants](https://tailscale.com/docs/concepts/macos-variants)\n");
    text
}
