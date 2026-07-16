//! Windows Firewall: allow LANPlay host ports (like clicking "Allow access").
//!
//! On Start Host we ensure inbound rules for the exe + control/input/video ports.
//! First time may show a one-click UAC prompt; after that it's silent.

use std::path::PathBuf;
use std::process::Command;

const RULE_APP: &str = "LANPlay";
const RULE_TCP: &str = "LANPlay Control TCP";
const RULE_UDP_IN: &str = "LANPlay Input UDP";
const RULE_UDP_VID: &str = "LANPlay Video UDP";
const RULE_OUT: &str = "LANPlay Out";

/// Ensure host traffic is allowed. Safe to call repeatedly.
pub fn ensure_host_firewall(control_port: u16, media_port: u16, video_port: u16) -> String {
    #[cfg(windows)]
    {
        windows_impl::ensure(control_port, media_port, video_port)
    }
    #[cfg(not(windows))]
    {
        let _ = (control_port, media_port, video_port);
        "Firewall: n/a (non-Windows)".into()
    }
}

/// Client: allow LANPlay through the firewall (outbound HELLO + replies).
pub fn ensure_client_firewall() -> String {
    #[cfg(windows)]
    {
        windows_impl::ensure_app_outbound()
    }
    #[cfg(not(windows))]
    {
        "Firewall: n/a".into()
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn exe_path() -> Option<PathBuf> {
        std::env::current_exe().ok()
    }

    fn rule_exists(name: &str) -> bool {
        let name_arg = format!("name={name}");
        let out = Command::new("netsh")
            .args(["advfirewall", "firewall", "show", "rule", &name_arg])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        match out {
            Ok(o) => {
                let s = String::from_utf8_lossy(&o.stdout);
                o.status.success() && !s.contains("No rules match") && s.contains(name)
            }
            Err(_) => false,
        }
    }

    fn netsh(args: &[String]) -> bool {
        Command::new("netsh")
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn s(a: &str) -> String {
        a.to_string()
    }

    fn try_add_rules_silent(exe: &str, control: u16, media: u16, video: u16) -> bool {
        let app_ok = rule_exists(RULE_APP)
            || netsh(&[
                s("advfirewall"),
                s("firewall"),
                s("add"),
                s("rule"),
                format!("name={RULE_APP}"),
                s("dir=in"),
                s("action=allow"),
                format!("program={exe}"),
                s("enable=yes"),
                s("profile=any"),
            ]);

        let tcp_ok = rule_exists(RULE_TCP)
            || netsh(&[
                s("advfirewall"),
                s("firewall"),
                s("add"),
                s("rule"),
                format!("name={RULE_TCP}"),
                s("dir=in"),
                s("action=allow"),
                s("protocol=TCP"),
                format!("localport={control}"),
                s("enable=yes"),
                s("profile=any"),
            ]);

        let udp_in_ok = rule_exists(RULE_UDP_IN)
            || netsh(&[
                s("advfirewall"),
                s("firewall"),
                s("add"),
                s("rule"),
                format!("name={RULE_UDP_IN}"),
                s("dir=in"),
                s("action=allow"),
                s("protocol=UDP"),
                format!("localport={media}"),
                s("enable=yes"),
                s("profile=any"),
            ]);

        let udp_vid_ok = rule_exists(RULE_UDP_VID)
            || netsh(&[
                s("advfirewall"),
                s("firewall"),
                s("add"),
                s("rule"),
                format!("name={RULE_UDP_VID}"),
                s("dir=in"),
                s("action=allow"),
                s("protocol=UDP"),
                format!("localport={video}"),
                s("enable=yes"),
                s("profile=any"),
            ]);

        let _ = netsh(&[
            s("advfirewall"),
            s("firewall"),
            s("add"),
            s("rule"),
            format!("name={RULE_OUT}"),
            s("dir=out"),
            s("action=allow"),
            format!("program={exe}"),
            s("enable=yes"),
            s("profile=any"),
        ]);

        app_ok && tcp_ok && udp_in_ok && udp_vid_ok
    }

    fn try_add_rules_elevated(exe: &str, control: u16, media: u16, video: u16) -> bool {
        let script = format!(
            r#"@echo off
netsh advfirewall firewall delete rule name="{RULE_APP}" >nul 2>&1
netsh advfirewall firewall delete rule name="{RULE_TCP}" >nul 2>&1
netsh advfirewall firewall delete rule name="{RULE_UDP_IN}" >nul 2>&1
netsh advfirewall firewall delete rule name="{RULE_UDP_VID}" >nul 2>&1
netsh advfirewall firewall delete rule name="{RULE_OUT}" >nul 2>&1
netsh advfirewall firewall add rule name="{RULE_APP}" dir=in action=allow program="{exe}" enable=yes profile=any
netsh advfirewall firewall add rule name="{RULE_TCP}" dir=in action=allow protocol=TCP localport={control} enable=yes profile=any
netsh advfirewall firewall add rule name="{RULE_UDP_IN}" dir=in action=allow protocol=UDP localport={media} enable=yes profile=any
netsh advfirewall firewall add rule name="{RULE_UDP_VID}" dir=in action=allow protocol=UDP localport={video} enable=yes profile=any
netsh advfirewall firewall add rule name="{RULE_OUT}" dir=out action=allow program="{exe}" enable=yes profile=any
"#
        );

        let bat = std::env::temp_dir().join("lanplay_firewall.bat");
        if std::fs::write(&bat, script).is_err() {
            return false;
        }

        let bat_path = bat.display().to_string().replace('\'', "''");
        let status = Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &format!(
                    "Start-Process -FilePath 'cmd.exe' -ArgumentList '/c \"{bat_path}\"' -Verb RunAs -Wait -WindowStyle Hidden"
                ),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status();

        let _ = std::fs::remove_file(&bat);
        status.map(|s| s.success()).unwrap_or(false) && rule_exists(RULE_APP)
    }

    pub fn ensure(control: u16, media: u16, video: u16) -> String {
        let Some(exe) = exe_path() else {
            return "Firewall: could not resolve lanplay.exe path".into();
        };
        let exe_s = exe.to_string_lossy().to_string();

        if rule_exists(RULE_APP) && rule_exists(RULE_UDP_VID) {
            return format!("Firewall OK (app + TCP {control} / UDP {media},{video})");
        }

        if try_add_rules_silent(&exe_s, control, media, video) {
            return format!("Firewall allowed (TCP {control}, UDP {media}/{video})");
        }

        if try_add_rules_elevated(&exe_s, control, media, video) {
            return format!("Firewall allowed via UAC (TCP {control}, UDP {media}/{video})");
        }

        "Firewall: click Allow if Windows asks, or Approve UAC once".into()
    }

    pub fn ensure_app_outbound() -> String {
        let Some(exe) = exe_path() else {
            return "Firewall: n/a".into();
        };
        let exe_s = exe.to_string_lossy().to_string();
        if rule_exists(RULE_APP) || rule_exists(RULE_OUT) {
            return "Firewall OK (client)".into();
        }
        if try_add_rules_silent(&exe_s, 47800, 47801, 47802) {
            return "Firewall allowed (client)".into();
        }
        if try_add_rules_elevated(&exe_s, 47800, 47801, 47802) {
            return "Firewall allowed via UAC (client)".into();
        }
        "Firewall: allow LANPlay if Windows prompts".into()
    }
}
