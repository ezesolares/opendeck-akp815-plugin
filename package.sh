#!/usr/bin/env bash
set -e

PLUGIN_UUID="com.artecom.akp815"
PLUGIN_DIR="${PLUGIN_UUID}.sdPlugin"
OUTPUT_FILE="${PLUGIN_UUID}.streamDeckPlugin"

echo "Building plugin..."
cargo build --release

echo "Packaging plugin..."
rm -rf "$PLUGIN_DIR" "$OUTPUT_FILE"
mkdir -p "$PLUGIN_DIR"

# Copy the binary
cp target/release/opendeck-akp815-plugin "$PLUGIN_DIR/"
chmod +x "$PLUGIN_DIR/opendeck-akp815-plugin"

# Copy the manifest
cp manifest.json "$PLUGIN_DIR/"

# Copy icons if they exist
if [ -d icons ]; then
  cp -r icons/ "$PLUGIN_DIR/icons/"
fi

# Create the .streamDeckPlugin file (it's just a zip)
# We must include the .sdPlugin directory itself because OpenDeck's zip_extract.rs 
# looks for a path component ending with ".sdplugin" to identify the plugin root.
zip -r "$OUTPUT_FILE" "$PLUGIN_DIR"

# Clean up
rm -rf "$PLUGIN_DIR"

echo ""
echo "✅ Plugin packaged: $OUTPUT_FILE"
echo "   Install it in OpenDeck: Settings → Plugins → Install from file"
