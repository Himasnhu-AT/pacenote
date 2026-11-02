# Design sheet: "Talus" rally hatch (player car 1)

An original car, designed and modelled for Pacenote (`tools/art/build_rallycar.py`, no
downloaded parts). It is the body for the tuned sim in `src/vehicle.rs` / `docs/HANDLING.md`:
~1,250 kg, four-wheel drive, 2.55 m wheelbase, 1.56 m track. It is not based on, and must not
be made to look like, any real make or model; it carries no badges, makers' names or sponsor
marks. "Talus" (the scree at the foot of a slope) is a working name only, never written on the
car.

## Silhouette

- **A shoebox on four flared wheels.** Five doors, an upright glasshouse and a near-vertical
  tailgate: the profile is a short sloping bonnet, a long flat roof and a tail cut off square.
- **Arches are the signature:** bolted-on, flat-topped trapezoid flares (not round), matte
  black, standing 4 cm proud of the flat flanks. Their flat tops line up with the bonnet's
  shut line front and back, so from the side the car reads as a box resting on four boxes.
- **Greenhouse:** thin pillars, a blacked-out B-pillar, the rear quarter glass a small
  trapezoid behind the rear door. The windscreen leans back ~55° from vertical.
- **Roof:** a raised scoop (forward-facing intake, black mesh) over the cabin for the crew's
  fresh air, and a two-plane roof-edge wing with flat end plates over the tailgate.
- **Gravel kit:** tall ride height (23 cm under the sills), bash plate under the nose, wide
  mud flaps behind all four wheels, tow loops front and rear, a four-lamp pod for night stages
  (`LightPod`, removable, hidden from the inside cameras).

## Proportions (metres)

| | |
|---|---|
| Length / width over arches / height | 3.98 / 1.82 / 1.46 |
| Wheelbase / track | 2.55 / 1.56 (the sim's mounts, exactly) |
| Overhang front / rear | 0.79 / 0.63 |
| Belt line / roof above ground | 0.93 / 1.46 |
| Tyre | 0.65 m diameter gravel tyre (radius 0.325), 15" six-spoke wheels |

## Era and personality

Late-1990s club rally hatch as it might have been built in a small workshop: flat panels,
square lamps, visible fasteners, nothing aerodynamic that isn't bolted on. It looks
hard-working rather than expensive. Personality: honest, a bit stubborn, happiest sideways
on gravel; a car you fix with cable ties between stages.

## Palette

| Use | Colour (sRGB) |
|---|---|
| Body paint (clearcoat) | slate teal `#1d5c5e` |
| Livery band, roof, mirror caps | chalk `#ece9df` |
| Pinstripe, calipers, tow loops, mud-flap stripe | signal ochre `#f2a01a` |
| Sills, arches, bumper lowers, wing, pod | matte black `#121314` |
| Wheels | anthracite `#3a3c3f` |
| Lamps | clear lenses on chrome reflectors; tail lamps deep red |

## Livery (original and generic)

- A chalk band starts low behind the front arch and climbs to the C-pillar, with an ochre
  pinstripe riding along its top edge; the roof is chalk.
- Black sills and lower bumpers (they take the stone chips).
- Competition number on each front door in a chalk square with a black border (default
  **27**; block digits drawn by the script, readable on both sides), and again on the roof
  for the helicopter.
- A plain black band along the top of the windscreen (no lettering).
- No sponsor names, logos, flags, brand marks or real number fonts. Blank plates front and
  rear.

The livery is painted into a 2048 x 1024 atlas by the script. The paint colour is a separate
parameter of the livery painter (`--paint`), so the body colour can change without redrawing
the stripes; runtime player liveries are future work.

## Cabin

A stripped rally interior (`tools/art/cockpit.py`): padded multi-point cage, two buckets with
six-point harnesses, flat suede-topped dash with a binnacle of two analogue dials (tacho,
speedo, driven by the sim) and shift lights, centre switch panel, a co-driver trip computer and
map light, deep-dish three-spoke wheel, sequential shifter, hydraulic handbrake, drilled
pedals, fire extinguisher, tread-plate footrest. Left-hand drive. The driver's gloved arms are
drawn and animated in code (`src/art/hands.rs`).

## Rig and damage parts

Node names follow `src/art/car.rs`: `Car` > `Chassis` (body, cabin, `DriverBody`,
`SeatHarness`, gauges, levers and the detachable `MirrorL`, `MirrorR`, `RearWing`,
`FrontSplitter`, `FrontBumper`, `RearBumper`, `RearDiffuser`), `Hub*` > `Steer*` (front) >
`Spin*` > `Wheel*`, `Caliper*`; `CoDriverEye`, `CoDriverTablet` anchors.
