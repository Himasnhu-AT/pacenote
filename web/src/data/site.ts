// All visible copy for the Pacenote site lives here.
//
// Facts come from the Pacenote repo: docs/DESIGN.md, HANDOFF.md, docs/ART.md, docs/TELEMETRY.md,
// src/codriver/calls.rs (the co-driver's wording) and src/stage.rs (the 20 stages). Stage
// geometry is src/data/stages.json (examples/stage_dump.rs); numbers are src/data/stats.json
// (tools/telemetry/analyze.py, or the sample from tools/sample_stats.py).
//
// Unknowns are `placeholder: true` and listed in README.md. Replace before launch:
// Steam app id (wishlist + demo links), studio name, production domain, press email,
// full-game price and release date, Discord.

export type Link = { label: string; href: string; placeholder?: true; note?: string };

export const site = {
  name: 'Pacenote',
  // PLACEHOLDER domain: canonical, OG, sitemap and robots read it (also astro.config.mjs).
  url: 'https://pacenote.marlstudio.com',
  urlPlaceholder: true,
  title: 'Pacenote: two people, one rally car',
  description:
    'A two-person rally game: one drives and sees only the road, one reads the map and calls the corners. 20 stages, crash damage, AI co-driver. Free demo on Steam.',
  kind: 'Two-person rally game',
  // PLACEHOLDER studio name.
  studio: { name: 'Marl Studio', placeholder: true as const },
} as const;

// Steam has no app id yet: both links point at Steam's search until it exists.
// Replace `href` with https://store.steampowered.com/app/<APPID>/Pacenote/ (wishlist) and
// steam://install/<DEMO_APPID> or the store page's demo anchor (demo).
export const cta = {
  wishlist: { label: 'Wishlist on Steam', href: 'https://store.steampowered.com/search/?term=Pacenote', placeholder: true },
  demo: { label: 'Play the free demo', href: 'https://store.steampowered.com/search/?term=Pacenote', placeholder: true },
} satisfies Record<string, Link>;

export const nav = [
  { href: '/#roles', label: 'The roles' },
  { href: '/stages/', label: 'Stages' },
  { href: '/stats/', label: 'Numbers' },
] as const;

export const facts = [
  { label: 'Players', value: 'Two: a driver and a guide. Solo with the AI co-driver.' },
  { label: 'Stages', value: '20 fixed stages, 2.6 to 6.0 km' },
  { label: 'Surfaces', value: 'Gravel, tarmac, mud, snow' },
  { label: 'Multiplayer', value: 'LAN, or online through a forwarded port' },
  { label: 'Voice', value: 'Built in, push to talk or open mic' },
  { label: 'Demo', value: 'Free on Steam' },
] as const;

// Unknowns shown honestly where they appear.
export const release = [
  { label: 'Full game release', value: 'Date to be announced', placeholder: true },
  { label: 'Full game price', value: 'To be announced', placeholder: true },
  { label: 'Platforms', value: 'Windows and macOS (Steam), Linux to be confirmed', placeholder: true },
] as const;

export const contact = {
  // PLACEHOLDER press address.
  press: { label: 'press@marlstudio.com', href: 'mailto:press@marlstudio.com', placeholder: true },
  // PLACEHOLDER: no Discord yet. Leave `href` empty to hide it.
  discord: { label: 'Discord', href: '', placeholder: true },
} as const;

