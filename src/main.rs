mod event_api;
mod protocol;

use event_api::{ConnectionState, MonitorEvent};
use gtk::{glib, prelude::*, Application, ApplicationWindow, Box as GtkBox, Label, Orientation};
use std::{cell::RefCell, rc::Rc, sync::mpsc, time::Duration};

const APP_ID: &str = "dev.streamdeck.EventMonitor";

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &Application) {
    let (sender, receiver) = mpsc::channel();
    event_api::start_hid_monitor(sender);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Stream Deck + Monitor")
        .default_width(920)
        .default_height(760)
        .build();

    let root = GtkBox::new(Orientation::Vertical, 22);
    root.set_margin_top(28);
    root.set_margin_bottom(28);
    root.set_margin_start(32);
    root.set_margin_end(32);
    root.add_css_class("app-shell");

    let header = GtkBox::new(Orientation::Horizontal, 12);
    let heading = GtkBox::new(Orientation::Vertical, 4);
    let title = Label::new(Some("STREAM DECK +"));
    title.add_css_class("title");
    title.set_xalign(0.0);
    let subtitle = Label::new(Some("Live input monitor  ·  Read-only observer"));
    subtitle.add_css_class("muted");
    subtitle.set_xalign(0.0);
    heading.append(&title);
    heading.append(&subtitle);
    header.append(&heading);
    let status = Label::new(Some("SEARCHING"));
    status.add_css_class("status");
    status.add_css_class("status-idle");
    status.set_hexpand(true);
    status.set_halign(gtk::Align::End);
    header.append(&status);
    root.append(&header);

    let device_card = card();
    let device_title = Label::new(Some("DEVICE CONNECTION"));
    device_title.add_css_class("section-label");
    device_title.set_xalign(0.0);
    let connection_detail = Label::new(Some("Looking for USB device · VID 0FD9 / PID 0084"));
    connection_detail.add_css_class("muted");
    connection_detail.set_xalign(0.0);
    device_card.append(&device_title);
    device_card.append(&connection_detail);
    root.append(&device_card);

    let controls = GtkBox::new(Orientation::Horizontal, 16);
    controls.set_homogeneous(true);
    let keys = card();
    let key_title = section_title("KEYPAD");
    keys.append(&key_title);
    let key_grid = GtkBox::new(Orientation::Vertical, 10);
    let mut key_labels = Vec::new();
    for row in 0..2 {
        let line = GtkBox::new(Orientation::Horizontal, 10);
        line.set_homogeneous(true);
        for col in 0..4 {
            let index = row * 4 + col;
            let label = Label::new(Some(&format!("KEY {}\nIDLE", index + 1)));
            label.add_css_class("control-tile");
            line.append(&label);
            key_labels.push(label);
        }
        key_grid.append(&line);
    }
    keys.append(&key_grid);

    let encoders = card();
    encoders.append(&section_title("ENCODERS"));
    let encoder_grid = GtkBox::new(Orientation::Vertical, 10);
    let mut encoder_labels = Vec::new();
    for row in 0..2 {
        let line = GtkBox::new(Orientation::Horizontal, 10);
        line.set_homogeneous(true);
        for col in 0..2 {
            let index = row * 2 + col;
            let label = Label::new(Some(&format!("DIAL {}\nIDLE · 0 ticks", index + 1)));
            label.add_css_class("control-tile");
            line.append(&label);
            encoder_labels.push(label);
        }
        encoder_grid.append(&line);
    }
    encoders.append(&encoder_grid);
    controls.append(&keys);
    controls.append(&encoders);
    root.append(&controls);

    let touch = card();
    let touch_header = GtkBox::new(Orientation::Horizontal, 8);
    touch_header.append(&section_title("TOUCH STRIP"));
    let coordinates = Label::new(Some("800 × 100 logical px"));
    coordinates.add_css_class("muted");
    coordinates.set_hexpand(true);
    coordinates.set_halign(gtk::Align::End);
    touch_header.append(&coordinates);
    touch.append(&touch_header);
    let touch_status = Label::new(Some("Tap, press and flick gestures will appear here"));
    touch_status.add_css_class("touch-display");
    touch_status.set_xalign(0.0);
    touch.append(&touch_status);
    root.append(&touch);

    let events_card = card();
    let event_heading = GtkBox::new(Orientation::Horizontal, 8);
    event_heading.append(&section_title("RECENT EVENTS"));
    let event_count = Label::new(Some("0 EVENTS"));
    event_count.add_css_class("muted");
    event_count.set_hexpand(true);
    event_count.set_halign(gtk::Align::End);
    event_heading.append(&event_count);
    events_card.append(&event_heading);
    let event_list = GtkBox::new(Orientation::Vertical, 6);
    let empty = Label::new(Some("Waiting for input…"));
    empty.add_css_class("muted");
    empty.set_xalign(0.0);
    event_list.append(&empty);
    let scroller = gtk::ScrolledWindow::builder()
        .min_content_height(120)
        .max_content_height(190)
        .child(&event_list)
        .build();
    events_card.append(&scroller);
    root.append(&events_card);

    install_style(&window);
    window.set_child(Some(&root));
    window.present();

    let ticks = Rc::new(RefCell::new([0i64; 4]));
    let history = Rc::new(RefCell::new(Vec::<String>::new()));
    glib::timeout_add_local(Duration::from_millis(40), move || {
        while let Ok(message) = receiver.try_recv() {
            match message {
                MonitorEvent::Connection(state) => match state {
                    ConnectionState::Searching => {
                        status.set_text("SEARCHING");
                        status.remove_css_class("status-online");
                        status.add_css_class("status-idle");
                        connection_detail.set_text("Looking for USB device · VID 0FD9 / PID 0084");
                    }
                    ConnectionState::Connected => {
                        status.set_text("CONNECTED");
                        status.remove_css_class("status-idle");
                        status.add_css_class("status-online");
                        connection_detail.set_text("Stream Deck + connected · HID input active");
                    }
                    ConnectionState::Unavailable(detail) => {
                        status.set_text("UNAVAILABLE");
                        status.remove_css_class("status-online");
                        status.add_css_class("status-idle");
                        connection_detail.set_text(&detail);
                    }
                    ConnectionState::Disconnected(detail) => {
                        status.set_text("DISCONNECTED");
                        status.remove_css_class("status-online");
                        status.add_css_class("status-idle");
                        connection_detail.set_text(&detail);
                    }
                },
                MonitorEvent::Input(event) => {
                    let text = describe(&event);
                    match event {
                        protocol::Event::Key { index, pressed } => {
                            if let Some(label) = key_labels.get(index) {
                                label.set_text(&format!(
                                    "KEY {}\n{}",
                                    index + 1,
                                    if pressed { "PRESSED" } else { "IDLE" }
                                ));
                                set_active(label, pressed);
                            }
                        }
                        protocol::Event::EncoderButton { index, pressed } => {
                            if let Some(label) = encoder_labels.get(index) {
                                label.set_text(&format!(
                                    "DIAL {}\n{} · {} ticks",
                                    index + 1,
                                    if pressed { "PRESSED" } else { "IDLE" },
                                    ticks.borrow()[index]
                                ));
                                set_active(label, pressed);
                            }
                        }
                        protocol::Event::Rotation {
                            index,
                            ticks: delta,
                        } => {
                            let mut totals = ticks.borrow_mut();
                            totals[index] += i64::from(delta);
                            if let Some(label) = encoder_labels.get(index) {
                                label.set_text(&format!(
                                    "DIAL {}\n{} ticks · {delta:+}",
                                    index + 1,
                                    totals[index]
                                ));
                            }
                        }
                        protocol::Event::TouchTap { x, y } => {
                            touch_status.set_text(&format!("TAP     ·     X {x}   /   Y {y}"))
                        }
                        protocol::Event::TouchPress { x, y } => touch_status
                            .set_text(&format!("PRESS & HOLD     ·     X {x}   /   Y {y}")),
                        protocol::Event::TouchFlick {
                            start_x,
                            start_y,
                            end_x,
                            end_y,
                        } => touch_status.set_text(&format!(
                            "FLICK     ·     ({start_x}, {start_y})  →  ({end_x}, {end_y})"
                        )),
                    }
                    let mut entries = history.borrow_mut();
                    entries.insert(0, text);
                    entries.truncate(20);
                    event_count.set_text(&format!("{} EVENTS", entries.len()));
                    while let Some(child) = event_list.first_child() {
                        event_list.remove(&child);
                    }
                    for entry in entries.iter() {
                        let row = Label::new(Some(entry));
                        row.add_css_class("event-row");
                        row.set_xalign(0.0);
                        event_list.append(&row);
                    }
                }
            }
        }
        glib::ControlFlow::Continue
    });
}

