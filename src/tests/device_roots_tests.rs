//! The layer worker reads and saves OBS settings in the root it was started on.
use super::*;
use sdeck_plus::device_roots::data_root_in;
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("sdeck-worker-root-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn save_through_worker(root: PathBuf, endpoint: &str) {
    let (commands, requests) = mpsc::channel();
    let (events, results) = mpsc::channel();
    let worker = thread::spawn(move || {
        feature_runtime::run_worker_at(requests, events, Default::default(), Some(root));
    });
    commands
        .send(LayerCommand::ObsSave(sdeck_plus::obs::ObsConfig {
            endpoint: endpoint.into(),
            password: String::new(),
        }))
        .unwrap();
    // The bounded receive is the completion signal for the save.
    let event = results.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(
        matches!(&event, MonitorEvent::AppEvent { name } if name == "OBS settings saved."),
        "{event:?}"
    );
    commands.send(LayerCommand::Shutdown).unwrap();
    worker.join().unwrap();
}

#[test]
fn obs_settings_are_saved_in_the_root_the_worker_runs_on() {
    let base = scratch("obs");
    let plus = base.join("sdeck-plus");
    let xl = data_root_in(&plus, &sdeck_plus::device_model::PLUS_XL);
    std::fs::create_dir_all(&xl).unwrap();
    std::fs::create_dir_all(&plus).unwrap();
    save_through_worker(xl.clone(), "ws://127.0.0.1:4457");
    save_through_worker(plus.clone(), "ws://127.0.0.1:4456");
    let endpoint = |root: &Path| sdeck_plus::obs::ObsConfig::load_at(root).unwrap().endpoint;
    assert_eq!(endpoint(&xl), "ws://127.0.0.1:4457");
    assert_eq!(endpoint(&plus), "ws://127.0.0.1:4456");
    std::fs::remove_dir_all(base).unwrap();
}
