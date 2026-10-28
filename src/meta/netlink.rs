//! The one place the menus talk to networking. Until the guide-net stream's module lands
//! these are stubs with the agreed signatures; swapping them for the real functions is a
//! change to this file only (re-export theirs, delete the stubs).
//!
//! - [`host`]: the driver starts a listen server and becomes [`Role::Driver`].
//! - [`join`]: the guide connects to the driver and becomes [`Role::Guide`]; from then on the
//!   net module owns the guide's flow (it enters the stage when the host starts one).
//! - [`leave`]: back to [`Role::Solo`], sockets closed.
//! - [`status`]: a line for the lobby / join screen.

use bevy::prelude::*;

/// Who this machine is in the team.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Role {
    /// Practice: no guide connected, the run counts as solo.
    #[default]
    Solo,
    Driver,
    Guide,
}

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

/// Stub state (replaced by the net module's own).
#[derive(Resource, Clone, Debug)]
struct StubNet(NetStatus);

pub fn host(world: &mut World, port: u16) -> Result<(), String> {
    world.insert_resource(Role::Driver);
    world.insert_resource(StubNet(NetStatus::Hosting { port, peer: None }));
    info!("netlink (stub): hosting on port {port}");
    Ok(())
}

pub fn join(world: &mut World, addr: &str) -> Result<(), String> {
    let addr = addr.trim();
    if addr.is_empty() {
        return Err("Enter the driver's address (host:port)".into());
    }
    if !addr.contains(':') {
        return Err(format!("'{addr}' has no port (e.g. 192.168.1.20:7777)"));
    }
    world.insert_resource(StubNet(NetStatus::Failed("Networking isn't in this build yet".into())));
    Err("Networking isn't in this build yet (guide-net stream)".into())
}

pub fn leave(world: &mut World) {
    world.insert_resource(Role::Solo);
    world.insert_resource(StubNet(NetStatus::Idle));
}

pub fn status(world: &World) -> NetStatus {
    world.get_resource::<StubNet>().map_or(NetStatus::Idle, |s| s.0.clone())
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
