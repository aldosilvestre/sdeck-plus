//! Probe a running OBS instance; never start streaming or recording.
use sdeck_plus::obs::{Client, ObsAction, ObsConfig};
use std::{error::Error, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let root = std::env::args()
        .nth(1)
        .ok_or("Pass the app data directory containing obs.json")?;
    let config = ObsConfig::load_at(Path::new(&root))?;
    let mut client = Client::new(config);
    let status = client.probe()?;
    println!(
        "OBS_CONNECTED: current scene {:?}, {} scenes, recording={}, streaming={}",
        status.current_scene,
        status.scenes.len(),
        status.recording,
        status.streaming
    );
    client.execute(&ObsAction::Scene(status.current_scene.clone()))?;
    println!("OBS_REQUEST_OK: SetCurrentProgramScene");
    Ok(())
}
