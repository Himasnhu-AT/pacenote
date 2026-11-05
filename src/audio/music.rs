//! Music for the Service Park menus and the loading screen.
//!
//! The tracks are recorded music from Openverse (CC0 / CC BY, credited in `assets/credits.txt`),
//! picked by ear and pinned in `tools/audio/sfx_wishlist.json` (`menu_music`, `loading_music`),
//! cooked by `tools/audio/cook_sfx.py` into `assets/sounds/music/` (stereo Vorbis, -18 LUFS).
//!
//! - **Menus:** a shuffled playlist; each song crossfades into the next a few seconds before it ends.
//! - **Loading screen:** a calmer song, crossfaded in from the menu.
//! - **Driving:** no music. It fades out over 2.5 s as the countdown starts, so the car and the
//!   world's ambience take over (pillar: "driving that feels earned").
//!
//! Playback is the engine's [`Music`] player on the `music` bus (Settings › Audio › Music;
//! the pause snapshot ducks it). A song is one looping stem. Memory: a decoded stereo song is
//! ~75-100 MB, so only the playing song and the one fading in are loaded; both are released
//! once the music has faded out for a drive.

use bevy::prelude::*;
use engine::flow::AppState;
use engine::sound::{Music, MusicStem, MusicTrack, MusicTransition, Quantize, SoundClip};

/// Menu playlist (`assets/sounds/music/`).
pub const MENU: [&str; 3] = ["menu_music_1", "menu_music_2", "menu_music_3"];
/// Loading-screen songs.
pub const LOADING: [&str; 3] = ["loading_music_1", "loading_music_2", "loading_music_3"];
/// Two track slots in the engine's player, used in turn so a song can crossfade into the next.
const SLOTS: [&str; 2] = ["pacenote.music.a", "pacenote.music.b"];
/// Crossfade between songs (s).
const XFADE_IN: f32 = 2.0;
const XFADE_OUT: f32 = 2.5;
/// Start the next menu song this long before the current one ends (s).
const LEAD_OUT: f32 = 4.0;
/// Track level (dB): the songs are mastered at -18 LUFS; under the menus they are a bed, so the
/// UI cues (`audio::ui`) sit about 6 dB over them (measured: select/back +6, hover +1).
pub const MUSIC_DB: f32 = -6.0;
/// Fade out when a drive starts (s); the songs are dropped after this.
const QUIET_FADE: f32 = 2.5;

/// What should be playing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    Menu,
    Loading,
    Quiet,
}

/// Menus play the playlist, the loading screen its own song, anything else is quiet.
pub fn want(state: AppState, loading: bool) -> Want {
    if loading {
        Want::Loading
    } else if state == AppState::MainMenu {
        Want::Menu
    } else {
        Want::Quiet
    }
}

