//! Physical keys share the existing GTK input tick, never a second worker.
use super::*;
use sdeck_plus::{key_hold::KeyDispatch, layers::Trigger};

impl FeatureRuntime {
    pub(crate) fn cancel_key_holds(&self) {
        self.key_hold.borrow_mut().cancel();
        self.key_generation
            .set(self.key_generation.get().wrapping_add(1));
    }

    pub(crate) fn poll_key_holds(&self) {
        if self.configuration_operation.get() == Some(configuration::Operation::Import) {
            return;
        }
        let dispatches = self
            .key_hold
            .borrow_mut()
            .poll(&self.layers.borrow(), self.key_epoch.elapsed());
        self.dispatch_keys(dispatches);
    }

    pub(super) fn dispatch_keys(&self, dispatches: Vec<KeyDispatch>) {
        let generation = self.key_generation.get();
        for dispatch in dispatches {
            // A navigation can switch away and back within one execution. The
            // generation also invalidates batches when the final owner is equal.
            if self.key_generation.get() != generation {
                break;
            }
            let key = match dispatch.trigger {
                Trigger::KeyPress(key) | Trigger::KeyHold(key) => key,
                _ => continue,
            };
            let seconds = match &dispatch.action {
                Action::Feature(FeatureAction::Timer { seconds }) => Some(*seconds),
                _ => None,
            };
            let timer_was_active = self.timer.borrow().is_some();
            self.execute_with_return(
                crate::binding_identity(&dispatch.owner, &dispatch.trigger),
                dispatch.action,
                dispatch.page_return,
            );
            if !timer_was_active && self.timer.borrow().is_some() {
                if let Some(seconds) = seconds {
                    self.artwork
                        .start_key_countdown(dispatch.owner, key, seconds);
                }
            }
        }
    }
}
