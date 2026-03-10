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

// Mapping derived from physical photo evidence + SDK internals.
// The SDK applies opendeck_to_device_key internally, so we pre-transform
// our positions to cancel that out and hit the correct physical LCD slot.
// OD 3x5 landscape → physical 5x3 portrait (90° CW rotation).
const OD_TO_SDK: [u8; 15] = [0, 5, 10, 1, 6, 2, 7, 12, 3, 8, 4, 9, 14, 11, 13];
const SDK_TO_OD: [u8; 15] = [0, 3, 5, 8, 10, 1, 4, 6, 9, 11, 2, 13, 7, 14, 12];

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct InboundMessage {
    event: String,
    position: Option<u8>,
    image: Option<String>,
}

enum DeviceCmd {
    SetImage(u8, Option<String>),
    SetBrightness(u8),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let log_file = std::fs::File::create("/tmp/opendeck_akp815.log")?;
    let _ = CombinedLogger::init(vec![
        TermLogger::new(LevelFilter::Debug, Config::default(), TerminalMode::Mixed, ColorChoice::Auto),
        WriteLogger::new(LevelFilter::Debug, Config::default(), log_file),
    ]);

    info!("Starting Ajazz AKP815 Device Plugin (3x5 landscape, photo-derived mapping)...");

    let args: Vec<String> = env::args().collect();
    let mut port = 0;
    let mut plugin_uuid = String::new();
    let mut register_event = String::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-port" => { if i + 1 < args.len() { port = args[i + 1].parse().unwrap_or(0); } i += 2; }
            "-pluginUUID" => { if i + 1 < args.len() { plugin_uuid = args[i + 1].clone(); } i += 2; }
            "-registerEvent" => { if i + 1 < args.len() { register_event = args[i + 1].clone(); } i += 2; }
            _ => i += 1,
        }
    }

    if port == 0 || plugin_uuid.is_empty() {
        error!("Missing required arguments.");
        return Ok(());
    }

    info!("Connecting to OpenDeck on port {}...", port);
    let url = format!("ws://127.0.0.1:{}", port);
    let (ws_stream, _) = connect_async(&url).await.expect("Failed to connect to OpenDeck");
    let (mut ws_sink, mut ws_read) = ws_stream.split();

    let (ws_write_tx, mut ws_write_rx) = tokio::sync::mpsc::channel::<Message>(32);
    let ws_write_tx_for_events = ws_write_tx.clone();

    tokio::spawn(async move {
        while let Some(msg) = ws_write_rx.recv().await {
            let _ = ws_sink.send(msg).await;
        }
    });

    let register_msg = json!({ "event": register_event, "uuid": plugin_uuid });
    let _ = ws_write_tx.send(Message::Text(register_msg.to_string().into())).await;

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
        None => { error!("AKP815 not found."); return Ok(()); }
    });

    let device_id = format!("aj-{}", serial_number);

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
    let _ = ws_write_tx.send(Message::Text(device_info_msg.to_string().into())).await;
    info!("Registered device with 3x5 landscape layout.");

    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<DeviceCmd>(32);

    tokio::spawn(async move {
        while let Some(msg) = ws_read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    let result: Result<InboundMessage, _> = serde_json::from_str(&text);
                    if let Ok(cmd) = result {
                        if cmd.event == "setImage" {
                            let _ = cmd_tx.send(DeviceCmd::SetImage(cmd.position.unwrap_or(0), cmd.image)).await;
                        } else if cmd.event == "setBrightness" {
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                                if let Some(b) = val.get("brightness").and_then(|v| v.as_u64()) {
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

    info!("Starting device event loop...");
    let reader = ajazz_device.get_reader();

    loop {
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                DeviceCmd::SetImage(pos, img_str) => {
                    let sdk_pos = if (pos as usize) < OD_TO_SDK.len() {
                        OD_TO_SDK[pos as usize]
                    } else { pos };

                    debug!("SetImage: od_pos={}, sdk_pos={}", pos, sdk_pos);

                    if let Some(img_str) = img_str {
                        if img_str.is_empty() {
                            let _ = ajazz_device.clear_button_image(sdk_pos);
                        } else if let Some((_, b64)) = img_str.split_once(',') {
                            use base64::Engine;
                            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                                if let Ok(img) = image::load_from_memory(&bytes) {
                                    use image::imageops::FilterType;
                                    let resized = img.resize_exact(100, 100, FilterType::Nearest);
                                    let _ = ajazz_device.set_button_image(sdk_pos, resized);
                                }
                            }
                        }
                    } else {
                        let _ = ajazz_device.clear_button_image(sdk_pos);
                    }
                    let _ = ajazz_device.flush();
                }
                DeviceCmd::SetBrightness(b) => {
                    let _ = ajazz_device.set_brightness(b);
                }
            }
        }

        match reader.read(Some(Duration::from_millis(50))) {
            Ok(events) => {
                for event in events {
                    match event {
                        Event::ButtonDown(sdk_pos) => {
                            let od_pos = if (sdk_pos as usize) < SDK_TO_OD.len() {
                                SDK_TO_OD[sdk_pos as usize]
                            } else { sdk_pos };
                            debug!("ButtonDown: sdk={}, od={}", sdk_pos, od_pos);
                            let msg = json!({
                                "event": "keyDown",
                                "payload": { "device": device_id, "position": od_pos }
                            });
                            let _ = ws_write_tx_for_events.send(Message::Text(msg.to_string().into())).await;
                        }
                        Event::ButtonUp(sdk_pos) => {
                            let od_pos = if (sdk_pos as usize) < SDK_TO_OD.len() {
                                SDK_TO_OD[sdk_pos as usize]
                            } else { sdk_pos };
                            let msg = json!({
                                "event": "keyUp",
                                "payload": { "device": device_id, "position": od_pos }
                            });
                            let _ = ws_write_tx_for_events.send(Message::Text(msg.to_string().into())).await;
                        }
                        _ => {}
                    }
                }
            }
            Err(_) => {}
        }
        tokio::task::yield_now().await;
    }
}