/// A shuffled order of `n` songs from `seed` (Fisher-Yates on a small LCG): not the sim, so
/// any seed will do; the wall clock gives a different order each launch.
pub fn shuffle(n: usize, seed: u64) -> Vec<usize> {
    let mut order: Vec<usize> = (0..n).collect();
    let mut s = seed | 1;
    for i in (1..n).rev() {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let j = ((s >> 33) % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    order
}

#[derive(Resource)]
struct Player {
    /// What the music is doing now.
    now: Want,
    /// Slot of the playing song.
    slot: usize,
    /// A song being loaded, and what it's for.
    pending: Option<(Handle<SoundClip>, Want)>,
    /// When the playing song started (real seconds) and how long it is.
    started: f64,
    length: f32,
    menu_order: Vec<usize>,
    menu_next: usize,
    loading_order: Vec<usize>,
    loading_next: usize,
    /// Real time the music went quiet (to release the songs after the fade).
    quiet_since: Option<f64>,
    /// Songs still held by the slots.
    holding: bool,
}

impl Player {
    fn new(seed: u64) -> Self {
        Player {
            now: Want::Quiet,
            slot: 0,
            pending: None,
            started: 0.0,
            length: 0.0,
            menu_order: shuffle(MENU.len(), seed),
            menu_next: 0,
            loading_order: shuffle(LOADING.len(), seed.rotate_left(17)),
            loading_next: 0,
            quiet_since: None,
            holding: false,
        }
    }

    /// The next song for `w` (advances its playlist).
    fn next_song(&mut self, w: Want) -> Option<&'static str> {
        match w {
            Want::Menu => {
                let i = self.menu_order[self.menu_next % MENU.len()];
                self.menu_next += 1;
                Some(MENU[i])
            }
            Want::Loading => {
                let i = self.loading_order[self.loading_next % LOADING.len()];
                self.loading_next += 1;
                Some(LOADING[i])
            }
            Want::Quiet => None,
        }
    }
}

pub struct MusicPlugin;

impl Plugin for MusicPlugin {
    fn build(&self, app: &mut App) {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(7, |d| d.as_nanos() as u64);
        app.insert_resource(Player::new(seed)).add_systems(Update, drive);
    }
}

fn drive(
    mut player: ResMut<Player>,
    state: Option<Res<State<AppState>>>,
    loading: Option<Res<crate::loading::Loading>>,
    music: Option<ResMut<Music>>,
    assets: Option<Res<AssetServer>>,
    clips: Option<Res<Assets<SoundClip>>>,
    time: Res<Time<Real>>,
) {
    // No sound system (headless, tests): nothing to do.
    let (Some(state), Some(mut music), Some(assets), Some(clips)) = (state, music, assets, clips) else { return };
    let now = time.elapsed_secs_f64();
    let w = want(*state.get(), loading.is_some());

    if w == Want::Quiet {
        if player.now != Want::Quiet {
            music.stop(QUIET_FADE);
            player.now = Want::Quiet;
            player.pending = None;
            player.quiet_since = Some(now);
        }
        // Release the decoded songs once they've faded.
        if player.holding && player.quiet_since.is_some_and(|t| now - t > QUIET_FADE as f64 + 0.5) {
            for s in SLOTS {
                music.add_track(MusicTrack::new(s, 120.0));
            }
            player.holding = false;
        }
        return;
    }
    player.quiet_since = None;

    // A new mode, or the menu song is about to end: load the next song.
    let pending_for = player.pending.as_ref().map(|(_, p)| *p);
    let song_ending = player.now == Want::Menu && player.length > 0.0 && now - player.started > (player.length - LEAD_OUT).max(1.0) as f64;
    if pending_for != Some(w) && (player.now != w || song_ending) {
        if let Some(song) = player.next_song(w) {
            let h: Handle<SoundClip> = assets.load(format!("sounds/music/{song}.ogg"));
            player.pending = Some((h, w));
        }
    }

    // Start the pending song once it has decoded.
    let Some((h, for_want)) = player.pending.clone() else { return };
    if let Some(clip) = clips.get(&h) {
        let slot = if player.holding { 1 - player.slot } else { player.slot };
        music.add_track(MusicTrack::new(SLOTS[slot], 120.0).volume(MUSIC_DB).stem(MusicStem::always(h)));
        music.play(SLOTS[slot], MusicTransition { fade_out: XFADE_OUT, fade_in: XFADE_IN, quantize: Quantize::Immediate });
        player.slot = slot;
        player.holding = true;
        player.started = now;
        player.length = clip.duration();
        player.now = for_want;
        player.pending = None;
    } else if assets.load_state(&h).is_failed() {
        warn!("music: can't load {:?}", assets.get_path(&h));
        // Don't retry every frame: count it as playing (silently) for its mode.
        player.now = for_want;
        player.length = 0.0;
        player.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menus_play_the_playlist_loading_its_song_driving_is_quiet() {
        assert_eq!(want(AppState::MainMenu, false), Want::Menu);
        assert_eq!(want(AppState::InGame, true), Want::Loading);
        assert_eq!(want(AppState::InGame, false), Want::Quiet);
        assert_eq!(want(AppState::Boot, false), Want::Quiet);
    }

    #[test]
    fn a_shuffle_plays_every_song_once() {
        for seed in 0..50 {
            let mut o = shuffle(MENU.len(), seed);
            o.sort();
            assert_eq!(o, vec![0, 1, 2]);
        }
        // Different launches start on different songs.
        let firsts: std::collections::HashSet<usize> = (0..50).map(|s| shuffle(3, s * 7919)[0]).collect();
        assert!(firsts.len() > 1);
    }

    #[test]
    fn the_playlist_cycles_through_all_songs() {
        let mut p = Player::new(42);
        let songs: Vec<_> = (0..6).map(|_| p.next_song(Want::Menu).unwrap()).collect();
        assert_eq!(songs[0..3].iter().collect::<std::collections::HashSet<_>>().len(), 3);
        assert_eq!(songs[0..3], songs[3..6]);
        assert!(LOADING.contains(&p.next_song(Want::Loading).unwrap()));
        assert_eq!(p.next_song(Want::Quiet), None);
    }

    #[test]
    fn every_song_is_shipped() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/sounds/music");
        for s in MENU.iter().chain(LOADING.iter()) {
            assert!(dir.join(format!("{s}.ogg")).exists(), "missing {s}.ogg");
        }
    }
}
