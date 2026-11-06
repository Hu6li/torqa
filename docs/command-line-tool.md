# Command-line tool

`orqwa-cli` finds and rides smart trainers without the app — handy for testing trainers and
sensors.

## Install

Download the `orqwa-cli` archive for your system from
[Releases](https://github.com/ORQWA/ORQWA/releases) and unpack it. Between
releases, use a successful [CI run on main](https://github.com/ORQWA/ORQWA/actions/workflows/ci.yml?query=branch%3Amain)
(sign in to download workflow artifacts). Release archives carry their version
(`orqwa-cli-0.5.2-linux-x86_64.tar.gz`); the CI run's are named without it:

| System | Download | Notes |
|---|---|---|
| macOS | `orqwa-cli-<version>-macos-arm64.tar.gz` | Unsigned: run `xattr -d com.apple.quarantine orqwa-cli`, and allow your terminal Bluetooth access |
| Windows | `orqwa-cli-<version>-windows-x86_64.zip` | Run `.\orqwa-cli.exe` in PowerShell; Bluetooth must be on |
| Linux | `orqwa-cli-<version>-linux-<arch>.tar.gz` | glibc 2.35+ and the ALSA runtime (`libasound.so.2`); the `bluetooth` service running, adapter on (`bluetoothctl power on`) |

## Check group-ride networking

Before riding with others, check the address friends would reach you at. Ordinary rides
contact none of these services.

```sh
./orqwa-cli net-check
./orqwa-cli net-check --lan --no-stun
./orqwa-cli net-check --loopback
./orqwa-cli net-check --interface en0
```

The first asks public address lookup services (STUN) which internet address they see for you.
`--lan --no-stun` checks for riders on one home network without asking them, `--loopback`
checks only this computer, and `--interface` uses one network connection, by its system name,
instead of the default. `--stun stun.example.org` asks your own STUN server instead of the
public ones, as `name` or `name:port`; repeat it for up to four. A result does not guarantee
that a friend can connect: a firewall can still block it. See [when ORQWA needs the internet](https://github.com/ORQWA/ORQWA/wiki/Getting-started#when-orqwa-needs-the-internet).

To test a connection with a friend, one of you runs:

```sh
./orqwa-cli net-ping host
```

Send the printed invite privately to your friend, who runs `./orqwa-cli net-ping join 'orqwa:…'`
with the whole invite in quotes and sends the printed reply back privately. Paste the reply
into the host's terminal and press Enter. A reply works for one minute; after that, your friend
runs the join again for a fresh one. A working connection prints ten ping times. Type `q` and
Enter, or press Ctrl+C, to close it.

On one home network, both add `--lan --no-stun`; in two terminals on one computer, `--loopback`.
The other options above work with `net-ping` too. Both computers' firewalls must allow ORQWA's
UDP traffic, and some routers cannot connect directly.

Keep invites and replies out of public posts, logs and bug reports: they reveal your name and
addresses. Only paste replies from people you invited. After sixteen replies, start the host
again for a fresh invite. The check does not start a trainer or record a ride.

## Find devices

```sh
./orqwa-cli scan               # 5 s scan for trainers, sensors and shifters
./orqwa-cli scan --seconds 10
```

Wake the trainer by pedalling and disconnect other apps (Zwift, Wahoo): only one app can control
it. Heart-rate straps usually only show up while worn.

## Ride

```sh
./orqwa-cli ride                       # strongest trainer found
./orqwa-cli ride --trainer kickr --hr  # trainer by name, plus a heart-rate strap
./orqwa-cli ride --fake                # simulated trainer, no hardware
```

Readings print every second. Type a command and press Enter:

| Command | Effect |
|---|---|
| `g 5` | Simulate a 5 % grade |
| `p 200` | Hold 200 W (ERG) |
| `r 30` | Resistance at 30 % |
| `q` | Quit (or Ctrl+C) |

<details>
<summary>All ride options</summary>

| Option | Meaning |
|---|---|
| `--trainer kickr` | Trainer by name (any part, any case) |
| `--hr [name]` | Also connect a heart-rate strap |
| `--fake` | Simulated trainer with a simulated heart rate |
| `--fake-power 200`, `--fake-cadence 90` | What the simulated rider pedals |
| `--controller [name]` | Also connect a shifter: a Shimano Di2 or a Zwift Click (original or v2) |
| `--up-channel 1`, `--down-channel 2` | The buttons that shift up and down: Di2 channels, or a Click's + (1) and − (2); a Click v2's Y, Z, A, B are 3–6, its arrows 7–10 |
| `--scan-seconds 5` | How long to scan |

</details>

## Ride a route

```sh
./orqwa-cli route my-ride.gpx                  # length, climbing, map
./orqwa-cli ride --route my-ride.gpx --hr      # the trainer follows the gradient
./orqwa-cli ride --route my-ride.gpx --fake --time-scale 50   # quick test ride
```

The route is prepared as in the app; map and terrain data are cached, so routes work offline
afterwards. The ride is saved as a FIT file.

<details>
<summary>All route options</summary>

| Option | Meaning |
|---|---|
| `--difficulty 50` | Share of the gradient you feel on the trainer |
| `--descent coast\|flat` | Coast downhill, or ride descents like flat roads |
| `--mass 83` | Rider plus bike in kg |
| `--output ride.fit` | Where to save the ride |
| `--offline` | Use cached data only |
| `--time-scale 50` | Faster simulated time (with `--fake`) |
| `--gears 50x14` | Virtual gears on a single cog; `u` / `d` and Enter shift |
| `--world` | (`route` only) also build the 3D world and report its size |

</details>

## Ride with others

Up to eight riders can ride together, each on their own trainer. Everyone needs the same GPX
file on their computer; the command does not send it to the others.

The host runs the following; as in the app, the host must not be behind carrier-grade NAT
([how to check](https://github.com/ORQWA/ORQWA/wiki/Group-rides#who-should-create-the-ride)):

```sh
./orqwa-cli ride-together --route shared.gpx --name Anna
```

Send the printed invite privately to each rider. Each runs the following with the whole invite
in quotes, then sends the printed reply privately back to the host:

```sh
./orqwa-cli ride-together --route shared.gpx --name Ben --join 'orqwa:…'
```

The host pastes each reply and presses Enter. A reply works for one minute; after that, the
rider runs the join command again for a fresh one. After sixteen replies, the host starts again
and sends a fresh invite. Once every rider shows as **joined**, the host types `go` and Enter
for a ten-second countdown (`--countdown 5` sets 1–60 seconds). Nobody can join after it starts.

During the ride, the terminal shows the others' gaps: positive means ahead of you. You keep
your own trainer settings and difficulty, and there is no drafting. The others see your name,
weight, rider-plus-bike mass, time, distance, speed, power and cadence; your heart rate stays
on your computer. In the app, CLI riders appear as the default rider. Keep the codes private
and out of bug reports and logs.

Type `q` and Enter, or press Ctrl+C, to leave and save your own FIT activity; `--output mine.fit`
chooses where. If a rider or the host drops out, your ride goes on; if the host disappears while
you are still waiting, type `go` to start alone.

`--difficulty 50` sets how much of the road gradient you feel (0–100 %), `--mass 83` your
rider-plus-bike mass (20–300 kg) and `--weight 75` your own weight, from which the others see
your W/kg. Trainer, sensor and shifter selection works as under **Ride** above, and the network
options as under **Check group-ride networking**. If a rider cannot connect, try
`--lan --no-stun` on one home network, or another network.

For a trial on one computer, run both commands in two terminals with the same GPX and add
`--loopback --fake --offline`. The app's [Group Rides](https://github.com/ORQWA/ORQWA/wiki/Group-rides)
download the course and workout for everyone; the command line needs the shared GPX.

## Voice chat

Add `--voice` on each computer that wants to talk or listen:

```sh
./orqwa-cli ride-together --route shared.gpx --name Anna --voice
./orqwa-cli ride-together --route shared.gpx --name Ben --voice --join 'orqwa:…'
```

Wait for **Voice ready; microphone muted**, then type commands and press Enter. Voice works in
the lobby and during the ride. Use headphones: ORQWA does not cancel speaker echo or remove
background noise.

| Command | Effect |
|---|---|
| `mic on` | Open the microphone and transmit to the group |
| `mic off` | Stop microphone transmission and close capture |
| `deafen` | Silence all group speech and mute your microphone |
| `undeafen` | Listen again; the microphone stays muted |
| `mute 2` / `unmute 2` | Silence/restore that rider locally; voice rider IDs print when people join |
| `volume 0.5` | Set voice volume from 0 to 1; default 0.6 |
| `q` or Ctrl+C | Leave, close voice and save your own activity |

The microphone stays off until `mic on`, which may ask for microphone permission. If it fails,
allow your terminal microphone access in the macOS or Windows privacy settings.

To use a particular headset or microphone, run `./orqwa-cli voice-devices`, copy its ID and add
`--voice-input 'ID'` and/or `--voice-output 'ID'`; otherwise the system's default devices are
used. If a device disconnects or voice stops, your ride keeps running. If the CLI does not start
on Linux because `libasound.so.2` is missing, install your distribution's ALSA package.

Voice needs no extra firewall ports. Speech goes through the host, who can hear it, as can
every rider who turns voice on. ORQWA does not record, transcribe or upload speech, nor put it
into FIT activities. Voice ends if the host leaves; your ride continues. If speech breaks up,
try a steadier network with less other traffic, especially at the host.

Everyone needs the same ORQWA version; after updating, exchange fresh codes.

## Ride a workout

```sh
./orqwa-cli ride --power 200 --hr                         # hold 200 W
./orqwa-cli ride --hr-zone 3 --hr --max-hr 190 --ftp 250  # hold heart-rate zone 3
./orqwa-cli ride --workout my.zwo --ftp 250             # a structured workout
```

`./orqwa-cli workout builtins` lists the built-in workouts. Other options: `--hr-target 140`,
`--min-power`, `--max-power` and `--workout file.zwo` (ZWO, ERG, MRC or FIT).

## Check virtual gears

```sh
./orqwa-cli gear-check --trainer kickr --gears 50x14
```

Rides a flat road in virtual gears and shows each second whether the trainer brakes as the gear
asks. `u` / `d` shift, a number jumps to that gear, `g 4` sets a 4 % road, `q` quits.

To check a shifter without a trainer, let `--fake` stand in for it; the gear changes as you
press the shifter's buttons, and each device prints its battery level (`battery 80 %`) when
it connects and whenever it changes:

```sh
RUST_LOG=orqwa_devices=debug ./orqwa-cli gear-check --fake --controller "Zwift Click v2"
```

<details>
<summary>Reading the output</summary>

```text
gear 24 5.50  sent +0.0 % Crr 0.0062 Cw 0.72 |  420 W  91 rpm  41.0 km/h | felt 3.57 (real 3.57) | expect 1114 W, ungeared 327 W | virtual 49.3 km/h = 4.29
```

| Part | Meaning |
|---|---|
| `gear 24 5.50` | Virtual gear and its ratio |
| `sent …` | Gradient, rolling resistance and wind sent to the trainer |
| `felt … (real …)` | The ratio the trainer turns in; should match the real gear |
| `expect … W` | What the trainer should brake at |
| `ungeared … W` | What it would brake without the gear |

If the power stays near `ungeared`, the trainer ignores rolling resistance and wind, so gears
only work on gradients.

</details>

## Check a video

```sh
./orqwa-cli video my-ride.mp4
```

Tells you whether a [video course](https://github.com/ORQWA/ORQWA/wiki/Video-courses) keeps up
on this computer.

## Sample rides

```sh
./orqwa-cli sample-rides --weeks 8
```

Fills the active rider's history with made-up rides, e.g. to try the
[Analysis](https://github.com/ORQWA/ORQWA/wiki/Analysis) tab. Delete them afterwards.

Set `RUST_LOG=debug` for protocol details. If the trainer drops out, the CLI reconnects by
itself.
