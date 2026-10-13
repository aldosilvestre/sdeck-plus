use super::{audio, Backend, MediaCommand, ScrollDirection};
use crate::layers::{ProcessRunner, SystemProcessRunner};
use std::{
    io::{self, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
};

pub(super) struct Native {
    process: SystemProcessRunner,
    audio: audio::Owner<Child>,
    errors_tx: Sender<String>,
    errors_rx: Receiver<String>,
    #[cfg(target_os = "linux")]
    mouse: Option<super::mouse::Device<super::mouse::Native>>,
}

impl Default for Native {
    fn default() -> Self {
        let (errors_tx, errors_rx) = mpsc::channel();
        Self {
            process: SystemProcessRunner,
            audio: audio::Owner::empty(),
            errors_tx,
            errors_rx,
            #[cfg(target_os = "linux")]
            mouse: None,
        }
    }
}

impl Native {
    fn collect_completed_audio(&mut self) -> io::Result<()> {
        if let Some(error) = self.audio.poll_error()? {
            self.errors_tx.send(error).map_err(io::Error::other)?;
        }
        Ok(())
    }
}

impl Backend for Native {
    fn run(&mut self, argv: &[String]) -> io::Result<()> {
        self.process.run(argv)
    }

    fn media(&mut self, command: MediaCommand) -> io::Result<()> {
        super::mpris::execute(command)
    }

    fn open(&mut self, target: &str) -> io::Result<()> {
        let mut child = Command::new("xdg-open")
            .arg(target)
            .stdin(Stdio::null())
            .spawn()?;
        let errors = self.errors_tx.clone();
        // xdg-open can remain attached to its application. Reap outside the
        // serial worker without killing the application on runtime drop.
        std::thread::spawn(move || {
            if let Some(error) = open_exit_error(child.wait()) {
                if let Err(error) = errors.send(error) {
                    // Runtime was dropped before this application completed.
                    eprintln!(
                        "Stream Deck background error after runtime drop: {}",
                        error.0
                    );
                }
            }
        });
        Ok(())
    }

    fn clipboard(&mut self, value: &str) -> io::Result<()> {
        if std::env::var_os("WAYLAND_DISPLAY").is_none_or(|value| value.is_empty()) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Text requires a Wayland compositor (WAYLAND_DISPLAY) and wl-copy",
            ));
        }
        let child = Command::new("wl-copy")
            .args(["--type", "text/plain;charset=utf-8"])
            .stdin(Stdio::piped())
            .spawn()?;
        // Keep a cleanup guard for the pipe-write and wait error paths.
        let mut child = ClipboardChild {
            child,
            completed: false,
        };
        let mut input =
            child.child.stdin.take().ok_or_else(|| {
                io::Error::other("wl-copy did not provide its requested stdin pipe")
            })?;
        input.write_all(value.as_bytes())?;
        drop(input);
        let status = child.child.wait()?;
        child.completed = true;
        // wl-copy daemonizes by default after establishing clipboard ownership;
        // its foreground parent exits before paste is sent.
        if status.success() {
            Ok(())
        } else {
            // The child is already reaped. This is not a fallback clipboard path.
            Err(io::Error::other(format!("wl-copy exited with {status}")))
        }
    }

    fn play_audio(&mut self, path: &Path) -> io::Result<()> {
        let path = path.canonicalize()?;
        let metadata = path.metadata()?;
        if !metadata.is_file() {
            return Err(super::invalid("audio path must name a regular file"));
        }
        self.collect_completed_audio()?;
        self.audio.stop()?;
        let child = Command::new("mpv")
            .args([
                "--no-config",
                "--no-video",
                "--no-terminal",
                "--input-terminal=no",
                "--audio-display=no",
                "--",
            ])
            .arg(&path)
            .stdin(Stdio::null())
            .spawn()?;
        self.audio.install(child);
        Ok(())
    }

    fn stop_audio(&mut self) -> io::Result<()> {
        self.collect_completed_audio()?;
        self.audio.stop()
    }

    fn poll_errors(&mut self) -> Vec<String> {
        let mut errors: Vec<_> = self.errors_rx.try_iter().collect();
        match self.audio.poll_error() {
            Ok(Some(error)) => errors.push(error),
            Ok(None) => {}
            Err(error) => errors.push(format!("Cannot inspect owned mpv audio: {error}")),
        }
        errors
    }

    fn scroll(&mut self, direction: ScrollDirection, ticks: u16) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        {
            if self.mouse.is_none() {
                self.mouse = Some(super::mouse::Device::new(super::mouse::Native::open()?)?);
            }
            let device = self
                .mouse
                .as_mut()
                .ok_or_else(|| io::Error::other("no wheel device"))?;
            let result = device.scroll(direction, ticks);
            if result.is_err() {
                // A partial frame must not leave a reusable broken device.
                self.mouse = None;
            }
            result
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (direction, ticks);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "mouse wheel requires Linux uinput",
            ))
        }
    }
}

fn open_exit_error(result: io::Result<std::process::ExitStatus>) -> Option<String> {
    match result {
        Ok(status) if status.success() => None,
        Ok(status) => Some(format!("xdg-open exited with {status}")),
        Err(error) => Some(format!("Cannot reap xdg-open: {error}")),
    }
}

struct ClipboardChild {
    child: Child,
    completed: bool,
}

impl Drop for ClipboardChild {
    fn drop(&mut self) {
        if !self.completed {
            if let Err(error) = self.child.kill() {
                eprintln!("Cannot terminate failed wl-copy: {error}");
            }
            if let Err(error) = self.child.wait() {
                eprintln!("Cannot reap failed wl-copy: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

    #[test]
    fn async_open_failure_is_classified_from_exit_status() {
        // Given an xdg-open helper that failed.
        let status = ExitStatus::from_raw(3 << 8);
        // When its completion is classified.
        let error = open_exit_error(Ok(status));
        // Then its failure is available to the worker.
        assert!(error.is_some());
    }

    #[test]
    fn async_open_success_is_not_an_error() {
        // Given a successful xdg-open helper exit.
        let status = ExitStatus::from_raw(0);
        // When its completion is classified.
        let error = open_exit_error(Ok(status));
        // Then no error is enqueued.
        assert!(error.is_none());
    }

    #[test]
    fn async_open_wait_failure_is_not_silently_discarded() {
        // Given a wait boundary error.
        let result = Err(io::Error::other("wait failed"));
        // When its completion is classified.
        let error = open_exit_error(result);
        // Then the worker can surface the failure.
        assert!(error.is_some());
    }

    #[test]
    fn native_error_queue_delivers_each_exit_only_once() {
        // Given a background completion already in the native channel.
        let mut backend = Native::default();
        backend.errors_tx.send("failure".into()).unwrap();
        // When the worker polls errors.
        let errors = backend.poll_errors();
        // Then the queued completion is drained exactly once without live I/O.
        assert_eq!(errors.len(), 1);
        assert!(backend.poll_errors().is_empty());
    }
}
