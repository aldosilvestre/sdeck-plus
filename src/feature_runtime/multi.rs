//! Multi action runs (feature task T3). A binding runs its steps strictly in
//! order: a step starts only after the one before it finished, wherever it ran
//! (GTK thread, layer worker, plugin worker or a wait). The first failure ends
//! the run. A binding has at most one run at a time; pressing it again while it
//! runs is ignored. Waits are GLib timeouts, so nothing ever sleeps.
use super::*;
use crate::plugin_runtime::Rejected;
use sdeck_plus::features::MultiStep;
use std::collections::BTreeMap;

/// What the run of a binding does next.
#[derive(Debug, PartialEq)]
pub(super) enum Next {
    /// Run `action` now under its own binding id; `token` reports its end.
    Action {
        id: String,
        action: Action,
        token: u64,
    },
    /// Pause for `duration`, then report `token` as finished.
    Wait { duration: Duration, token: u64 },
    /// Every step succeeded: the binding's page return may start now.
    Finished(Option<PageReturn>),
}

struct Run<H> {
    steps: Vec<MultiStep>,
    page_return: Option<PageReturn>,
    /// The step in flight, or the one to start next when none is.
    current: usize,
    in_flight: Option<u64>,
    /// The timer of a pending wait, so a cancellation can remove it.
    timer: Option<H>,
}

/// The runs in progress, by binding id. `H` is the handle of a wait's timer.
pub(super) struct Runs<H> {
    runs: BTreeMap<String, Run<H>>,
    last_token: u64,
}

impl<H> Default for Runs<H> {
    fn default() -> Self {
        Self {
            runs: BTreeMap::new(),
            last_token: 0,
        }
    }
}

impl<H> Runs<H> {
    /// Starts a run for `binding_id`; false while that binding's run is still going.
    pub(super) fn begin(
        &mut self,
        binding_id: &str,
        steps: Vec<MultiStep>,
        page_return: Option<PageReturn>,
    ) -> bool {
        if self.runs.contains_key(binding_id) {
            return false;
        }
        self.runs.insert(
            binding_id.to_owned(),
            Run {
                steps,
                page_return,
                current: 0,
                in_flight: None,
                timer: None,
            },
        );
        true
    }

    /// Puts the next step of `binding_id`'s run in flight. `None` while a step is
    /// still in flight or when the binding has no run.
    pub(super) fn advance(&mut self, binding_id: &str) -> Option<Next> {
        let run = self.runs.get_mut(binding_id)?;
        if run.in_flight.is_some() {
            return None;
        }
        let Some(step) = run.steps.get(run.current) else {
            let run = self.runs.remove(binding_id)?;
            return Some(Next::Finished(run.page_return));
        };
        self.last_token += 1;
        let token = self.last_token;
        run.in_flight = Some(token);
        Some(match step {
            MultiStep::Wait { milliseconds } => Next::Wait {
                duration: Duration::from_millis(u64::from(*milliseconds)),
                token,
            },
            MultiStep::Action(action) => Next::Action {
                id: format!("{binding_id}#step{}", run.current + 1),
                action: action.clone(),
                token,
            },
        })
    }

    /// Keeps the timer of the wait `token`. A timer no run waits on is handed back.
    pub(super) fn hold_timer(&mut self, token: u64, timer: H) -> Option<H> {
        match self
            .runs
            .values_mut()
            .find(|run| run.in_flight == Some(token))
        {
            Some(run) => {
                run.timer = Some(timer);
                None
            }
            None => Some(timer),
        }
    }

    /// Ends the step `token`: `Ok(binding id)` when its run goes on, `Err(message)`
    /// when the step failed and its run stopped. `None` for a token no run is
    /// waiting on, such as one of a cancelled run.
    pub(super) fn finish(
        &mut self,
        token: u64,
        result: Result<(), String>,
    ) -> Option<Result<String, String>> {
        let (binding_id, run) = self
            .runs
            .iter_mut()
            .find(|(_, run)| run.in_flight == Some(token))?;
        let binding_id = binding_id.clone();
        run.in_flight = None;
        // A wait's timer has fired by now; it must not be removed again.
        run.timer = None;
        match result {
            Ok(()) => {
                run.current += 1;
                Some(Ok(binding_id))
            }
            Err(error) => {
                let position = run.current + 1;
                self.runs.remove(&binding_id);
                Some(Err(format!(
                    "Multi action stopped at step {position}: {error}"
                )))
            }
        }
    }

    /// Ends every run; returns the timers of pending waits so they can be removed.
    pub(super) fn cancel_all(&mut self) -> Vec<H> {
        std::mem::take(&mut self.runs)
            .into_values()
            .filter_map(|run| run.timer)
            .collect()
    }

    #[cfg(test)]
    pub(super) fn is_running(&self, binding_id: &str) -> bool {
        self.runs.contains_key(binding_id)
    }
}

