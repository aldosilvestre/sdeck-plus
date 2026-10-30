//! Exclusive physical keypad tap/hold recognition with caller-owned monotonic time.
use crate::layers::{Action, LayerStore, PageReturn, Trigger};
use std::time::Duration;

pub const HOLD_DURATION: Duration = Duration::from_secs(2);

/// Binding values captured at key-down, not looked up when dispatching later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyDispatch {
    pub owner: String,
    pub trigger: Trigger,
    pub action: Action,
    pub page_return: Option<PageReturn>,
}

impl KeyDispatch {
    fn snapshot(store: &LayerStore, trigger: Trigger) -> Option<Self> {
        let owner = store.binding_layer(store.active(), &trigger)?.name.clone();
        Some(Self {
            action: store.binding(&owner, &trigger)?,
            page_return: store.page_return(&owner, &trigger),
            owner,
            trigger,
        })
    }
}

struct Pending {
    started: Duration,
    tap: Option<KeyDispatch>,
    hold: KeyDispatch,
}

#[derive(Default)]
struct KeyState {
    down: bool,
    pending: Option<Pending>,
}

/// Owned by the existing serial input worker; creates no threads or timers.
/// Feed every physical key report (zero-based HID index), including releases.
/// Repeated down/up snapshots are harmless. Use one monotonic epoch throughout.
#[derive(Default)]
pub struct KeyHoldRecognizer {
    owner: Option<String>,
    keys: [KeyState; 8],
}

impl KeyHoldRecognizer {
    /// A configured hold defers tap until release. At or after the boundary,
    /// release dispatches hold even if the worker has not polled in the meantime.
    /// Invalid HID indices are ignored without changing state.
    pub fn update(
        &mut self,
        store: &LayerStore,
        index: usize,
        pressed: bool,
        now: Duration,
    ) -> Vec<KeyDispatch> {
        if index >= self.keys.len() {
            return Vec::new();
        }
        self.sync_owner(store);
        let key = &mut self.keys[index];
        if key.down == pressed {
            // A down snapshot may arrive at the deadline before the next poll.
            return Self::due(key, now).into_iter().collect();
        }
        key.down = pressed;
        if !pressed {
            let Some(pending) = key.pending.take() else {
                return Vec::new();
            };
            return if now.saturating_sub(pending.started) >= HOLD_DURATION {
                vec![pending.hold]
            } else {
                pending.tap.into_iter().collect()
            };
        }
        let number = index as u8 + 1;
        let tap = KeyDispatch::snapshot(store, Trigger::KeyPress(number));
        if let Some(hold) = KeyDispatch::snapshot(store, Trigger::KeyHold(number)) {
            key.pending = Some(Pending {
                started: now,
                tap,
                hold,
            });
            Vec::new()
        } else {
            tap.into_iter().collect()
        }
    }

    /// Call from the worker's existing tick, before dispatching other events.
    /// Check owner changes before polling, never execute returned actions while
    /// holding the layer-store lock. Each hold is emitted at most once.
    pub fn poll(&mut self, store: &LayerStore, now: Duration) -> Vec<KeyDispatch> {
        self.sync_owner(store);
        self.keys
            .iter_mut()
            .filter_map(|key| Self::due(key, now))
            .collect()
    }

    /// Cancel on owner changes, disconnect, shutdown and configuration import.
    /// Keep down state: a canceled held key stays suppressed until its release.
    /// Do not replace the recognizer with Default on reconnect/import.
    pub fn cancel(&mut self) {
        for key in &mut self.keys {
            key.pending = None;
        }
    }

    /// Earliest absolute deadline in the caller's monotonic epoch, if pending.
    pub fn next_deadline(&self) -> Option<Duration> {
        self.keys
            .iter()
            .filter_map(|key| key.pending.as_ref()?.started.checked_add(HOLD_DURATION))
            .min()
    }

    fn sync_owner(&mut self, store: &LayerStore) {
        if self.owner.as_deref() != Some(store.active()) {
            self.cancel();
            self.owner = Some(store.active().to_owned());
        }
    }

    fn due(key: &mut KeyState, now: Duration) -> Option<KeyDispatch> {
        let pending = key.pending.as_ref()?;
        if now.saturating_sub(pending.started) < HOLD_DURATION {
            return None;
        }
        key.pending.take().map(|pending| pending.hold)
    }
}
