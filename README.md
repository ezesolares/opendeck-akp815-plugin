# OpenDeck Plugin for AJAZZ AKP815

A plugin for [OpenDeck](https://github.com/nekename/OpenDeck) targeting the AJAZZ AKP815 stream controller.

## Prerequisites

- Rust (https://rustup.rs)
- OpenDeck installed and running
- AJAZZ AKP815 device

## Find Your Device PID

With the AKP815 plugged in, run:

```bash
lsusb | grep -i ajazz
```

Example output: `Bus 003 Device 007: ID 0300:1005 Ajazz AKP815`

The PID is the second 4-digit hex value (e.g. `1005`). If it differs from `0x1005`, update:
- `AKP815_PID` in `src/main.rs`
- Both `.rules` file entries

## Install

```bash
chmod +x install.sh
./install.sh
```

## Actions Included

| Action | UUID | Description |
|---|---|---|
| Button Action | com.yourname.akp815.button | Configurable label button |
| Counter | com.yourname.akp815.counter | Counts key presses |

## Customizing

- Change the `UUID` prefix from `com.yourname` to your own reverse-domain identifier
- Add more actions by creating new structs implementing the `Action` trait
- Add a `property-inspector/index.html` to expose settings in the OpenDeck UI
