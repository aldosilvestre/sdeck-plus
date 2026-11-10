//! Manual tool, not part of the suite: opens the real window as if Stream Decks
//! were plugged in, over scratch directories (nothing real is read or written).
//! The simulated devices run through the real HID hub, so each one really
//! shows its pictures and runs its model's actions.
//!
//!     cargo test --offline --bin sdeck-plus \
//!         tests::simulated_device_tests::open_the_window_with_simulated_devices \
//!         -- --ignored --nocapture --test-threads=1
//!
//! `SDECK_SIMULATE_MODEL` is a comma list of model ids (default `plus-xl`):
//! `plus,plus-xl` connects both, which shows the "Current" selector.
use super::model_session_tests::remember;
use super::two_device_tests::start_pair_with;
use super::*;
use sdeck_plus::{device_model::DeviceModel, event_api::test_support::SimDevice};

#[test]
#[ignore = "opens a window and waits until it is closed"]
fn open_the_window_with_simulated_devices() {
    let list = std::env::var("SDECK_SIMULATE_MODEL").unwrap_or_else(|_| "plus-xl".into());
    let models: Vec<&'static DeviceModel> = list
        .split(',')
        .map(|id| DeviceModel::by_id_supported(id.trim()).expect("a supported model id"))
        .collect();
    assert!(!models.is_empty(), "SDECK_SIMULATE_MODEL names no model");
    let Some(pair) = start_pair_with("simulated-devices", |roots| {
        remember(roots, models[0]);
        for model in &models {
            let root = roots.root_for(model);
            std::fs::create_dir_all(&root).unwrap();
            let mut store = LayerStore::open_at_model(&root, model).unwrap();
            for key in [3u8, 9, 20, 36] {
                if key <= model.keys() {
                    store
                        .bind(
                            "Base",
                            Trigger::KeyPress(key),
                            Action::Emit(format!("demo-{key}")),
                        )
                        .unwrap();
                }
            }
            store.persist().unwrap();
        }
    }) else {
        eprintln!("no display: nothing to open");
        return;
    };
    let devices: Vec<SimDevice> = models.iter().map(|model| SimDevice::new(model)).collect();
    for (index, device) in devices.iter().enumerate() {
        pair.bus.plug(&format!("/dev/hidraw{}", index + 1), device);
    }
    pair.wait_devices(devices.len());
    pair.window.present();
    eprintln!("Simulated {list} connected through the real hub. Close the window to finish.");
    let main_loop = glib::MainLoop::new(None, false);
    let quit = main_loop.clone();
    pair.window.connect_close_request(move |_| {
        quit.quit();
        glib::Propagation::Proceed
    });
    main_loop.run();
}
