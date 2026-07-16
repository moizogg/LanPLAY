//! Best-effort Tailscale IPv4 discovery for the Host UI.

use lanplay_shared::TailscaleInfo;
use std::process::Command;

/// Try `tailscale ip -4`, then fall back to scanning `tailscale status --json` style text.
pub fn detect_tailscale() -> TailscaleInfo {
    match run_tailscale_ip() {
        Some(ip) => TailscaleInfo {
            ip: Some(ip.clone()),
            available: true,
            detail: format!("Detected via `tailscale ip -4`: {ip}"),
        },
        None => {
            // Secondary: parse `tailscale ip` (may print v4 + v6).
            if let Some(ip) = run_tailscale_ip_any() {
                return TailscaleInfo {
                    ip: Some(ip.clone()),
                    available: true,
                    detail: format!("Detected via `tailscale ip`: {ip}"),
                };
            }

            TailscaleInfo {
                ip: None,
                available: false,
                detail: "Tailscale CLI not found or not logged in. Install Tailscale and run `tailscale ip -4`.".into(),
            }
        }
    }
}

fn run_tailscale_ip() -> Option<String> {
    let output = Command::new("tailscale").args(["ip", "-4"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    first_ipv4(&text)
}

fn run_tailscale_ip_any() -> Option<String> {
    let output = Command::new("tailscale").arg("ip").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    first_ipv4(&text)
}

fn first_ipv4(text: &str) -> Option<String> {
    for token in text.split_whitespace() {
        let candidate = token.trim();
        if is_ipv4(candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

fn is_ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|p| {
        p.parse::<u8>().is_ok() && !p.is_empty() && p.len() <= 3
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ipv4_line() {
        assert_eq!(first_ipv4("100.64.1.5\n"), Some("100.64.1.5".into()));
        assert_eq!(
            first_ipv4("100.64.1.5\nfd7a:115c::1\n"),
            Some("100.64.1.5".into())
        );
    }
}
