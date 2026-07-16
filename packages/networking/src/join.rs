//! TCP join handshake: client requests → host Accept/Reject → then UDP input allowed.

use lanplay_protocol::PROTOCOL_VERSION;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireMsg {
    Join {
        name: String,
        protocol: u16,
    },
    Accept {},
    Reject {
        reason: String,
    },
}

/// Visible to the Host UI while waiting for Accept/Reject.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingJoin {
    pub peer_ip: String,
    pub client_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinDecision {
    Accept,
    Reject,
}

struct PendingInner {
    peer_ip: IpAddr,
    client_name: String,
    stream: TcpStream,
}

/// Host-side join listener (control port TCP).
pub struct HostJoinHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    pending: Arc<Mutex<Option<PendingInner>>>,
    /// After Accept, only this IP may send UDP input.
    allowed_peer: Arc<Mutex<Option<IpAddr>>>,
    /// TCP to accepted client (kept open for disconnect detection later).
    accepted: Arc<Mutex<Option<TcpStream>>>,
}

impl HostJoinHandle {
    pub fn pending_snapshot(&self) -> Option<PendingJoin> {
        self.pending.lock().as_ref().map(|p| PendingJoin {
            peer_ip: p.peer_ip.to_string(),
            client_name: p.client_name.clone(),
        })
    }

    pub fn allowed_peer(&self) -> Arc<Mutex<Option<IpAddr>>> {
        Arc::clone(&self.allowed_peer)
    }

    pub fn has_accepted_session(&self) -> bool {
        self.allowed_peer.lock().is_some()
    }

    pub fn decide(&self, decision: JoinDecision) -> Result<String, String> {
        let mut pending = self.pending.lock();
        let Some(mut p) = pending.take() else {
            return Err("No pending join request.".into());
        };

        match decision {
            JoinDecision::Accept => {
                let msg = WireMsg::Accept {};
                write_msg(&mut p.stream, &msg).map_err(|e| e.to_string())?;
                *self.allowed_peer.lock() = Some(p.peer_ip);
                *self.accepted.lock() = Some(p.stream);
                Ok(format!(
                    "Accepted {} ({})",
                    p.client_name, p.peer_ip
                ))
            }
            JoinDecision::Reject => {
                let msg = WireMsg::Reject {
                    reason: "Host rejected the connection.".into(),
                };
                let _ = write_msg(&mut p.stream, &msg);
                let _ = p.stream.shutdown(Shutdown::Both);
                Ok(format!("Rejected {} ({})", p.client_name, p.peer_ip))
            }
        }
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Accept loop is non-blocking; it will exit within ~100ms.
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
        *self.pending.lock() = None;
        *self.allowed_peer.lock() = None;
        if let Some(s) = self.accepted.lock().take() {
            let _ = s.shutdown(Shutdown::Both);
        }
    }
}

impl Drop for HostJoinHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        *self.pending.lock() = None;
        *self.allowed_peer.lock() = None;
        if let Some(s) = self.accepted.lock().take() {
            let _ = s.shutdown(Shutdown::Both);
        }
    }
}

