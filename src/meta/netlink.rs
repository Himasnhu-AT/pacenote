//! The one place the menus talk to networking. A thin adapter over
//! `crate::net_session` (roles, host / join / leave, peer names); see that module.
//!
//! - [`host`]: the driver starts a listen server and becomes [`Role::Driver`].
//! - [`join`]: the guide connects to the driver and becomes [`Role::Guide`]; from then on the
//!   net module owns the guide's flow (it enters the stage when the host starts one).
//! - [`leave`]: back to [`Role::Solo`], sockets closed.
//! - [`status`]: a line for the lobby / join screen.

use bevy::prelude::*;

pub use crate::net_session::Role;
use crate::net_session::{self, Session};

/// Connection state for the menus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetStatus {
    Idle,
    /// Hosting; `peer` = a guide is connected (with their name if known).
    Hosting { port: u16, peer: Option<String> },
    Connecting(String),
    Connected(String),
    Failed(String),
}

/// Host as the driver (the driver's name from the profile goes to the guide).
pub fn host(world: &mut World, port: u16) -> Result<(), String> {
    let name = world.get_resource::<super::Profile>().map(|p| p.driver_name.clone()).unwrap_or_default();
    world.resource_mut::<Session>().name = name;
    net_session::host(world, port).map(|_| ()).map_err(|e| format!("Can't host on port {port}: {e}"))
}

/// Join a driver at `host:port` as the guide.
pub fn join(world: &mut World, addr: &str) -> Result<(), String> {
    let addr = addr.trim();
    if addr.is_empty() {
        return Err("Enter the driver's address (host:port)".into());
    }
    if !addr.contains(':') {
        return Err(format!("'{addr}' has no port (e.g. 192.168.1.20:7777)"));
    }
    let Some(a) = engine::net::parse_addr(addr) else {
        return Err(format!("Can't find '{addr}'"));
    };
    let name = world.get_resource::<super::Profile>().map(|p| p.guide_name.clone()).unwrap_or_default();
    world.resource_mut::<Session>().name = name;
    net_session::join(world, a).map_err(|e| format!("Can't connect to {addr}: {e}"))
}

pub fn leave(world: &mut World) {
    net_session::leave(world);
}

pub fn status(world: &World) -> NetStatus {
    let s = world.resource::<Session>();
    let addr = s.addr.map(|a| a.to_string()).unwrap_or_default();
    match role(world) {
        Role::Solo => NetStatus::Idle,
        Role::Driver => NetStatus::Hosting { port: s.addr.map_or(0, |a| a.port()), peer: s.guide.map(|_| s.peer_name.clone().unwrap_or_else(|| "Guide".into())) },
        Role::Guide if s.connected => NetStatus::Connected(addr),
        Role::Guide => match s.last_drop {
            Some(r) if s.was_connected || !matches!(r, engine::net::DisconnectReason::Requested) => NetStatus::Failed(format!("No answer from {addr} ({r:?}); retrying")),
            _ => NetStatus::Connecting(addr),
        },
    }
}

/// The guide's name as the driver's machine knows it (sent by the guide on connect), if any.
pub fn peer_name(world: &World) -> Option<String> {
    match status(world) {
        NetStatus::Hosting { peer, .. } => peer,
        _ => None,
    }
}

pub fn role(world: &World) -> Role {
    world.get_resource::<Role>().copied().unwrap_or_default()
}
