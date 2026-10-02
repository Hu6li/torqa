# torqa-cli

Headless tool to find and ride smart trainers without the app. Useful for testing trainers and
sensors.

## Install (macOS)

Download the `torqa-cli-macos-arm64` artifact from a CI run, then:

```sh
unzip torqa-cli-macos-arm64.zip
tar -xzf torqa-cli.tar.gz
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
```

Live readings are printed every second. Type a command and press Enter:

| Command | Effect |
|---|---|
| `g 5` | SIM mode, 5 % grade (negative for descents) |
| `p 200` | ERG mode, hold 200 W |
| `r 30` | Resistance at 30 % of the trainer's range |
| `q` | Quit and disconnect (Ctrl+C works too; press it twice to skip the disconnect) |

If no heart-rate strap is found, the ride continues without one. Wahoo trainers estimate cadence
from the flywheel, so it may read 0 for the first seconds or at very low power.

If the trainer drops out, the CLI reconnects automatically and re-applies the last command.
Set `RUST_LOG=debug` for protocol details (accepted/rejected commands, resistance range).