/// Listen for TCP join requests on `control_port`.
pub fn run_host_join_listener(control_port: u16) -> lanplay_shared::Result<HostJoinHandle> {
    let listener = TcpListener::bind(format!("0.0.0.0:{control_port}"))
        .map_err(lanplay_shared::LanPlayError::from)?;
    listener
        .set_nonblocking(true)
        .map_err(lanplay_shared::LanPlayError::from)?;

    let stop = Arc::new(AtomicBool::new(false));
    let pending = Arc::new(Mutex::new(None));
    let allowed_peer = Arc::new(Mutex::new(None));
    let accepted = Arc::new(Mutex::new(None));

    let stop_t = Arc::clone(&stop);
    let pending_t = Arc::clone(&pending);
    let allowed_t = Arc::clone(&allowed_peer);
    let accepted_t = Arc::clone(&accepted);

    let join = thread::Builder::new()
        .name("lanplay-host-join".into())
        .spawn(move || {
            while !stop_t.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, addr)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));

                        // Only one session / one pending at a time
                        if allowed_t.lock().is_some() {
                            let _ = write_msg(
                                &mut stream,
                                &WireMsg::Reject {
                                    reason: "Host already has an active session.".into(),
                                },
                            );
                            let _ = stream.shutdown(Shutdown::Both);
                            continue;
                        }
                        if pending_t.lock().is_some() {
                            let _ = write_msg(
                                &mut stream,
                                &WireMsg::Reject {
                                    reason: "Host is busy with another join request.".into(),
                                },
                            );
                            let _ = stream.shutdown(Shutdown::Both);
                            continue;
                        }

                        match read_msg(&mut stream) {
                            Ok(WireMsg::Join { name, protocol }) => {
                                if protocol != PROTOCOL_VERSION {
                                    let _ = write_msg(
                                        &mut stream,
                                        &WireMsg::Reject {
                                            reason: format!(
                                                "Protocol mismatch (client {protocol}, host {})",
                                                PROTOCOL_VERSION
                                            ),
                                        },
                                    );
                                    let _ = stream.shutdown(Shutdown::Both);
                                    continue;
                                }
                                *pending_t.lock() = Some(PendingInner {
                                    peer_ip: addr.ip(),
                                    client_name: if name.trim().is_empty() {
                                        addr.ip().to_string()
                                    } else {
                                        name
                                    },
                                    stream,
                                });
                            }
                            Ok(_) => {
                                let _ = stream.shutdown(Shutdown::Both);
                            }
                            Err(_) => {
                                let _ = stream.shutdown(Shutdown::Both);
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(100));
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(200));
                    }
                }

                // Drop accepted TCP if peer closed (optional cleanup)
                let mut acc = accepted_t.lock();
                if let Some(ref s) = *acc {
                    let mut buf = [0u8; 1];
                    s.set_nonblocking(true).ok();
                    match s.peek(&mut buf) {
                        Ok(0) => {
                            // peer closed
                            *allowed_t.lock() = None;
                            *acc = None;
                        }
                        Ok(_) => {}
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::TimedOut => {}
                        Err(_) => {
                            *allowed_t.lock() = None;
                            *acc = None;
                        }
                    }
                    if let Some(ref s) = *acc {
                        let _ = s.set_nonblocking(false);
                    }
                }
            }
        })
        .map_err(|e| lanplay_shared::LanPlayError::Message(e.to_string()))?;

    Ok(HostJoinHandle {
        stop,
        join: Some(join),
        pending,
        allowed_peer,
        accepted,
    })
}

/// Client: TCP join request; blocks until Accept/Reject or timeout.
pub fn client_request_join(
    host_ip: &str,
    control_port: u16,
    client_name: &str,
    timeout: Duration,
) -> Result<(), String> {
    let addr = format!("{host_ip}:{control_port}");
    let mut stream = TcpStream::connect_timeout(
        &addr
            .parse()
            .map_err(|e| format!("bad address {addr}: {e}"))?,
        timeout,
    )
    .map_err(|e| format!("Could not reach host at {addr}: {e}"))?;

    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;

    write_msg(
        &mut stream,
        &WireMsg::Join {
            name: client_name.to_string(),
            protocol: PROTOCOL_VERSION,
        },
    )
    .map_err(|e| e.to_string())?;

    match read_msg(&mut stream).map_err(|e| e.to_string())? {
        WireMsg::Accept {} => {
            // Keep TCP open so host can detect disconnect — leak into thread
            std::mem::forget(stream);
            Ok(())
        }
        WireMsg::Reject { reason } => Err(reason),
        WireMsg::Join { .. } => Err("Unexpected message from host.".into()),
    }
}

fn write_msg(stream: &mut TcpStream, msg: &WireMsg) -> std::io::Result<()> {
    let mut line = serde_json::to_string(msg).map_err(std::io::Error::other)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn read_msg(stream: &mut TcpStream) -> std::io::Result<WireMsg> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "connection closed",
        ));
    }
    serde_json::from_str(line.trim()).map_err(std::io::Error::other)
}

/// Best-effort local machine name for join requests.
pub fn local_client_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "LANPlay Client".into())
}
