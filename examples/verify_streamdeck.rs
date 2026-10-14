//! Opt-in verification against the real Stream Deck +, using production HID code.
use sdeck_plus::{
    device_control::DeviceAction,
    event_api::{self, ConnectionState, MonitorEvent},
    protocol::Event,
};
use std::{
    error::Error,
    sync::mpsc,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args
        .iter()
        .any(|arg| arg != "--device-controls" && arg != "--input" && arg != "--button-text")
    {
        return Err("Supported options: --device-controls (sets brightness 50, sleeps and wakes), --input (waits for key, dial and touch input), --button-text (uploads horizontal and vertical keypad text)".into());
    }
    let (sender, events) = mpsc::channel();
    let mut hid = event_api::start_hid_monitor(sender);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match events.recv_timeout(deadline.saturating_duration_since(Instant::now()))? {
            MonitorEvent::Connection(ConnectionState::Connected) => {
                println!("CONNECTED: Stream Deck + 0FD9:0084, production HID worker");
                break;
            }
            MonitorEvent::Connection(ConnectionState::Unavailable(error)) => {
                return Err(error.into());
            }
            _ => {}
        }
    }
    if args.iter().any(|arg| arg == "--device-controls") {
        for action in [
            DeviceAction::Brightness(50),
            DeviceAction::AdjustBrightness(5),
            DeviceAction::AdjustBrightness(-5),
            DeviceAction::Sleep,
            DeviceAction::Wake,
        ] {
            hid.control_sender().send(action.clone())?;
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                match events.recv_timeout(deadline.saturating_duration_since(Instant::now()))? {
                    MonitorEvent::DeviceResult {
                        action: received,
                        result,
                    } if received == action => {
                        result.map_err(std::io::Error::other)?;
                        println!("DEVICE_OK: {action:?}");
                        break;
                    }
                    MonitorEvent::Connection(ConnectionState::Disconnected(error)) => {
                        return Err(error.into());
                    }
                    _ => {}
                }
            }
        }
    }
    if args.iter().any(|arg| arg == "--button-text") {
        use sdeck_plus::button_text::{ButtonText, TextOrientation};
        for orientation in [TextOrientation::Horizontal, TextOrientation::Vertical] {
            hid.artwork_sender()
                .send(event_api::ArtworkCommand::ComposeButton {
                    key: 1,
                    path: None,
                    text: ButtonText {
                        text: "Play".into(),
                        orientation,
                        ..Default::default()
                    },
                })?;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match events.recv_timeout(deadline.saturating_duration_since(Instant::now()))? {
                    MonitorEvent::ArtworkResult {
                        target: sdeck_plus::local_artwork::ArtworkTarget::Keypad(1),
                        result,
                    } => {
                        result.map_err(std::io::Error::other)?;
                        println!("BUTTON_TEXT_OK: {orientation:?}");
                        break;
                    }
                    MonitorEvent::Connection(ConnectionState::Disconnected(error)) => {
                        return Err(error.into());
                    }
                    _ => {}
                }
            }
        }
    }
    if args.iter().any(|arg| arg == "--input") {
        println!("INPUT_READY: press a key, turn a dial and touch the strip within 60 seconds");
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut observed = 0u8;
        loop {
            match events.recv_timeout(deadline.saturating_duration_since(Instant::now()))? {
                MonitorEvent::Input(input) => {
                    let kind = match &input {
                        Event::Key { pressed: true, .. } => 1,
                        Event::Rotation { ticks, .. } if *ticks != 0 => 2,
                        Event::TouchTap { .. }
                        | Event::TouchPress { .. }
                        | Event::TouchFlick { .. } => 4,
                        Event::Key { pressed: false, .. }
                        | Event::EncoderButton { .. }
                        | Event::Rotation { .. } => 0,
                    };
                    if kind != 0 {
                        observed |= kind;
                        println!("PHYSICAL_INPUT: {input:?}");
                    }
                    if observed == 7 {
                        println!("INPUT_OK: key, dial and touch strip received");
                        break;
                    }
                }
                MonitorEvent::Connection(ConnectionState::Disconnected(error)) => {
                    return Err(error.into());
                }
                _ => {}
            }
        }
    }
    hid.shutdown();
    println!("CLOSED: HID worker joined");
    Ok(())
}
