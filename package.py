import os
import shutil
import zipfile
import subprocess
import time

PLUGIN_UUID = "com.artecom.akp815"
PLUGIN_DIR = f"{PLUGIN_UUID}.sdPlugin"
OUTPUT_FILE = f"{PLUGIN_UUID}.streamDeckPlugin"

def run_cmd(cmd):
    print(f"Running: {cmd}")
    subprocess.run(cmd, shell=True, check=True)

def package():
    print("Building plugin binary (release)...")
    run_cmd("cargo build --release")

    print(f"Cleaning previous build... {PLUGIN_DIR}")
    if os.path.exists(PLUGIN_DIR):
        shutil.rmtree(PLUGIN_DIR)
    if os.path.exists(OUTPUT_FILE):
        os.remove(OUTPUT_FILE)

    os.makedirs(PLUGIN_DIR)

    # Copy files
    print("Copying files to plugin directory...")
    shutil.copy("target/release/opendeck-akp815-plugin", os.path.join(PLUGIN_DIR, "opendeck-akp815-plugin"))
    os.chmod(os.path.join(PLUGIN_DIR, "opendeck-akp815-plugin"), 0o755)
    shutil.copy("manifest.json", os.path.join(PLUGIN_DIR, "manifest.json"))

    if os.path.exists("icons"):
        shutil.copytree("icons", os.path.join(PLUGIN_DIR, "icons"))

    print(f"Creating ZIP archive: {OUTPUT_FILE}")
    # We use ZipFile with explicit closes and fsync to ensure the EOCD (End of Central Directory)
    # is written correctly and flushed to block storage.
    with open(OUTPUT_FILE, 'wb') as f_out:
        with zipfile.ZipFile(f_out, 'w', compression=zipfile.ZIP_DEFLATED) as zf:
            for root, dirs, files in os.walk(PLUGIN_DIR):
                for file in files:
                    # We want the ZIP to contain the folder `PLUGIN_DIR/` at its root.
                    file_path = os.path.join(root, file)
                    rel_path = os.path.relpath(file_path, ".")
                    zf.write(file_path, rel_path)
        
        # Explicit flush and fsync of the file handle
        f_out.flush()
        os.fsync(f_out.fileno())

    # Wait a moment for OS to settle
    time.sleep(1)

    print("Verifying archive integrity...")
    with zipfile.ZipFile(OUTPUT_FILE, 'r') as zf:
        ret = zf.testzip()
        if ret is not None:
            raise Exception(f"Corrupt ZIP at file: {ret}")
        print("✅ ZIP internal verification passed.")

    # Show structure
    run_cmd(f"unzip -l {OUTPUT_FILE}")

    # Cleanup temp folder
    shutil.rmtree(PLUGIN_DIR)
    
    # Final check of file size
    size = os.path.getsize(OUTPUT_FILE)
    print(f"✅ Plugin packaged successfully: {OUTPUT_FILE} ({size} bytes)")
    print("   Ready for install in OpenDeck.")

if __name__ == "__main__":
    try:
        package()
    except Exception as e:
        print(f"❌ Error during packaging: {e}")
        exit(1)