export const home = {
  hero: {
    driver: 'Driver sees',
    driverNote: 'The road. No map.',
    guide: 'Guide sees',
    guideNote: 'The map and every corner. No wheel.',
    line: 'Two people, one rally car. One drives blind. One reads the road.',
    stageNote: 'Live on Pinewater, stage 1, from the game’s own road data.',
  },
  roles: {
    title: 'Neither of you finishes alone',
    driver: {
      name: 'The driver',
      text: 'You get the cockpit, the wheel and the stage clock, and nothing else. No minimap, no arrows. The road goes over a crest and you have no idea what is on the other side, unless someone tells you.',
      list: ['Steer, throttle, brake, handbrake', 'Cockpit, bonnet or chase camera', 'Auto or manual gears, pad or keyboard'],
    },
    guide: {
      name: 'The guide',
      text: 'You get the stage map with the car on it, the next 500 metres lit up, and every corner graded from 1 to 6. You can’t touch the car. Your voice is the only thing that keeps it on the road.',
      list: ['Live map, distance to the next corner', 'Corner list: grade, length, crests, narrows', 'Split times, push to talk'],
    },
    solo: {
      name: 'Alone',
      text: 'No guide tonight? The AI co-driver reads the same notes out loud, a few seconds ahead of the car, chained the way a real co-driver would. It tells you about the damage too, and it goes quiet when a human guide joins. N turns it off.',
      transcript: [
        { t: 'Ready. 150 left four long.', k: 'note' },
        { t: '90 left four, into right two, careful.', k: 'note' },
        { t: 'Ouch. The suspension took a knock.', k: 'warn' },
        { t: '80 over crest.', k: 'note' },
        { t: 'Split 1, 1 minute 23 point 4.', k: 'chat' },
        { t: 'Big hit! The engine took a knock.', k: 'urgent' },
        { t: 'That’s it, the steering is gone. We’re out of the stage.', k: 'urgent' },
      ],
      transcriptNote: 'Lines from the co-driver’s script in the game.',
    },
  },
  recce: {
    title: 'Read the road',
    stage: 2, // Kettle Ridge: every grade from hairpin to six
    lead: 'Kettle Ridge, stage 2, drawn from the game’s own road data.',
    leadMotion: 'Scroll to drive it.',
    leadStill: 'Drag the slider to drive it.',
    leadEnd: 'The co-driver’s calls for the corners ahead come up before you reach them, in the same words the game uses.',
    speak: 'Read it to me',
    speakOn: 'Reading out loud',
    speakNote: 'Uses your browser’s voice, not the game’s.',
    gradesTitle: 'How corners are graded',
    grades: [
      { g: 1, word: 'hairpin', note: 'Nearly stop' },
      { g: 2, word: 'two', note: 'Very slow' },
      { g: 3, word: 'three', note: 'Slow' },
      { g: 4, word: 'four', note: 'Medium' },
      { g: 5, word: 'five', note: 'Fast' },
      { g: 6, word: 'six', note: 'Flat out' },
    ],
    gradesNote: 'Tight corners are set narrow and hot; fast ones wide and pale. Plus long, over crest, narrows, careful.',
  },
  stages: {
    title: 'Twenty roads. Same ones for everybody.',
    lead: 'Every stage is generated from a fixed seed, so every team drives the same 20 roads and every time on a leaderboard means something. Five landscapes, four surfaces, from a clear morning to fog at dusk.',
    all: 'All 20 stages',
  },
  damage: {
    title: 'Hit something and you will feel it',
    lead: 'Your car is an original three-door gravel hatchback in white, navy and orange, number 14. Clip a tree and the damage goes where the hit was. A big enough hit ends your run.',
    parts: [
      { name: 'Engine', text: 'Less power, lower top speed.' },
      { name: 'Steering', text: 'The car pulls to one side. Keep correcting.' },
      { name: 'Suspension', text: 'Softer, slower, less grip in the bumps.' },
    ],
    retire: 'Break one part completely and you are out of the stage: retired.',
    recover: 'Off the road but still in one piece? Recover puts you back on it for 5 seconds.',
  },
  numbers: {
    title: 'The stages, by the numbers',
    lead: 'Anonymous stats from people playing the demo. Which corners end the most runs, how many finish, and whether the co-driver helps.',
    more: 'All the numbers',
  },
  gallery: { title: 'In the game' },
  faq: {
    title: 'Questions',
    items: [
      { q: 'What does the demo include?', a: 'The demo is free on Steam. It has the driver, the guide and the AI co-driver, with 3 of the 20 stages: Pinewater (gravel), Thornback (tarmac) and Frostline (snow). Your wishlist is what tells Steam people want the full game.', placeholder: true },
      { q: 'Which platforms?', a: 'Windows and macOS on Steam. Linux is to be confirmed.', placeholder: true },
      { q: 'How much is the full game?', a: 'The demo is free. Price and release date of the full game are still to be announced.', placeholder: true },
      { q: 'How do we play together?', a: 'The driver hosts and the guide joins with an address. That works on the same network (LAN) straight away. Over the internet the driver needs to forward a port on their router, because there is no relay server yet. Voice chat is built in, so you don’t need another app.' },
      { q: 'Can I play alone?', a: 'Yes. Drive solo and the AI co-driver reads the pace notes out loud. It uses your system’s voice.' },
      { q: 'Controller or keyboard?', a: 'Both. A gamepad gives you analogue steering, throttle and brake. On a keyboard the game helps with the steering.' },
      { q: 'Is there a leaderboard?', a: 'Every stage has its own board and a ghost of your team’s best run. Boards are stored on your machine and merge when two teams connect. A shared online board is planned.' },
      { q: 'Does the game collect data?', a: 'Anonymous gameplay stats, on by default: stage, finish or retire, time, where crashes happened. No names, no chat, no voice. Turn it off in Settings, Profile, “Share anonymous stats”. Everything it sends is listed on the privacy page.', link: { href: '/privacy/', label: 'What is sent' } },
    ],
  },
  finale: {
    title: 'Find a guide.',
    text: 'Download the free demo, hand someone the map and see how far you get. If you want the full game, a wishlist is the best way to tell us.',
  },
} as const;


