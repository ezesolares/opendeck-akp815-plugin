use std::env;
use tokio::time::Duration;
use std::sync::Arc;
use futures_util::{StreamExt, SinkExt};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use serde::{Deserialize, Serialize};
use serde_json::json;
use log::{info, error, debug};
use hidapi::HidApi;
use simplelog::{CombinedLogger, WriteLogger, TermLogger, LevelFilter, Config, TerminalMode, ColorChoice};
use ajazz_sdk::{
    Kind, Ajazz, Event
};

// -------------------------------------------------------------------------
// Types for communicating with OpenDeck (WebSocket)
// -------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum PayloadEvent<T> {
    Event { payload: T },
}

#[derive(Serialize, Deserialize, Debug)]
struct DeviceInfo {
    id: String,
    plugin: String,
    name: String,
    rows: u8,
    columns: u8,
    encoders: u8,
    touchpoints: u8,
    #[serde(rename = "type")]
    r#type: u8,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
enum OutboundEvent {
    RegisterDevice { payload: DeviceInfo },
    KeyDown { payload: PressPayload },
    KeyUp { payload: PressPayload },
}

#[derive(Serialize, Debug)]
struct PressPayload {
    device: String,
    position: u8,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct InboundMessage {
    event: String,
    device: Option<String>,
    position: Option<u8>,
    image: Option<String>,
    context: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RegistrationParams {
    port: u16,
    plugin_uuid: String,
    register_event: String,
    info: serde_json::Value,
}

// -------------------------------------------------------------------------
// Main Entry Point
// -------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup file and terminal logging so we know what's happening
    let log_file = std::fs::File::create("/tmp/opendeck_akp815.log")?;
    CombinedLogger::init(
        vec![
            TermLogger::new(LevelFilter::Debug, Config::default(), TerminalMode::Mixed, ColorChoice::Auto),
            WriteLogger::new(LevelFilter::Debug, Config::default(), log_file),
        ]
    ).unwrap_or_else(|e| eprintln!("Failed to initialize logger: {}", e));

    info!("Starting Ajazz AKP815 Device Plugin... Debug mode enabled.");

    // Parse the command line arguments sent by OpenDeck
    let args: Vec<String> = env::args().collect();
    let mut port = 0;
    let mut plugin_uuid = String::new();
    let mut register_event = String::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-port" => {
                port = args[i + 1].parse()?;
                i += 2;
            }
            "-pluginUUID" => {
                plugin_uuid = args[i + 1].clone();
                i += 2;
            }
            "-registerEvent" => {
                register_event = args[i + 1].clone();
                i += 2;
            }
            "-info" => {
                i += 2; // Ignoring info for now
            }
            _ => {
                i += 1;
            }
        }
    }

    if port == 0 || plugin_uuid.is_empty() {
        error!("Missing required arguments. Port: {}, UUID: {}", port, plugin_uuid);
        return Ok(());
    }

    info!("Connecting to OpenDeck on port {}...", port);

    // Connect to OpenDeck via WebSocket
    let url = format!("ws://127.0.0.1:{}", port);
    let (ws_stream, _) = connect_async(&url).await.expect("Failed to connect to OpenDeck");
    let (mut ws_write, mut ws_read) = ws_stream.split();

    // Send the register event
    let register_msg = json!({
        "event": register_event,
        "uuid": plugin_uuid
    });
    ws_write.send(Message::Text(register_msg.to_string().into())).await?;
    info!("Registered plugin with OpenDeck!");

    // Initialize HID API and find the Ajazz device
    let hidapi = HidApi::new()?;
    let mut ajazz_op = None;
    let mut serial_number = String::new();

    for device_info in hidapi.device_list() {
        if let Some(kind) = Kind::from_vid_pid(device_info.vendor_id(), device_info.product_id()) {
            if kind == Kind::Akp815 {
                info!("Found AKP815! Path: {:?}", device_info.path());
                serial_number = device_info.serial_number().map(|sn| sn.to_string()).unwrap_or_else(|| "UNKNOWN".to_string());
                ajazz_op = Some(Ajazz::connect(&hidapi, kind, &serial_number)?);
                break;
            }
        }
    }

    let ajazz_device = Arc::new(match ajazz_op {
        Some(dev) => dev,
        None => {
            error!("AKP815 not found. Plugin will wait for device (not implemented to rescan yet).");
            return Ok(());
        }
    });

    let device_id = format!("aj-{}", serial_number);

    // Register our device with OpenDeck
    let device_info_msg = json!({
        "event": "registerDevice",
        "payload": {
            "id": device_id.clone(),
            "plugin": plugin_uuid.clone(),
            "name": "Ajazz AKP815",
            "rows": 3,
            "columns": 5,
            "encoders": 0,
            "touchpoints": 0,
            "type": 2
        }
    });
    
    let reg_msg_str = serde_json::to_string(&device_info_msg)?;
    ws_write.send(Message::Text(reg_msg_str.into())).await?;
    info!("Sent registerDevice event.");

    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<DeviceCmd>(32);

    // Spawn a task to read from OpenDeck and send commands to the device
    tokio::spawn(async move {
        while let Some(msg) = ws_read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    debug!("Received message from OpenDeck: {}", text);
                    let result: Result<InboundMessage, _> = serde_json::from_str(&text);
                    if let Ok(cmd) = result {
                        if cmd.event == "setImage" {
                            debug!("Command: setImage at pos {}", cmd.position.unwrap_or(0));
                            let _ = cmd_tx.send(DeviceCmd::SetImage(
                                cmd.position.unwrap_or(0),
                                cmd.image
                            )).await;
                        } else if cmd.event == "setBrightness" {
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                                if let Some(b) = val.get("brightness").and_then(|v| v.as_u64()) {
                                    debug!("Command: setBrightness to {}", b);
                                    let _ = cmd_tx.send(DeviceCmd::SetBrightness(b as u8)).await;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    });

    // Main loop: read from AKP815 and send to OpenDeck, and process commands
    info!("Starting device event loop...");
    let reader = ajazz_device.get_reader();
    
    loop {
        // Process any pending commands
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                DeviceCmd::SetImage(pos, img_str) => {
                    if let Some(img_str) = img_str {
                        if img_str.is_empty() {
                            let _ = ajazz_device.clear_button_image(pos);
                        } else {
                            if let Some((_, b64)) = img_str.split_once(',') {
                                use base64::Engine;
                                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                                    if let Ok(img) = image::load_from_memory(&bytes) {
                                        use image::imageops::FilterType;
                                        let resized = img.resize_exact(100, 100, FilterType::Nearest);
                                        let _ = ajazz_device.set_button_image(pos, resized);
                                    }
                                }
                            }
                        }
                    } else {
                        let _ = ajazz_device.clear_button_image(pos);
                    }
                    let _ = ajazz_device.flush();
                }
                DeviceCmd::SetBrightness(b) => {
                    let _ = ajazz_device.set_brightness(b);
                }
            }
        }

        // DeviceStateReader::read is blocking with timeout
        match reader.read(Some(Duration::from_millis(10))) {
            Ok(events) => {
                for event in events {
                    match event {
                        Event::ButtonDown(pos) => {
                            debug!("AKP815: ButtonDown at pos {}", pos);
                            let msg = json!({
                                "event": "keyDown",
                                "payload": {
                                    "device": device_id,
                                    "position": pos
                                }
                            });
                            let msg_str = serde_json::to_string(&msg)?;
                            let _ = ws_write.send(Message::Text(msg_str.into())).await;
                        }
                        Event::ButtonUp(pos) => {
                            debug!("AKP815: ButtonUp at pos {}", pos);
                            let msg = json!({
                                "event": "keyUp",
                                "payload": {
                                    "device": device_id,
                                    "position": pos
                                }
                            });
                            let msg_str = serde_json::to_string(&msg)?;
                            let _ = ws_write.send(Message::Text(msg_str.into())).await;
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                // Ignore timeout errors, but log others
                // (Note: reader.read might return empty Vec on timeout, or an error)
                // If it's a real HID error, we might want to exit.
            }
        }
        tokio::task::yield_now().await;
    }

    Ok(())
}

enum DeviceCmd {
    SetImage(u8, Option<String>),
    SetBrightness(u8),
}
