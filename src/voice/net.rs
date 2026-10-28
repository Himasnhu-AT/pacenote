//! Voice over the engine net session: one unreliable typed message per 20 ms frame.
//!
//! - Host (driver, listen server): its frames go to every client
//!   (`broadcast_except(HOST)`, so the host never hears itself); frames from a client are
//!   played and relayed to the other clients (spectators later).
//! - Client (guide): frames go `to_server`; frames from the server are played.
//! - The server stamps a client's frames with its `ClientId`, so a client can't speak as
//!   someone else. Talker 0 is the host.
//!
//! This is the only file that knows about the engine's net API; [`local_role`] is the one
//! place that decides who is driver and who is guide.

use bevy::prelude::*;
use engine::net::{ClientId, FromClient, FromServer, NetClient, NetRole, NetSender, NetServer};
use serde::{Deserialize, Serialize};

use super::adpcm::FRAME_BYTES;
use super::playback::Packet;

/// One 20 ms voice frame on the wire (~175 bytes with postcard + packet headers).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoiceFrame {
    /// Who is talking (`ClientId`, 0 = host). Overwritten by the server for client frames.
    pub talker: u32,
    /// Counts sent frames (loss statistics).
    pub seq: u32,
    /// Capture frame index: +1 per 20 ms of microphone time, sent or not (playout timing).
    pub ts: u32,
    /// IMA ADPCM frame ([`FRAME_BYTES`]).
    pub data: Vec<u8>,
}

/// Which seat this instance has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum VoiceRole {
    /// Not in a session: no capture, no playback.
    #[default]
    Offline,
    Driver,
    Guide,
}

/// Driver hosts, guide connects (docs/DESIGN.md). Swap this for the session's role resource
/// once it exists.
pub fn local_role(role: NetRole) -> VoiceRole {
    match role {
        NetRole::Offline => VoiceRole::Offline,
        NetRole::Server | NetRole::ListenServer => VoiceRole::Driver,
        NetRole::Client => VoiceRole::Guide,
    }
}

/// Our talker id in the session (`None` until a client has its id).
pub fn local_talker(role: NetRole, client: Option<&NetClient>) -> Option<u32> {
    match role {
        NetRole::Server | NetRole::ListenServer => Some(ClientId::HOST.0),
        NetRole::Client => client.and_then(NetClient::id).map(|c| c.0),
        NetRole::Offline => None,
    }
}

/// Frames encoded this frame, waiting for [`send_frames`].
#[derive(Resource, Default)]
pub struct VoiceOutbox {
    pub frames: Vec<VoiceFrame>,
}

/// Frames received this frame (filled by [`recv_frames`], drained by playback).
#[derive(Resource, Default)]
pub struct VoiceInbox {
    pub packets: Vec<Packet>,
    /// Frames received in total / with a bad size.
    pub received: u64,
    pub malformed: u64,
}

/// Counters for `voice.status`.
#[derive(Resource, Default, Clone, Copy, Debug, Serialize)]
pub struct VoiceNetStats {
    pub sent: u64,
    pub relayed: u64,
    pub sent_bytes: u64,
}

pub fn send_frames(mut outbox: ResMut<VoiceOutbox>, role: Res<NetRole>, mut tx: NetSender, mut stats: ResMut<VoiceNetStats>) {
    for f in outbox.frames.drain(..) {
        match *role {
            NetRole::Server | NetRole::ListenServer => tx.broadcast_except(ClientId::HOST, &f),
            NetRole::Client => tx.to_server(&f),
            NetRole::Offline => continue,
        }
        stats.sent += 1;
        stats.sent_bytes += f.data.len() as u64 + 12;
    }
}

fn packet(talker: u32, f: &VoiceFrame, at_ms: f64) -> Option<Packet> {
    let data: [u8; FRAME_BYTES] = f.data.as_slice().try_into().ok()?;
    Some(Packet { talker, ts: f.ts, at_ms, data })
}

#[allow(clippy::too_many_arguments)]
pub fn recv_frames(
    mut from_clients: MessageReader<FromClient<VoiceFrame>>,
    mut from_server: MessageReader<FromServer<VoiceFrame>>,
    role: Res<NetRole>,
    server: Option<Res<NetServer>>,
    client: Option<Res<NetClient>>,
    time: Res<Time<Real>>,
    mut inbox: ResMut<VoiceInbox>,
    mut tx: NetSender,
    mut stats: ResMut<VoiceNetStats>,
) {
    let now = time.elapsed_secs_f64() * 1000.0;
    let me = local_talker(*role, client.as_deref());
    for m in from_clients.read() {
        // The host's own `to_server` loops back as FromClient(HOST) on a listen server: ignore.
        if m.client == ClientId::HOST {
            continue;
        }
        let mut f = m.message.clone();
        f.talker = m.client.0;
        inbox.received += 1;
        match packet(f.talker, &f, now) {
            Some(p) => inbox.packets.push(p),
            None => {
                inbox.malformed += 1;
                continue;
            }
        }
        if let Some(s) = &server {
            for c in s.endpoint.clients() {
                if c.id != m.client {
                    tx.to_client(c.id, &f);
                    stats.relayed += 1;
                }
            }
        }
    }
    for FromServer(f) in from_server.read() {
        if Some(f.talker) == me {
            continue;
        }
        inbox.received += 1;
        match packet(f.talker, f, now) {
            Some(p) => inbox.packets.push(p),
            None => inbox.malformed += 1,
        }
    }
}
