# OpenDeck Plugin for AJAZZ AKP815

A plugin for [OpenDeck](https://github.com/nekename/OpenDeck) targeting the AJAZZ AKP815 stream controller.

## Prerequisites

- Rust (https://rustup.rs)
- OpenDeck installed and running
- AJAZZ AKP815 keyboard

This wouldnt have been done without the amazing mirajazz library (https://github.com/nekename/mirajazz). All hail to them.

## Find Your Device PID

With the AKP815 plugged in, run:

```bash
lsusb

For me, it was Bus 001 Device 010: ID 5548:6672 9B3 9B390
```

Bus 001 Device 010: ID 5548:6672 9B3 9B390


Example output: `Bus 001 Device 010: ID 5548:6672 9B3 9B390`

The PID is the second 4-digit hex value (e.g. `6672`). If it differs from `0x6672`, update:
- `AKP815_PID` in `src/main.rs`
- Both `.rules` file entries

## Install

```bash
chmod +x install.sh
./install.sh
```