fn set_active(label: &Label, active: bool) {
    if active {
        label.add_css_class("control-active");
    } else {
        label.remove_css_class("control-active");
    }
}

fn describe(event: &protocol::Event) -> String {
    match event {
        protocol::Event::Key { index, pressed } => format!(
            "KEY {}  ·  {}",
            index + 1,
            if *pressed { "PRESSED" } else { "RELEASED" }
        ),
        protocol::Event::EncoderButton { index, pressed } => format!(
            "ENCODER {} BUTTON  ·  {}",
            index + 1,
            if *pressed { "PRESSED" } else { "RELEASED" }
        ),
        protocol::Event::Rotation { index, ticks } => {
            format!("ENCODER {}  ·  ROTATED {ticks:+} TICKS", index + 1)
        }
        protocol::Event::TouchTap { x, y } => format!("TOUCH STRIP  ·  TAP ({x}, {y})"),
        protocol::Event::TouchPress { x, y } => format!("TOUCH STRIP  ·  PRESS ({x}, {y})"),
        protocol::Event::TouchFlick {
            start_x,
            start_y,
            end_x,
            end_y,
        } => format!("TOUCH STRIP  ·  FLICK ({start_x}, {start_y}) → ({end_x}, {end_y})"),
    }
}

fn card() -> GtkBox {
    let panel = GtkBox::new(Orientation::Vertical, 12);
    panel.add_css_class("card");
    panel.set_margin_top(1);
    panel.set_margin_bottom(1);
    panel.set_margin_start(1);
    panel.set_margin_end(1);
    panel
}