export const notFound = {
  title: 'We’re off the road.',
  text: 'Get back on, or press R.',
  note: 'Nothing at this address. The co-driver says that line in the game when you leave the road.',
  back: 'Back to the road',
};

export const privacy = {
  title: 'Privacy',
  lead: 'What the Pacenote demo and game send, and how to turn it off. This site itself sets no cookies and loads no trackers.',
  summary: [
    'Gameplay stats and crash reports are anonymous and on by default. The first launch explains them, and nothing is sent until you close that notice.',
    'Turn them off in Settings, Privacy: “Share anonymous stats” and “Send crash reports”. “Forget me” there deletes what was collected.',
    'Setting DO_NOT_TRACK=1 or TRACER_DISABLED=1 in your environment turns it all off: nothing is written or sent.',
    'No names, no chat, no voice, no addresses or lobby codes, no location from your IP address.',
  ],
  ids: 'The only ids are a random install id and a random session id, made on your machine. Neither is linked to your name or your Steam account.',
  device: 'Device info: operating system, CPU architecture, CPU name and core count, memory size and locale. Crash and freeze reports add where in the code it happened and the last lines of the game’s log; performance reports carry frame times, loading time and memory use.',
  where: 'Data goes to our own server at api.marlstudio.com. A build without the server address compiled in sends nothing.',
  use: 'We use it to balance the stages, fix crashes, and publish totals like “stage 7’s hairpin ended 1 in 3 runs”. We publish totals only, never single runs.',
  events: [
    { name: 'session.settings', when: 'Once per session', props: 'Camera view, speed units, steering speed, traction control and ABS on or off, co-driver options, graphics quality, demo or full game' },
    { name: 'run.start', when: 'The countdown ends', props: 'Stage, solo or team, co-driver on or off, camera, weather, time of day, traction control and ABS on or off, demo or full game' },
    { name: 'run.end', when: 'The run ends', props: 'Stage, result (finish, retired, quit, restart), time with penalties, penalty seconds, metres driven, number of hits, engine, steering and suspension damage, recovers, co-driver on or off, demo or full game' },
    { name: 'run.crash', when: 'A hit hard enough to matter', props: 'Stage, metres along the road, which corner and its grade, how hard, speed, which side of the car, whether it ended the run' },
    { name: 'run.recover', when: 'You press recover', props: 'Stage, metres along the road' },
    { name: 'play_together.connect', when: 'Hosting or joining a Play together session', props: 'Host or join, whether it connected (or timed out, was refused, failed), the round-trip time in ms' },
    { name: 'play_together.end', when: 'A Play together session ends', props: 'Host or join, how long it lasted, why it ended (left, partner left, timeout, error)' },
    { name: 'demo.store_click', when: 'You press the store button in the demo', props: 'Which button (main menu or end of a run), which store (itch.io or Steam)' },
  ],
  // PLACEHOLDER contact.
  contact: 'Questions or a deletion request: write to us at',
  updated: '31 October 2026',
} as const;

// ---------- Images (in-game captures, src/assets/shots; raw PNGs in _source-shots) ----------
export type ShotRef = { src: string; alt: string; caption?: string };

export const shots: { hero: ShotRef; driver: ShotRef; guide: ShotRef | null; crash: ShotRef; car: ShotRef & { caption: string } } = {
  hero: { src: 'hero-cockpit', alt: 'The driver’s view on Pinewater at 68 km/h: a white roll cage frames a straight gravel road between tall pines, the dash with two lit gauges below.' },
  driver: { src: 'driver-frostline', alt: 'Cockpit view on Frostline: snow-covered spruce on both sides of a white road, the steering wheel and gauges in front.' },
  guide: { src: 'guide-view', alt: 'The guide’s screen during a run on Pinewater: a top-down map with the car’s position and the next stretch lit in orange, corner tags like L4 and R5 along the road, and a panel with the stage clock, speed, the next call Right 5 at 20 m, and the coming-up list Left 4, Left 5, Right 3, Left 5, Right 1, Left 2.' },
  crash: { src: 'crash-pinewater', alt: 'Chase camera after a crash on Pinewater: the white rally car off the road among pine trees, the subtitle reads Big hit! The engine took a knock, and a damage readout shows engine 32%, steering 27%, suspension 14%.' },
  car: { src: 'car-sunscar', alt: 'The rally car from behind on Sunscar’s orange gravel, kicking up dust under a clear sky, scrub trees on the ridge.', caption: 'Car 14 on Sunscar, stage 5.' },
};

