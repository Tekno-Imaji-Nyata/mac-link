# Mac Link

A Rust macOS desktop app for setting up a Mac mini and connecting from another Mac using **Apple Screen Sharing**, macOS SSH, and your existing Tailscale network. It does not implement or install a remote desktop server.

## Download and install

**[Download Mac Link for macOS — universal DMG](https://github.com/Tekno-Imaji-Nyata/mac-link/releases/download/v0.1.0/Mac-Link-0.1.0-universal.dmg)**

[All releases and ZIP downloads](https://github.com/Tekno-Imaji-Nyata/mac-link/releases/latest) · [SHA-256 checksums](https://github.com/Tekno-Imaji-Nyata/mac-link/releases/download/v0.1.0/SHA256SUMS.txt) · [MIT license](LICENSE)

1. Download the DMG on the Mac where you want to use Mac Link.
2. Open it and drag **Mac Link** onto **Applications**.
3. Eject the disk image, then open **Mac Link** from Applications.
4. Choose **Set up this Mac** on the Mac mini host, or **Connect to a Mac** on your other Mac.

The download includes **Apple silicon and Intel** binaries and targets **macOS 12 or later**. You do not need Rust, Homebrew, or a separate installer. The host's native sharing services work independently of Mac Link; you can close the app after setup. Tailscale is optional for LAN access and is installed separately for access through your tailnet.

**First launch:** this community release is ad-hoc signed and **not Apple notarized**. If macOS blocks it, first attempt to open it, then use **System Settings → Privacy & Security → Open Anyway** if you trust the download. On macOS 12, use the corresponding Security & Privacy pane in System Preferences. Follow [Apple's instructions](https://support.apple.com/en-us/102445); do not disable Gatekeeper globally. Managed Macs may require administrator approval.

To verify a download, place `SHA256SUMS.txt` beside the downloaded DMG and ZIP and run:

```sh
shasum -a 256 -c SHA256SUMS.txt
```

If you downloaded only one artifact, compare its `shasum -a 256 <filename>` output with the matching entry instead.

## Set up the host Mac

1. Open Sharing settings from the app. Enable **Screen Sharing**, choose **Only these users**, and add the host's designated local account. Remove broad groups and leave legacy VNC passwords and permission requests off. Remote Management must be off. On macOS 13 and later this is **System Settings → General → Sharing**; on macOS 12 it is **System Preferences → Sharing**.
2. Open your existing Tailscale app and sign in if necessary. If absent, Mac Link links to the official download. Connect both Macs to your tailnet. Mac Link does not replace an installation or change routes, identity, access policy, or Funnel.
3. Enable **Remote Login** for designated users in Sharing if you want SSH. Existing SSH keys and configuration remain unchanged.
4. On a desktop Mac, **Keep awake on AC** records previous settings before requesting native administrator authorization. It changes only supported AC `sleep` and `womp` values, reads them back, and offers **Restore AC settings**. Display sleep is preserved. Keeping a system awake uses more energy.
5. Copy the host address from **Connection guide**. Use its LAN address locally, or its Tailscale IP/MagicDNS name remotely.

## Connect from another Mac

Install the same app on the client, choose **Connect to a Mac**, enter the host address and the host's username, and open **Screen Sharing** or **SSH**. You can save Macs for later and select Mac peers discovered through Tailscale.

You can also use Apple's clients directly, without installing Mac Link on the client:

```sh
# Replace the example address and username with the host's details.
open 'vnc://mac-mini.local'
ssh yourname@mac-mini.local
```

For remote access, replace `mac-mini.local` with the host's Tailscale address or MagicDNS name. Authenticate with the authorized account on the **host**, not necessarily your client Mac's account. No public router port forwarding is needed.

## What it does

- Read-only discovery of hardware, macOS, interfaces, native sharing, SSH, Tailscale, firewall, FileVault, displays, and power settings.
- Guided setup through Apple's settings, with explicit access restrictions.
- Saved Macs, Tailscale peer discovery and ping, native Screen Sharing, and SSH in Terminal.
- Verification and connection reports that distinguish **VERIFIED**, **CONFIGURED BUT NOT TESTED**, **REQUIRES USER ACTION**, and **UNSUPPORTED**.
- Optional, reversible AC power changes with native macOS administrator authorization.

A successful TCP check confirms reachability only; it does not prove a working desktop or successful authentication. Authentication stays in Apple's clients. Mac Link stores no passwords and opens no VNC or SSH server of its own.

## Build from source

Requires macOS, Xcode Command Line Tools, Rust via rustup, and Python 3 for packaging. `rust-toolchain.toml` pins the tested Rust version. The GUI uses eframe/egui in a native macOS window, with no web server, browser runtime, Node, or background helper.

```sh
git clone https://github.com/Tekno-Imaji-Nyata/mac-link.git
cd mac-link
make run
make check
make test
make app
open 'dist/Mac Link.app'
```

`make app` builds an app and ZIP for the build machine's architecture. To build both architectures and the installer-style disk image:

```sh
make dmg
```

This installs the two macOS Rust targets through rustup, builds with the committed lockfile, combines them with `lipo`, ad-hoc signs the app, and verifies the DMG. Output is in `dist/release/`: a universal DMG, ZIP, and `SHA256SUMS.txt`. Build paths are remapped in the release binaries. The package includes only the app, license notices, installation instructions, and an Applications shortcut; local reports and saved settings are excluded.

Dependency notices are committed for the locked macOS dependencies. After changing dependencies, run `make notices` to regenerate them from crate license files and pinned upstream sources. This step needs network access. Run `make check` and `make test` before submitting a pull request. CI runs these checks on Apple silicon and Intel macOS.

Read-only command-line discovery:

```sh
./target/release/mac-link --report
./target/release/mac-link --report-json
```

## Evidence and storage

**Verification** shows the state and limits of each observation. **Connection guide** copies or exports a report containing system discovery, addresses, command results, security checks, recovery tests, and power rollback instructions. Exported reports contain private machine/network details; share with trusted people only.

Saved Macs, action history, and the power rollback record live in `~/Library/Application Support/Mac Link/settings.json` (directory mode 0700, file mode 0600, atomic writes). Exports go to its `Reports` directory and are revealed in Finder. Read-only discovery is in memory unless explicitly exported. Raw Tailscale status JSON, credentials, SSH key contents, and FileVault keys are not logged. Corrupt settings files are preserved and block writes. No telemetry.

Commands run off the UI thread with timeouts, capped output and separate stdout/stderr draining. Hostnames and usernames are validated before being passed as process arguments. No user input is inserted into a shell command. The only elevated command is a fixed `pmset` operation with allowlisted keys and typed integer values.

## Limits and recovery

- macOS controls sharing permissions through System Settings. The app guides that setup; a launchd job or access group does not establish that authentication works. Administrative/privacy barriers remain visible.
- A local SSH test uses batch/key authentication and strict host-key verification. Unknown keys or missing credentials produce a failed test without changing trust or prompting for a password. Verify host keys independently before trusting them in Terminal.
- High Performance and native virtual displays require compatible client and host hardware, macOS, and network conditions. Standard mode remains available. See [Apple's current requirements](https://support.apple.com/guide/remote-desktop/apdf8e09f5a9/mac). No virtual display drivers or dummy plugs are installed.
- Monitor disconnect, screen lock, logout, network interruption, service restart, and reboot require real tests. They are not inferred from an open port. The app never reboots, logs out, or restarts remote-access services.
- GUI Tailscale deployments can depend on login; FileVault can require a pre-boot unlock. A system daemon is a separate administrative deployment decision. See [Tailscale's macOS variants](https://tailscale.com/docs/concepts/macos-variants).
- Background development services should use suitable launchd jobs if they must outlive terminal sessions. Mac Link installs none.
- Router port forwarding and external IPv6 exposure cannot be proven absent from this Mac. No public forwarding is required. Review existing rules and tailnet access policy separately.

Rollback: use **Restore AC settings** for recorded power changes. Restore manual Sharing changes to their previous values only with a safe access path. Closing/removing Mac Link does not alter native remote access. The app does not disable FileVault, enable automatic login, change SSH password policy, or install third-party streaming software.

Setup references: [Apple Screen Sharing](https://support.apple.com/guide/mac-help/mh11848/mac), [Apple Remote Login](https://support.apple.com/guide/mac-help/mchlp1066/mac).

## License and contributions

Mac Link is open source under the [MIT license](LICENSE). Dependencies retain their own licenses; see [third-party notices](packaging/THIRD-PARTY-NOTICES.txt). Issues and pull requests are welcome. Please remove machine names, addresses, usernames, and other private details before sharing diagnostics in a public issue.
