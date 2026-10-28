//! The network session: who this machine is ([`Role`]) and plain functions to host, join and
//! leave. Other modules (voice, menus/lobby) rely on this file; keep it small and stable.
//!
//! - The **driver hosts** a listen server (engine `net`, UDP, default port 7777) and runs the
//!   authoritative car simulation. At most one guide connects.
//! - The **guide joins** as a client, receives the stage id, car state and run (see
//!   `guide::sync`) and never simulates the car. If the link drops the guide retries every
//!   [`RETRY_SECS`] until [`leave`] is called.
//! - **Solo** (default) has no network at all.
//!
//! Command line: `--role driver [--host [port]]`, `--role guide [--connect host[:port]]`.
//! `--host` alone implies the driver, `--connect` alone the guide (the engine's own net flags;
//! the engine starts the session at `Startup`, this module fills in what it misses at
//! `PostStartup`). From code (lobby): [`host`], [`join`], [`leave`] on `&mut World`.
//!
//! For voice: read [`Role`] and [`Session`] (`Session::peer_connected`, `Session::guide`), then
//! send with the engine's `NetSender` (`to_server` on the guide, `to_client(guide)` on the
//! driver) and read `FromClient<M>` / `FromServer<M>`. Register your messages with
//! `add_net_message` in your plugin's `build` (same order on both sides: same binary).

use std::net::SocketAddr;

use bevy::prelude::*;
use engine::net::{self, Channel, ClientId, DisconnectReason, FromClient, FromServer, NetAppExt, NetClient, NetConfig, NetEvent, NetSender, NetServer};

/// Default UDP port.
pub const DEFAULT_PORT: u16 = net::DEFAULT_PORT;
/// A dropped guide reconnects after this many seconds.
pub const RETRY_SECS: f32 = 2.0;

/// Who this machine is in the team.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Role {
    /// Drives alone, no network.
    #[default]
    Solo,
    /// Drives and hosts: the car simulation is authoritative here.
    Driver,
    /// Reads the map; joined to a driver; never simulates or controls the car.
    Guide,
}

impl Role {
    /// `--role driver|guide|solo`, else `--host` → Driver, `--connect` → Guide, else Solo.
    pub fn from_args() -> Role {
        match engine::cli::arg_value("role").as_deref().map(str::to_ascii_lowercase).as_deref() {
            Some("driver") => return Role::Driver,
            Some("guide") => return Role::Guide,
            Some("solo") => return Role::Solo,
            Some(other) => warn!("--role {other}: expected driver, guide or solo"),
            None => {}
        }
        if engine::cli::has_flag("host") {
            Role::Driver
        } else if engine::cli::arg_value("connect").is_some() {
            Role::Guide
        } else {
            Role::Solo
        }
    }

    pub fn is_networked(self) -> bool {
        self != Role::Solo
    }
}

/// Live state of the session (both roles).
#[derive(Resource, Clone, Debug, Default)]
pub struct Session {
    /// Driver: the address it listens on. Guide: the driver's address (reconnects go there).
    pub addr: Option<SocketAddr>,
    /// Driver: the connected guide's client id.
    pub guide: Option<ClientId>,
    /// Guide: handshake done, receiving from the driver.
    pub connected: bool,
    /// Guide: was connected at least once this session.
    pub was_connected: bool,
    /// Why the peer last went away.
    pub last_drop: Option<DisconnectReason>,
    /// Real-time seconds of the last connect / disconnect of the peer (for notices).
    pub changed_at: f64,
    /// Round-trip time to the peer (ms), 0 when unknown.
    pub rtt_ms: f32,
    /// This player's name, sent to the peer on connect (set it before [`host`] / [`join`]).
    pub name: String,
    /// The other player's name, once their [`Hello`] arrived (cleared when they go).
    pub peer_name: Option<String>,
    /// Guide: false after [`leave`] (no more reconnects).
    pub(crate) wants: bool,
    /// Guide: when to try again (real-time seconds).
    pub(crate) retry_at: Option<f64>,
}

impl Session {
    /// The other player is there (driver: a guide is connected; guide: connected to the driver).
    pub fn peer_connected(&self) -> bool {
        self.guide.is_some() || self.connected
    }
}

