//! Independent theme alarm playback; never uses the Play audio action's player.
use gtk::gio;
use std::ffi::OsString;

pub(super) struct TimerSound {
    launcher: gio::SubprocessLauncher,
    argv: Vec<OsString>,
}

impl Default for TimerSound {
    fn default() -> Self {
        Self {
            launcher: gio::SubprocessLauncher::new(
                gio::SubprocessFlags::STDOUT_SILENCE | gio::SubprocessFlags::STDERR_SILENCE,
            ),
            argv: vec![
                "canberra-gtk-play".into(),
                "--id".into(),
                "alarm-clock-elapsed".into(),
                "--description".into(),
                "sdeck-plus timer complete".into(),
            ],
        }
    }
}

impl TimerSound {
    pub(super) fn play(&self, finished: impl FnOnce(bool) + 'static) {
        let argv: Vec<_> = self.argv.iter().map(OsString::as_os_str).collect();
        match self.launcher.spawn(&argv) {
            Ok(child) => {
                // GIO owns/reaps the child; waiting never blocks the GTK main loop.
                child.wait_check_async(gio::Cancellable::NONE, move |result| {
                    if let Err(error) = &result {
                        eprintln!("Timer chime failed: {error}");
                    }
                    finished(result.is_ok());
                });
            }
            Err(error) => {
                eprintln!("Timer chime unavailable (optional canberra-gtk-play): {error}");
                finished(false);
            }
        }
    }

    #[cfg(test)]
    pub(super) fn for_test(launcher: gio::SubprocessLauncher, argv: Vec<OsString>) -> Self {
        Self { launcher, argv }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::glib;
    use std::{cell::Cell, rc::Rc, time::Duration};

    #[test]
    fn playback_reaps_success_and_failure_and_reports_missing_helper() {
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let executable = std::env::current_exe().unwrap().into_os_string();
                for (argv, expected) in [
                    (
                        vec![
                            executable.clone(),
                            "--exact".into(),
                            "tests::timer_notification_tests::mock_timer_chime".into(),
                        ],
                        true,
                    ),
                    (
                        vec![executable, "--invalid-mock-playback-option".into()],
                        false,
                    ),
                    (vec!["/nonexistent/sdeck-plus-test-canberra".into()], false),
                ] {
                    let sound = TimerSound::for_test(
                        gio::SubprocessLauncher::new(
                            gio::SubprocessFlags::STDOUT_SILENCE
                                | gio::SubprocessFlags::STDERR_SILENCE,
                        ),
                        argv,
                    );
                    let loop_ = glib::MainLoop::new(Some(&context), false);
                    let result = Rc::new(Cell::new(None));
                    let received = result.clone();
                    let finished = loop_.clone();
                    sound.play(move |success| {
                        assert!(
                            received.replace(Some(success)).is_none(),
                            "completion exactly once"
                        );
                        finished.quit();
                    });
                    if result.get().is_none() {
                        let guard_loop = loop_.clone();
                        let guard = glib::timeout_source_new(
                            Duration::from_secs(4),
                            None,
                            glib::Priority::DEFAULT,
                            move || {
                                guard_loop.quit();
                                glib::ControlFlow::Break
                            },
                        );
                        guard.attach(Some(&context));
                        loop_.run();
                        guard.destroy();
                    }
                    assert_eq!(result.get(), Some(expected));
                }
            })
            .unwrap();
    }
}
