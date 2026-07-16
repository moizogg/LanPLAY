//! Networking for LANPlay.
//!
//! V1: Tailscale IP + TCP join handshake (accept/reject) + UDP input.

mod join;
mod stub;

pub use join::{
    client_request_join, local_client_name, run_host_join_listener, HostJoinHandle, JoinDecision,
    PendingJoin,
};
pub use stub::{default_ports, NetworkTransport, StubTransport};
