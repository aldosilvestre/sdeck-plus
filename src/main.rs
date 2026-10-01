mod protocol;

use crossterm::{
    cursor,
    event::{self, Event as TerminalEvent, KeyCode},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use hidapi::HidApi;
use protocol::{decode_report, Event};
use std::{
    io::{self, Write},
    thread,
    time::Duration,
};

const VID: u16 = 0x0FD9;
const PID: u16 = 0x0084;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = HidApi::new()?;
    let Some(info) = api
        .device_list()
        .find(|d| d.vendor_id() == VID && d.product_id() == PID)
    else {
        println!("Stream Deck + not found (VID 0x0FD9 / PID 0x0084). Connect the device and check permissions.");
        return Ok(());
    };
    let device = match info.open_device(&api) {
        Ok(device) => device,
        Err(error) => {
            eprintln!(
                "Found Stream Deck +, but could not open it: {error}. Check device permissions."
            );
            return Ok(());
        }
    };
    let mut stdout = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
    let result = monitor(&device, &mut stdout);
    execute!(
        stdout,
        cursor::Show,
        terminal::LeaveAlternateScreen,
        ResetColor
    )?;
    terminal::disable_raw_mode()?;
    result
}

fn monitor(
    device: &hidapi::HidDevice,
    out: &mut impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut lines: Vec<(String, Color)> = Vec::new();
    loop {
        execute!(out, cursor::MoveTo(0, 0), Clear(ClearType::All), SetForegroundColor(Color::Cyan), Print("  STREAM DECK +  /  EVENT MONITOR\n"), ResetColor,
            Print("  Read-only HID observer  ·  8 keys  ·  4 encoder dials\n\n"), SetForegroundColor(Color::DarkGrey),
            Print("  KEYPAD\n  [1] [2] [3] [4]\n  [5] [6] [7] [8]\n\n  ENCODERS\n  (1) (2) (3) (4)\n\n  LIVE EVENTS\n"), ResetColor)?;
        if lines.is_empty() {
            execute!(out, Print("  Waiting for input…   ·   Press Q to quit\n"))?;
        }
        for line in lines.iter().rev().take(12) {
            execute!(
                out,
                Print("  "),
                SetForegroundColor(line.1),
                Print(&line.0),
                ResetColor,
                Print("\n")
            )?;
        }
        out.flush()?;
        if event::poll(Duration::from_millis(1))?
            && matches!(event::read()?, TerminalEvent::Key(k) if k.code == KeyCode::Char('q') || k.code == KeyCode::Esc)
        {
            break;
        }
        let mut buffer = [0u8; 64];
        match device.read_timeout(&mut buffer, 50) {
            Ok(0) => {}
            Ok(size) => {
                for event in decode_report(&buffer[..size]) {
                    let (text, color) = describe(event);
                    lines.push((text, color));
                }
            }
            Err(error) => {
                lines.push((
                    format!("Device disconnected or read failed: {error}"),
                    Color::Red,
                ));
                break;
            }
        }
        thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

fn describe(event: Event) -> (String, Color) {
    match event {
        Event::Key { index, pressed } => (
            format!(
                "KEY {}  {}",
                index + 1,
                if pressed { "PRESSED" } else { "RELEASED" }
            ),
            if pressed {
                Color::Green
            } else {
                Color::DarkGrey
            },
        ),
        Event::EncoderButton { index, pressed } => (
            format!(
                "ENCODER {} BUTTON  {}",
                index + 1,
                if pressed { "PRESSED" } else { "RELEASED" }
            ),
            if pressed {
                Color::Yellow
            } else {
                Color::DarkGrey
            },
        ),
        Event::Rotation { index, ticks } => (
            format!("ENCODER {}  ROTATED  {ticks:+} ticks", index + 1),
            Color::Magenta,
        ),
    }
}
