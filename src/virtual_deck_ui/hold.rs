//! A test-only hold on the panel's server starts: while it is held, a start
//! or restart worker waits before it starts its server, so a test can act
//! while the panel is Starting or Restarting. The app never holds anything.
use std::{
    sync::{Arc, Condvar, Mutex, PoisonError},
    time::Duration,
};

/// The longest a worker waits for a test that never lets it go.
const LIMIT: Duration = Duration::from_secs(60);

/// Shared by the panel's workers and the test's [`ServerHold`].
#[derive(Clone, Default)]
pub(crate) struct Gate(Arc<(Mutex<bool>, Condvar)>);

impl Gate {
    /// A gate that holds every worker passing it until it is released.
    pub(super) fn held() -> Self {
        let gate = Self::default();
        *gate.0 .0.lock().unwrap_or_else(PoisonError::into_inner) = true;
        gate
    }

    /// Waits while the gate is held, for at most [`LIMIT`].
    pub(super) fn pass(&self) {
        let (held, turn) = &*self.0;
        let guard = held.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = turn.wait_timeout_while(guard, LIMIT, |held| *held);
    }

    fn release(&self) {
        let (held, turn) = &*self.0;
        *held.lock().unwrap_or_else(PoisonError::into_inner) = false;
        turn.notify_all();
    }
}

/// Lets the held workers go when released or dropped (also on a failing test).
pub(crate) struct ServerHold(Gate);

impl ServerHold {
    pub(super) fn new(gate: Gate) -> Self {
        Self(gate)
    }

    pub(crate) fn release(&self) {
        self.0.release();
    }
}

impl Drop for ServerHold {
    fn drop(&mut self) {
        self.0.release();
    }
}
