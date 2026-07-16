//! Legacy stub transport (kept for workspace compile / future swap).

use lanplay_shared::{DEFAULT_CONTROL_PORT, DEFAULT_MEDIA_PORT};

pub trait NetworkTransport {
    fn listen(&mut self, control_port: u16, media_port: u16) -> lanplay_shared::Result<()>;
    fn connect(
        &mut self,
        host_ip: &str,
        control_port: u16,
        media_port: u16,
    ) -> lanplay_shared::Result<()>;
    fn close(&mut self) -> lanplay_shared::Result<()>;
}

#[derive(Debug, Default)]
pub struct StubTransport {
    pub listening: bool,
    pub connected: bool,
    pub peer: Option<String>,
}

impl NetworkTransport for StubTransport {
    fn listen(&mut self, _control_port: u16, _media_port: u16) -> lanplay_shared::Result<()> {
        self.listening = true;
        self.connected = false;
        self.peer = None;
        Ok(())
    }

    fn connect(
        &mut self,
        host_ip: &str,
        _control_port: u16,
        _media_port: u16,
    ) -> lanplay_shared::Result<()> {
        if host_ip.trim().is_empty() {
            return Err(lanplay_shared::LanPlayError::Message(
                "Host IP is required".into(),
            ));
        }
        self.connected = true;
        self.listening = false;
        self.peer = Some(host_ip.trim().to_string());
        Ok(())
    }

    fn close(&mut self) -> lanplay_shared::Result<()> {
        self.listening = false;
        self.connected = false;
        self.peer = None;
        Ok(())
    }
}

pub fn default_ports() -> (u16, u16) {
    (DEFAULT_CONTROL_PORT, DEFAULT_MEDIA_PORT)
}
