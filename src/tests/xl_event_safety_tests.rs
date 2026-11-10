use super::*;

/// Every index and coordinate a supported model can emit must pass through the
/// real GTK event path without panicking; controls the preview lacks are ignored.
#[test]
fn every_event_an_xl_can_emit_is_handled_by_the_event_path() {
    run_gtk_test(|| {
        let (root, layers, artwork_store, _source) = artwork_fixture("xl-event-safety");
        let layers = Rc::new(RefCell::new(layers));
        let window = ApplicationWindow::builder().build();
        install_style(&window);
        let (output, _hardware) = mpsc::channel();
        let artwork = ArtworkUi {
            store: Rc::new(RefCell::new(Some(artwork_store))),
            layers: layers.clone(),
            pictures: Rc::new(RefCell::new(Vec::new())),
            output,
            last_layer: Rc::new(RefCell::new(None)),
            chooser_owners: Rc::new(RefCell::new(Default::default())),
            selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
            text_editor: Rc::new(RefCell::new(std::rc::Weak::new())),
            info: Rc::default(),
        };
        let (layer_sender, _layer_commands) = mpsc::channel();
        let widgets = build_widgets(
            window.clone(),
            layers.clone(),
            artwork.clone(),
            layer_sender,
        );
        let zones = widgets.zone_widgets.clone();
        let keys = widgets.key_widgets.clone();
        let (events, receiver) = mpsc::channel();
        let (layer_sender, _layer_commands) = mpsc::channel();
        let polling = start_event_updates(
            widgets,
            receiver,
            layers,
            artwork,
            layer_sender,
            events.clone(),
        );
        let send = |event: protocol::Event| {
            events
                .send(MonitorEvent::Input {
                    device: sdeck_plus::event_api::DeviceId::from_path("sim"),
                    model: &sdeck_plus::device_model::PLUS_XL,
                    event,
                })
                .unwrap()
        };
        events
            .send(MonitorEvent::Connection(ConnectionState::Connected {
                device: sdeck_plus::event_api::DeviceId::from_path("sim"),
                model: &sdeck_plus::device_model::PLUS_XL,
            }))
            .unwrap();
        for index in 0..36 {
            for pressed in [true, false] {
                send(protocol::Event::Key { index, pressed });
            }
        }
        for index in 0..6 {
            for pressed in [true, false] {
                send(protocol::Event::EncoderButton { index, pressed });
            }
            for ticks in [3, -2] {
                send(protocol::Event::Rotation { index, ticks });
            }
        }
        for x in [0u16, 199, 200, 1199, 1200, 65535] {
            send(protocol::Event::TouchTap { x, y: 50 });
            send(protocol::Event::TouchPress { x, y: 50 });
            send(protocol::Event::TouchFlick {
                start_x: x,
                start_y: 50,
                end_x: 0,
                end_y: 50,
            });
        }
        // The last event proves the handler consumed everything before it.
        send(protocol::Event::Key {
            index: 0,
            pressed: true,
        });
        let pump = glib::MainContext::default();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !keys[0].has_css_class("control-pressed") {
            assert!(std::time::Instant::now() < deadline, "events not consumed");
            pump.iteration(true);
        }
        assert_eq!(zones.len(), 4, "the + preview keeps its four sections");
        polling.remove();
        window.destroy();
        std::fs::remove_dir_all(root).unwrap();
    });
}
