# torqa-cli

Headless tool to find and ride smart trainers without the app. Useful for testing trainers and
sensors.

## Install (macOS)

Download the `torqa-cli-macos-arm64.tar.gz` artifact from a CI run, then:

```sh
tar -xzf torqa-cli-macos-arm64.tar.gz
xattr -d com.apple.quarantine torqa-cli   # unsigned binary
./torqa-cli --help
```

On first use macOS asks whether your terminal app may use Bluetooth — allow it
(System Settings → Privacy & Security → Bluetooth).

## Find devices

```sh
./torqa-cli scan               # scans 5 s for FTMS trainers and heart-rate sensors
./torqa-cli scan --seconds 10
```

Wake the trainer by pedalling and make sure no other app (Zwift, Wahoo app) is connected to it —
FTMS allows only one controlling app. Heart-rate straps usually only advertise while worn (moisten
the electrodes) and while not connected to a watch or phone.

## Ride

```sh
./torqa-cli ride                       # strongest trainer found
./torqa-cli ride --trainer kickr --hr  # trainer by name + strongest heart-rate strap
./torqa-cli ride --hr polar            # heart-rate strap by name
./torqa-cli ride --fake                # simulated trainer, no hardware
./torqa-cli ride --fake --fake-power 250 --fake-cadence 95   # the simulated rider's output
```

| Option | Meaning |
|---|---|
| `--trainer kickr` | Trainer by name (part of it, any case); default the strongest signal |
| `--hr [name]` | Also connect a heart-rate strap, by name or the strongest |
| `--fake` | The simulated trainer instead of Bluetooth (with `--hr`, a real strap still connects) |
| `--fake-power 200`, `--fake-cadence 90` | What the simulated rider pedals, in W and rpm |
| `--scan-seconds 5` | How long to scan for the trainer and strap |

Live readings are printed every second. Without a route, type a command and press Enter:

| Command | Effect |
|---|---|
| `g 5` | SIM mode, 5 % grade (negative for descents) |
| `p 200` | ERG mode, hold 200 W |
| `r 30` | Resistance at 30 % of the trainer's range |
| `q` | Quit and disconnect (Ctrl+C works too; press it twice to skip the disconnect) |

If no heart-rate strap is found, the ride continues without one. Wahoo trainers estimate cadence
from the flywheel, so it may read 0 for the first seconds or at very low power.

## Ride a route

```sh
./torqa-cli route my-ride.gpx                     # length, climbing, elevation source, map
./torqa-cli route my-ride.gpx --world             # also build its 3D world: size and timings
./torqa-cli ride --route my-ride.gpx --hr         # ride it; the trainer follows the gradient
./torqa-cli ride --route my-ride.gpx --difficulty 100 --descent flat --mass 90
./torqa-cli ride --route my-ride.gpx --fake --time-scale 50   # quick simulated test ride
```

Like an import in the app, the track is put onto the roads it rides (OpenStreetMap) and its
elevations come from a terrain model (corrected and smoothed); bridges and tunnels of the roads
ridden run straight between their ends. Map and terrain data are downloaded once and cached, so
a route you have imported before also works offline (`--offline` forces cache-only). Without
terrain data, the GPX elevations are used; without map data, the track stays as recorded and
bridges and tunnels follow the ground (short dips or humps).

| Option | Meaning |
|---|---|
| `--difficulty 50` | Share of the road gradient you feel on the trainer (speed always uses the real gradient) |
| `--descent coast\|flat` | Coast: gravity builds speed downhill. Flat: descents ride like flat roads |
| `--mass 83` | Rider plus bike in kg |
| `--output ride.fit` | Where to save the activity (default `torqa-<date>-<time>.fit`) |
| `--offline` | Use cached map and terrain data only (also for `route`) |
| `--time-scale 50` | Run simulated time faster; only with `--fake` |

On a route the trainer follows the gradient, so `g`, `p` and `r` do not apply: the ride starts
when the trainer connects and ends at the finish or with `q` / Ctrl+C; the FIT
file can be uploaded to Strava, intervals.icu, Garmin Connect and others as a virtual ride.

Map data: © [OpenFreeMap](https://openfreemap.org) © [OpenMapTiles](https://openmaptiles.org),
data © [OpenStreetMap contributors](https://www.openstreetmap.org/copyright) (ODbL).
Terrain data: [Mapterhorn](https://mapterhorn.com/attribution) (CC BY 4.0) and
[AWS Terrain Tiles](https://registry.opendata.aws/terrain-tiles/).

## Connection

If the trainer drops out, the CLI reconnects automatically and re-applies the last command.
Set `RUST_LOG=debug` for protocol details (accepted/rejected commands, resistance range).
