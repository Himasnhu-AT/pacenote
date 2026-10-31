//! Prints the bits the game's determinism depends on: the scripted drive's hash
//! (`vehicle::check`) and all 20 stage road hashes (as `meta::stages::stage_hash`). Run it
//! for two CPUs and compare: `tools/simcheck/run.sh`.

#[path = "../../../src/stage.rs"]
#[allow(dead_code)]
mod stage;
#[path = "../../../src/vehicle.rs"]
#[allow(dead_code)]
mod vehicle;

/// `meta::stages::stage_hash`'s hasher.
struct Hasher(u64);

impl Hasher {
    fn u64(&mut self, v: u64) {
        self.0 = pcg_core::mix64(self.0 ^ v.wrapping_mul(0xA24B_AED4_963E_E407));
    }
    fn f32(&mut self, v: f32) {
        self.u64(v.to_bits() as u64);
    }
}

fn main() {
    println!("vehicle {:016x}", vehicle::check::scripted_hash());
    for def in stage::stages() {
        let road = stage::generate_road(&def);
        let mut h = Hasher(0x9E37_79B9_7F4A_7C15);
        h.u64(def.id as u64);
        h.u64(def.seed);
        h.f32(def.length);
        h.u64(road.len() as u64);
        for i in 0..road.len() {
            let p = road.pts[i];
            h.f32(p.x);
            h.f32(p.y);
            h.f32(p.z);
            h.f32(road.width[i]);
            h.f32(road.bank[i]);
            h.u64(road.surface[i] as u64);
        }
        println!("stage {:2} {:016x}", def.id, h.0);
    }
}