/// Host as the driver on UDP `port` (0 = any free port). Returns the bound address.
pub fn host(world: &mut World, port: u16) -> std::io::Result<SocketAddr> {
    world.insert_resource(Role::Driver);
    let addr = net::host_udp(world, port, true)?;
    let mut s = world.resource_mut::<Session>();
    s.addr = Some(addr);
    s.guide = None;
    s.wants = true;
    Ok(addr)
}

/// Join a driver as the guide over UDP. Keeps retrying on drops until [`leave`].
pub fn join(world: &mut World, addr: SocketAddr) -> std::io::Result<()> {
    join_via(world, addr, Connector::udp())
}

/// Join over any transport: `make` builds a fresh one for each (re)connect (tests use the
/// engine's in-memory `LoopbackNet`: `move || lo.bind(0)`).
pub fn join_with<T: net::Transport>(world: &mut World, addr: SocketAddr, make: impl Fn() -> T + Send + Sync + 'static) {
    let c = Connector(std::sync::Arc::new(move |w: &mut World, a: SocketAddr| {
        net::connect(w, make(), a);
        Ok(())
    }));
    let _ = join_via(world, addr, c);
}

fn join_via(world: &mut World, addr: SocketAddr, c: Connector) -> std::io::Result<()> {
    world.insert_resource(Role::Guide);
    {
        let mut s = world.resource_mut::<Session>();
        s.addr = Some(addr);
        s.wants = true;
        s.retry_at = None;
    }
    world.insert_resource(c.clone());
    (c.0)(world, addr)
}

/// How the guide (re)connects.
#[derive(Resource, Clone)]
struct Connector(std::sync::Arc<ConnectFn>);

type ConnectFn = dyn Fn(&mut World, SocketAddr) -> std::io::Result<()> + Send + Sync;

impl Connector {
    fn udp() -> Connector {
        Connector(std::sync::Arc::new(|w: &mut World, a: SocketAddr| net::connect_udp(w, a)))
    }
}

/// Host over any transport (tests).
pub fn host_with(world: &mut World, transport: impl net::Transport) {
    world.insert_resource(Role::Driver);
    net::host(world, transport, true);
    let mut s = world.resource_mut::<Session>();
    s.guide = None;
    s.wants = true;
}

/// Leave the session (guide) or stop hosting (driver; the guide sees "driver left"), and go
/// back to `Role::Solo`.
pub fn leave(world: &mut World) {
    world.insert_resource(Role::Solo);
    {
        let mut s = world.resource_mut::<Session>();
        s.peer_name = None;
        s.wants = false;
        s.retry_at = None;
        s.guide = None;
        s.connected = false;
    }
    net::disconnect(world);
}

pub struct NetSessionPlugin;

impl Plugin for NetSessionPlugin {
    fn build(&self, app: &mut App) {
        let role = Role::from_args();
        app.insert_resource(role).init_resource::<Session>();
        {
            let mut cfg = app.world_mut().resource_mut::<NetConfig>();
            // One guide per driver. Snapshots at 30 Hz: plenty for a map, half the bytes.
            cfg.max_clients = 1;
            cfg.send_interval = 2;
            // The guide stays on its map screen when the link drops (and reconnects).
            cfg.drive_app_state = false;
        }
        if role == Role::Guide {
            // The guide has no menu to sit in: straight to its screen, waiting for the driver.
            app.insert_resource(engine::flow::SkipMenu(true));
        }
        app.add_net_message::<Hello>(Channel::Reliable)
            .add_systems(PostStartup, start_from_args)
            .add_systems(PreUpdate, (track, reconnect, hello).chain().after(engine::net::NetSet::Deliver));
    }
}

/// Sent by each side when the other connects: who's playing (for the leaderboard).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Hello {
    pub name: String,
}

fn hello(mut events: MessageReader<NetEvent>, mut from_guide: MessageReader<FromClient<Hello>>, mut from_driver: MessageReader<FromServer<Hello>>, mut s: ResMut<Session>, mut send: NetSender) {
    for e in events.read() {
        match *e {
            NetEvent::ClientConnected(id) if id != ClientId::HOST => send.to_client(id, &Hello { name: s.name.clone() }),
            NetEvent::Connected(_) => send.to_server(&Hello { name: s.name.clone() }),
            NetEvent::ClientDisconnected(..) if s.guide.is_none() => s.peer_name = None,
            _ => {}
        }
    }
    for m in from_guide.read() {
        if Some(m.client) == s.guide {
            info!("driver: the guide is {:?}", m.message.name);
            s.peer_name = Some(m.message.name.clone());
        }
    }
    for m in from_driver.read() {
        info!("guide: the driver is {:?}", m.0.name);
        s.peer_name = Some(m.0.name.clone());
    }
}