impl FeatureRuntime {
    /// Starts `binding_id`'s sequence; its page return starts after the last
    /// step succeeded. Ignored while that binding's sequence still runs.
    pub(super) fn start_multi(
        &self,
        binding_id: String,
        steps: Vec<MultiStep>,
        page_return: Option<PageReturn>,
    ) {
        if !self
            .multi
            .borrow_mut()
            .begin(&binding_id, steps, page_return)
        {
            return;
        }
        self.status.set_text("Multi action started.");
        self.continue_multi(&binding_id);
    }

    /// Runs the steps of `binding_id` until one has to be waited for.
    fn continue_multi(&self, binding_id: &str) {
        loop {
            let next = self.multi.borrow_mut().advance(binding_id);
            match next {
                None => return,
                Some(Next::Finished(page_return)) => {
                    self.report("Multi action finished.", false);
                    if let Some(value) = page_return {
                        self.schedule_page_return(value);
                    }
                    return;
                }
                Some(Next::Wait { duration, token }) => {
                    let runtime = self.clone();
                    let timer = glib::timeout_add_local_once(duration, move || {
                        runtime.step_finished(token, Ok(()));
                    });
                    if let Some(orphan) = self.multi.borrow_mut().hold_timer(token, timer) {
                        orphan.remove();
                    }
                    return;
                }
                Some(Next::Action { id, action, token }) => {
                    match self.dispatch_step(id, action, token) {
                        // A worker reports the end of the step.
                        Ok(true) => return,
                        Ok(false) => {
                            if self.multi.borrow_mut().finish(token, Ok(())).is_none() {
                                return;
                            }
                        }
                        Err(error) => {
                            self.step_finished(token, Err(error));
                            return;
                        }
                    }
                }
            }
        }
    }

    /// The end of a step a worker or a wait ran: continue its run, or stop it
    /// and say which step failed. A step of a cancelled run changes nothing.
    pub(crate) fn step_finished(&self, token: u64, result: Result<(), String>) {
        let outcome = self.multi.borrow_mut().finish(token, result);
        match outcome {
            None => {}
            Some(Ok(binding_id)) => self.continue_multi(&binding_id),
            Some(Err(message)) => {
                self.report(&message, true);
                self.ui.toast.show(&message, true);
            }
        }
    }

    /// Starts one step. `Ok(true)` when a worker reports its end with `token`,
    /// `Ok(false)` when it already finished here on the GTK thread.
    fn dispatch_step(&self, id: String, action: Action, token: u64) -> Result<bool, String> {
        match action {
            Action::SwitchLayer(target) => self.switch_layer(&target).map(|()| false),
            Action::Emit(name) => self.emit(name).map(|()| false),
            Action::Feature(FeatureAction::Navigation(action)) => {
                self.try_navigate(&action).map(|_| false)
            }
            Action::Feature(FeatureAction::Device(action)) => {
                self.send_device(action).map(|()| false)
            }
            Action::Feature(FeatureAction::Plugin(binding)) => {
                let name = format!("{}/{}", binding.plugin_id, binding.action_id);
                let submitted = self
                    .plugins
                    .borrow()
                    .as_ref()
                    .map(|sender| sender.submit_step(&binding, token));
                match submitted {
                    Some(Ok(_)) => Ok(true),
                    Some(Err(rejected)) => Err(plugin_rejection(&name, rejected)),
                    None => Err(plugin_rejection(&name, Rejected::Stopped)),
                }
            }
            Action::Feature(FeatureAction::Obs(_)) if !self.obs_valid.get() => {
                Err("OBS settings are unavailable.".into())
            }
            Action::Feature(FeatureAction::Desktop(_) | FeatureAction::Obs(_))
            | Action::Launch { .. }
            | Action::Command(_)
            | Action::KeyPress { .. }
            | Action::Shortcut { .. } => self
                .send_command(LayerCommand::ExecuteStep {
                    binding_id: id,
                    action,
                    step: token,
                })
                .map(|()| true),
            Action::None
            | Action::Feature(
                FeatureAction::Touchscreen(_)
                | FeatureAction::Timer { .. }
                | FeatureAction::CancelTimer
                | FeatureAction::Multi { .. },
            ) => Err("this action cannot be a step of a multi action".into()),
        }
    }

    /// Ends every sequence, as a configuration import does; their pending
    /// waits are removed and late worker reports are ignored.
    pub(crate) fn cancel_multi_runs(&self) {
        let timers = self.multi.borrow_mut().cancel_all();
        for timer in timers {
            timer.remove();
        }
    }

    #[cfg(test)]
    pub(crate) fn multi_running(&self, binding_id: &str) -> bool {
        self.multi.borrow().is_running(binding_id)
    }
}

/// Why a plugin request was not queued, as the status says it.
pub(super) fn plugin_rejection(name: &str, rejected: Rejected) -> String {
    match rejected {
        Rejected::Busy => {
            format!("Plugin {name} was not queued: too many plugin requests are waiting.")
        }
        Rejected::Stopped => "Plugin actions are unavailable.".into(),
    }
}

#[cfg(test)]
mod tests;
