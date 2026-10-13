//! Run inside dbus-run-session to avoid controlling the user's media players.
use gtk::{gio, glib};
use sdeck_plus::{
    desktop_actions::{DesktopAction, DesktopRuntime, MediaCommand},
    keyboard::Keyboard,
};
use std::{
    cell::Cell,
    error::Error,
    process::{Child, Command, Stdio},
    rc::Rc,
    time::Duration,
};

struct Player(Child);

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let clip = std::env::args()
        .nth(1)
        .ok_or("Pass a local silent audio fixture")?;
    let bus = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)?;
    let event_loop = glib::MainLoop::new(None, false);
    let ready = Rc::new(Cell::new(false));
    let observed = ready.clone();
    let waiting = event_loop.clone();
    let subscription = bus.signal_subscribe(
        Some("org.freedesktop.DBus"),
        Some("org.freedesktop.DBus"),
        Some("NameOwnerChanged"),
        Some("/org/freedesktop/DBus"),
        None,
        gio::DBusSignalFlags::NONE,
        move |_, _, _, _, _, parameters| {
            if let Some((name, _, owner)) = parameters.get::<(String, String, String)>() {
                if name.starts_with("org.mpris.MediaPlayer2.mpv") && !owner.is_empty() {
                    observed.set(true);
                    waiting.quit();
                }
            }
        },
    );
    let _player = Player(
        Command::new("mpv")
            .args([
                "--no-config",
                "--load-scripts=no",
                "--script=/usr/lib/mpv-mpris/mpris.so",
                "--ao=null",
                "--idle=yes",
                "--loop-file=inf",
                "--no-video",
                "--",
                &clip,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let waiting = event_loop.clone();
    let deadline = glib::timeout_add_local_once(Duration::from_secs(5), move || waiting.quit());
    event_loop.run();
    if ready.get() {
        deadline.remove();
    }
    bus.signal_unsubscribe(subscription);
    if !ready.get() {
        return Err("Owned mpv did not register its MPRIS service".into());
    }
    let mut runtime = DesktopRuntime::new();
    let mut keyboard = Keyboard::new();
    for command in [
        MediaCommand::PlayPause,
        MediaCommand::PlayPause,
        MediaCommand::Next,
        MediaCommand::Previous,
        MediaCommand::Stop,
    ] {
        runtime.execute(&DesktopAction::Media(command), &mut keyboard)?;
        println!("MEDIA_OK: {command:?}, owned mpv MPRIS on isolated bus");
    }
    runtime.execute(
        &DesktopAction::PlayAudio { path: clip.into() },
        &mut keyboard,
    )?;
    runtime.execute(&DesktopAction::StopAudio, &mut keyboard)?;
    println!("SOUNDBOARD_OK: production mpv spawn and owned stop");
    Ok(())
}