fn section_title(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.add_css_class("section-label");
    label.set_xalign(0.0);
    label
}

fn install_style(window: &ApplicationWindow) {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(".app-shell { background: #10141d; color: #edf1fb; } .title { color: #f4f6ff; font-size: 25px; font-weight: 800; letter-spacing: 1px; } .muted { color: #929db2; font-size: 12px; } .status { padding: 8px 14px; border-radius: 18px; font-size: 11px; font-weight: 800; letter-spacing: 1px; } .status-idle { color: #f3c77b; background: #392e1d; } .status-online { color: #78e0ae; background: #18372f; } .card { background: #191f2c; border: 1px solid #293246; border-radius: 15px; padding: 16px; } .section-label { color: #9ca9c1; font-size: 11px; font-weight: 800; letter-spacing: 1.3px; } .control-tile { background: #202838; border: 1px solid #303b50; border-radius: 11px; color: #c2ccdc; font-size: 12px; font-weight: 700; padding: 13px 8px; } .control-active { background: #174936; border-color: #38d996; color: #9af2c4; } .touch-display { background: #202838; border-radius: 10px; color: #8fe7ca; font-size: 14px; font-weight: 700; padding: 16px; } .event-row { background: #202838; border-radius: 7px; color: #d9e1ef; font-size: 12px; padding: 8px 10px; }");
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("GTK display"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let _ = window;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_touch_events_with_coordinates() {
        assert_eq!(
            describe(&protocol::Event::TouchTap { x: 5, y: 9 }),
            "TOUCH STRIP  ·  TAP (5, 9)"
        );
        assert!(describe(&protocol::Event::TouchFlick {
            start_x: 1,
            start_y: 2,
            end_x: 3,
            end_y: 4
        })
        .contains("(1, 2) → (3, 4)"));
    }
}