export const gallery: (ShotRef & { caption: string })[] = [
  { src: 'frostline', alt: 'The car on a snowy road through spruce forest on Frostline, overcast noon.', caption: 'Frostline, stage 9. Snow, overcast.' },
  { src: 'low-fen', alt: 'The car on a muddy road in thick fog on Low Fen, birch trees fading into grey.', caption: 'Low Fen, stage 4. Mud in the fog.' },
  { src: 'dust-devil', alt: 'The car on orange gravel at dusk on Dust Devil, the low sun behind scrub trees and long shadows across the road.', caption: 'Dust Devil, stage 15. Gravel at dusk.' },
  { src: 'bramble', alt: 'The car on wet, shining tarmac in the rain on Bramble, birch and pine on both sides.', caption: 'Bramble, stage 14. Wet tarmac.' },
  { src: 'pinewater-bonnet', alt: 'Bonnet camera on Pinewater: the white, navy and orange bonnet in front of a rutted forest road.', caption: 'Pinewater from the bonnet camera.' },
  { src: 'main-menu', alt: 'Pacenote’s main menu: the title over the line One drives. One reads the road, a menu with Practice, Host as driver, Join as guide, Stage select and Leaderboards, and Pinewater drawn over gold contour lines.', caption: 'The main menu. Host as driver, join as guide, or practise alone.' },
];

// One capture per stage page (stage id → shot). A stage without one shows the map alone.
const at = (src: string, name: string, alt: string) => ({ src, alt, caption: `${name} in the game.` });

// One capture per stage page (stage id → shot). Stages without one show the map alone.
export const stageShots: Record<number, ShotRef & { caption: string }> = {
  1: at('pinewater', 'Pinewater', 'The car on Pinewater’s rutted forest road in morning light.'),
  2: at('kettle-ridge', 'Kettle Ridge', 'The car on a grey gravel road through pines on Kettle Ridge, overcast afternoon.'),
  3: at('birchmoor', 'Birchmoor', 'The car on a dirt road across open grass on Birchmoor, a lone pine on the right, clear noon.'),
  4: at('low-fen', 'Low Fen', 'The car on a muddy road in thick fog on Low Fen.'),
  5: at('sunscar', 'Sunscar', 'The car on dry orange gravel on Sunscar under a blue sky.'),
  6: at('thornback', 'Thornback', 'The car on dark tarmac between orange scrubland on Thornback, an olive tree by the road.'),
  7: at('high-cairn', 'High Cairn', 'The car on a grey gravel road over the moor on High Cairn at dusk.'),
  8: at('grey-hollow', 'Grey Hollow', 'The car on a wet road across open moor in the rain on Grey Hollow.'),
  9: at('frostline', 'Frostline', 'The car on a snowy road through spruce forest on Frostline.'),
  10: at('white-spruce', 'White Spruce', 'The car on a packed snow road between snowy spruce on White Spruce, long blue shadows across the road.'),
  11: at('long-drop', 'Long Drop', 'The car throwing up dust on a forest road on Long Drop, afternoon sun.'),
  12: at('saw-mill', 'Saw Mill', 'The car on tarmac through dark pines at dusk on Saw Mill.'),
  13: at('meadow-run', 'Meadow Run', 'The car on a gravel road between birch trees on Meadow Run, overcast.'),
  14: at('bramble', 'Bramble', 'The car on wet tarmac in the rain on Bramble.'),
  15: at('dust-devil', 'Dust Devil', 'The car on orange gravel at dusk on Dust Devil, the low sun behind scrub trees.'),
  16: at('red-rocks', 'Red Rocks', 'The car on red gravel climbing through scrub on Red Rocks.'),
  17: at('moorgate', 'Moorgate', 'The car on a gravel road across the moor in fog on Moorgate.'),
  18: at('tor-crossing', 'Tor Crossing', 'The car on grey tarmac across misty moorland on Tor Crossing, overcast morning.'),
  19: at('iceback', 'Iceback', 'The car on a snowy road through fog and spruce at dusk on Iceback.'),
  20: at('silent-pines', 'Silent Pines', 'The car spraying snow on a road through snowy pines on Silent Pines, clear morning.'),
};

export const biomes = {
  PineForest: 'Tall pines, birch at the edges, low sun through the trunks.',
  BirchHills: 'Rolling hills and white birch. Fog and rain live here.',
  Scrub: 'Dry, hot, wide open. Orange gravel and olive scrub.',
  Moor: 'Treeless highland: rock, heather and long sightlines, until the fog comes in.',
  SnowForest: 'Spruce under snow. Grip is a rumour.',
} as const;
