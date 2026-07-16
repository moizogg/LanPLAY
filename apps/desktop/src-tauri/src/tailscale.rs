//! Best-effort Tailscale IPv4 discovery for the Host UI.
//!
//! Important: never flash a console window. On Windows, `tailscale` is often a
//! `.cmd` shim — we hide the window and cache results so the UI can poll safely.

use lanplay_shared::TailscaleInfo;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Cache {
    at: Instant,
    value: TailscaleInfo,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// How long to reuse a detection result (avoids spawning CLI every UI tick).
const CACHE_TTL: Duration = Duration::from_secs(8);

/// Try `tailscale ip -4`, then fall back to `tailscale ip`.
pub fn detect_tailscale() -> TailscaleInfo {
    if let Ok(guard) = CACHE.lock() {
        if let Some(c) = guard.as_ref() {
            if c.at.elapsed() < CACHE_TTL {
                return c.value.clone();
            }
        }
    }

    let value = detect_uncached();

    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some(Cache {
            at: Instant::now(),
            value: value.clone(),
        });
    }

    value
}

/// Force a fresh lookup (e.g. user clicked Refresh).
pub fn detect_tailscale_fresh() -> TailscaleInfo {
    if let Ok(mut guard) = CACHE.lock() {
        *guard = None;
    }
    detect_tailscale()
}

fn detect_uncached() -> TailscaleInfo {
    match run_tailscale(&["ip", "-4"]) {
        Some(ip) => TailscaleInfo {
            ip: Some(ip.clone()),
            available: true,
            detail: format!("Detected via tailscale: {ip}"),
        },
        None => {
            if let Some(ip) = run_tailscale(&["ip"]) {
                return TailscaleInfo {
                    ip: Some(ip.clone()),
                    available: true,
                    detail: format!("Detected via tailscale: {ip}"),
                };
            }

            TailscaleInfo {
                ip: None,
                available: false,
                detail: "Tailscale CLI not found or not logged in. Install Tailscale and sign in."
                    .into(),
            }
        }
    }
}

fn run_tailscale(args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("tailscale");
    cmd.args(args);

    // Prevent black CMD windows when `tailscale` resolves to a .cmd/.bat shim.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd.output().ok()?;
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
    parts
        .iter()
        .all(|p| p.parse::<u8>().is_ok() && !p.is_empty() && p.len() <= 3)
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