/// Fill in what the engine's `--host` / `--connect` didn't start (`--role` alone).
fn start_from_args(world: &mut World) {
    let role = *world.resource::<Role>();
    match role {
        Role::Driver => {
            if let Some(s) = world.get_resource::<NetServer>() {
                let a = s.endpoint.local_addr();
                let mut ses = world.resource_mut::<Session>();
                ses.addr = Some(a);
                ses.wants = true;
            } else {
                let port = engine::cli::arg_value("host").and_then(|v| v.trim_start_matches(':').parse().ok()).unwrap_or(DEFAULT_PORT);
                match host(world, port) {
                    Ok(a) => info!("driver: hosting on {a}"),
                    Err(e) => error!("driver: can't host on port {port}: {e}"),
                }
            }
        }
        Role::Guide => {
            let addr = engine::cli::arg_value("connect").and_then(|a| net::parse_addr(&a)).unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)));
            if world.contains_resource::<NetClient>() {
                let mut ses = world.resource_mut::<Session>();
                ses.addr = Some(addr);
                ses.wants = true;
            } else if let Err(e) = join(world, addr) {
                error!("guide: can't connect to {addr}: {e}");
            }
        }
        Role::Solo => {}
    }
}

/// Peer connects / drops and RTT.
fn track(time: Res<Time<Real>>, mut events: MessageReader<NetEvent>, mut s: ResMut<Session>, client: Option<Res<NetClient>>, server: Option<Res<NetServer>>) {
    let now = time.elapsed_secs_f64();
    for e in events.read() {
        match *e {
            NetEvent::ClientConnected(id) if id != ClientId::HOST => {
                info!("driver: guide connected ({id})");
                s.guide = Some(id);
                s.changed_at = now;
            }
            NetEvent::ClientDisconnected(id, reason) if s.guide == Some(id) => {
                info!("driver: guide disconnected ({reason:?})");
                s.guide = None;
                s.last_drop = Some(reason);
                s.changed_at = now;
            }
            NetEvent::Connected(_) => {
                info!("guide: connected to the driver");
                s.connected = true;
                s.was_connected = true;
                s.retry_at = None;
                s.changed_at = now;
            }
            NetEvent::Disconnected(reason) => {
                if s.connected {
                    s.changed_at = now;
                }
                s.connected = false;
                if reason != DisconnectReason::Requested {
                    s.last_drop = Some(reason);
                }
            }
            _ => {}
        }
    }
    let rtt = match (client, server) {
        (Some(c), _) => c.endpoint.stats().map(|st| st.rtt_ms),
        (None, Some(srv)) => s.guide.and_then(|g| srv.endpoint.client(g)).map(|c| c.conn.stats().rtt_ms),
        _ => None,
    };
    let rtt = rtt.unwrap_or(0.0);
    if s.rtt_ms != rtt {
        s.rtt_ms = rtt;
    }
}

/// Guide: reconnect after a drop (or a failed first attempt), every [`RETRY_SECS`].
fn reconnect(world: &mut World) {
    if *world.resource::<Role>() != Role::Guide || world.contains_resource::<NetClient>() {
        return;
    }
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    let s = world.resource::<Session>();
    let (wants, addr, retry_at) = (s.wants, s.addr, s.retry_at);
    let Some(addr) = addr.filter(|_| wants) else { return };
    match retry_at {
        None => world.resource_mut::<Session>().retry_at = Some(now + f64::from(RETRY_SECS)),
        Some(t) if now >= t => {
            info!("guide: reconnecting to {addr}");
            let c = world.get_resource::<Connector>().cloned().unwrap_or_else(Connector::udp);
            world.resource_mut::<Session>().retry_at = None;
            if let Err(e) = (c.0)(world, addr) {
                warn!("guide: reconnect failed: {e}");
                world.resource_mut::<Session>().retry_at = Some(now + f64::from(RETRY_SECS));
            }
        }
        _ => {}
    }
}
