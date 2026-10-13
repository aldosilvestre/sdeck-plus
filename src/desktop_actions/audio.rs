//! Own only the child launched by this runtime, never a system-wide player.

use std::{
    io,
    process::{Child, ExitStatus},
};

pub(super) trait AudioChild {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>>;
    fn kill(&mut self) -> io::Result<()>;
    fn reap(&mut self) -> io::Result<()>;
}

impl AudioChild for Child {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        Child::try_wait(self)
    }
    fn kill(&mut self) -> io::Result<()> {
        Child::kill(self)
    }
    fn reap(&mut self) -> io::Result<()> {
        self.wait().map(|_| ())
    }
}

#[derive(Default)]
pub(super) struct Owner<C: AudioChild> {
    child: Option<C>,
}

impl<C: AudioChild> Owner<C> {
    pub(super) fn empty() -> Self {
        Self { child: None }
    }

    pub(super) fn install(&mut self, child: C) {
        // The adapter stops the previous child before spawning a replacement.
        debug_assert!(self.child.is_none());
        self.child = Some(child);
    }

    pub(super) fn stop(&mut self) -> io::Result<()> {
        if let Some(child) = &mut self.child {
            if child.try_wait()?.is_none() {
                // SIGKILL applies only to this owned child. Wait only after
                // termination, never for normal playback completion.
                child.kill()?;
                child.reap()?;
            }
            self.child = None;
        }
        Ok(())
    }

    pub(super) fn poll_error(&mut self) -> io::Result<Option<String>> {
        let Some(child) = &mut self.child else {
            return Ok(None);
        };
        let Some(status) = child.try_wait()? else {
            return Ok(None);
        };
        self.child = None;
        Ok((!status.success()).then(|| format!("mpv audio exited with {status}")))
    }
}

impl<C: AudioChild> Drop for Owner<C> {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("Cannot clean up owned Stream Deck audio child: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::{cell::RefCell, rc::Rc};

    #[derive(Default)]
    struct State {
        exited: bool,
        exit_code: i32,
        fail_kill: bool,
        kills: usize,
        reaps: usize,
    }
    struct Fake(Rc<RefCell<State>>);
    impl AudioChild for Fake {
        fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
            let state = self.0.borrow();
            Ok(state
                .exited
                .then(|| ExitStatus::from_raw(state.exit_code << 8)))
        }
        fn kill(&mut self) -> io::Result<()> {
            let mut state = self.0.borrow_mut();
            if state.fail_kill {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "kill denied",
                ));
            }
            state.kills += 1;
            Ok(())
        }
        fn reap(&mut self) -> io::Result<()> {
            self.0.borrow_mut().reaps += 1;
            Ok(())
        }
    }

    #[test]
    fn stop_terminates_and_reaps_only_owned_child() {
        // Given an owned running child and a different unrelated child.
        let state = Rc::new(RefCell::new(State::default()));
        let unrelated = Rc::new(RefCell::new(State::default()));
        let mut owner = Owner::empty();
        owner.install(Fake(state.clone()));
        // When owned audio is stopped.
        owner.stop().unwrap();
        // Then only the owned child is killed and reaped.
        assert_eq!((state.borrow().kills, state.borrow().reaps), (1, 1));
        assert_eq!((unrelated.borrow().kills, unrelated.borrow().reaps), (0, 0));
        assert!(owner.child.is_none());
    }

    #[test]
    fn stop_does_not_kill_when_child_already_exited() {
        // Given try_wait already reaped a completed child.
        let state = Rc::new(RefCell::new(State {
            exited: true,
            ..State::default()
        }));
        let mut owner = Owner::empty();
        owner.install(Fake(state.clone()));
        // When owned audio is stopped.
        owner.stop().unwrap();
        // Then no signal or extra wait occurs.
        assert_eq!((state.borrow().kills, state.borrow().reaps), (0, 0));
        assert!(owner.child.is_none());
    }

    #[test]
    fn stop_retains_ownership_when_termination_fails() {
        // Given a kill boundary failure.
        let state = Rc::new(RefCell::new(State {
            fail_kill: true,
            ..State::default()
        }));
        let mut owner = Owner::empty();
        owner.install(Fake(state.clone()));
        // When stop fails.
        let error = owner.stop().unwrap_err();
        // Then ownership is retained for subsequent cleanup, with no wait on playback.
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(owner.child.is_some());
        assert_eq!(state.borrow().reaps, 0);
        state.borrow_mut().fail_kill = false; // Drop can now clean up.
    }

    #[test]
    fn drop_cleans_up_running_owned_audio() {
        // Given an owned running child.
        let state = Rc::new(RefCell::new(State::default()));
        let mut owner = Owner::empty();
        owner.install(Fake(state.clone()));
        // When the runtime's audio owner is dropped.
        drop(owner);
        // Then the child is killed and reaped exactly once.
        assert_eq!((state.borrow().kills, state.borrow().reaps), (1, 1));
    }

    #[test]
    fn polling_failed_audio_exit_reports_once_and_releases_ownership() {
        // Given a child that exited with decode failure.
        let state = Rc::new(RefCell::new(State {
            exited: true,
            exit_code: 2,
            ..State::default()
        }));
        let mut owner = Owner::empty();
        owner.install(Fake(state.clone()));
        // When completion is polled.
        let error = owner.poll_error().unwrap();
        // Then the failure is reported once and the reaped child is released.
        assert!(error.is_some());
        assert!(owner.child.is_none());
        assert!(owner.poll_error().unwrap().is_none());
        assert_eq!((state.borrow().kills, state.borrow().reaps), (0, 0));
    }

    #[test]
    fn polling_running_audio_never_waits_for_completion() {
        // Given ongoing playback.
        let state = Rc::new(RefCell::new(State::default()));
        let mut owner = Owner::empty();
        owner.install(Fake(state.clone()));
        // When completion is polled.
        let error = owner.poll_error().unwrap();
        // Then ownership remains and no blocking wait or termination occurs.
        assert!(error.is_none());
        assert!(owner.child.is_some());
        assert_eq!((state.borrow().kills, state.borrow().reaps), (0, 0));
    }

    #[test]
    fn polling_successful_audio_exit_releases_without_error() {
        // Given successful completed playback.
        let state = Rc::new(RefCell::new(State {
            exited: true,
            ..State::default()
        }));
        let mut owner = Owner::empty();
        owner.install(Fake(state));
        // When completion is polled.
        let error = owner.poll_error().unwrap();
        // Then completed playback is reaped without an error or signal.
        assert!(error.is_none());
        assert!(owner.child.is_none());
    }
}
