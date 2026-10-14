use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Evidence {
    pub command: String,
    pub output: String,
    pub success: bool,
}

// Drain both pipes concurrently and cap retained output. No shell is involved.
pub fn run(program: &str, args: &[&str], seconds: u64) -> Evidence {
    let label = std::iter::once(program)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ");
    let mut child = match Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .env("TAILSCALE_BE_CLI", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return Evidence {
                command: label,
                output: error.to_string(),
                success: false,
            };
        }
    };
    fn drain(mut pipe: impl Read + Send + 'static) -> mpsc::Receiver<String> {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut kept = Vec::new();
            let mut buf = [0u8; 4096];
            while let Ok(n) = pipe.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let take = n.min(128_000usize.saturating_sub(kept.len()));
                kept.extend_from_slice(&buf[..take]);
            }
            let _ = tx.send(String::from_utf8_lossy(&kept).trim().to_string());
        });
        rx
    }
    let stdout = drain(child.stdout.take().expect("piped stdout"));
    let stderr = drain(child.stderr.take().expect("piped stderr"));
    let start = Instant::now();
    let mut timeout = false;
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if start.elapsed() < Duration::from_secs(seconds) => {
                thread::sleep(Duration::from_millis(25))
            }
            _ => {
                timeout = true;
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let mut output = [stdout, stderr]
        .into_iter()
        .filter_map(|r| r.recv_timeout(Duration::from_secs(1)).ok())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if timeout {
        output.push_str("\nTimed out; result is unverified.");
    }
    Evidence {
        command: label,
        output,
        success,
    }
}

pub fn apple_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timeout_and_large_output_do_not_block() {
        assert!(!run("/bin/sleep", &["2"], 0).success);
        let r = run("/usr/bin/yes", &["test"], 1);
        assert!(!r.success);
        assert!(r.output.len() < 129_000);
    }
    #[test]
    fn literal_arguments_are_not_executed() {
        assert_eq!(
            run("/usr/bin/printf", &["%s", "$(whoami);'\""], 2).output,
            "$(whoami);'\""
        );
    }
}
