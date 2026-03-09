#!/usr/bin/env bash
set -e

PLUGIN_ID="com.artecom.akp815"
PLUGIN_DIR="$HOME/.local/share/opendeck/plugins/$PLUGIN_ID"

echo "Building plugin..."
cargo build --release

echo "Installing to $PLUGIN_DIR ..."
mkdir -p "$PLUGIN_DIR/icons"

cp target/release/opendeck-akp815-plugin "$PLUGIN_DIR/"
cp manifest.json "$PLUGIN_DIR/"

# Copy icons if they exist
if [ -d icons ]; then
  cp -r icons/ "$PLUGIN_DIR/icons/"
fi

echo "Installing udev rules (requires sudo)..."
sudo cp 40-opendeck-akp815.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger

echo ""
echo "Done! Please restart OpenDeck."
echo ""
echo "IMPORTANT: Verify your AKP815 PID with: lsusb | grep -i ajazz"
echo "Then update AKP815_PID in src/main.rs and 40-opendeck-akp815.rules if needed."
