//! Multi action runs (feature task T3): strict order across the GTK thread and
//! the workers, non-blocking waits, stop on the first failure, one run per binding.
use super::key_hold_ui_tests::HoldFixture;
use super::*;
use sdeck_plus::{
    device_control::DeviceAction,
    features::{FeatureAction, MultiStep},
    layers::PageReturn,
    organization::NavigationAction,
};
use std::time::Instant;

const BOUND: Duration = Duration::from_secs(5);

fn multi(steps: Vec<MultiStep>) -> Action {
    Action::Feature(FeatureAction::Multi { steps })
}

fn step(action: Action) -> MultiStep {
    MultiStep::Action(action)
}

/// Runs `command` on a real layer worker thread of its own and returns what it reported.
fn run_on_worker(command: LayerCommand) -> Vec<MonitorEvent> {
    let (sender, receiver) = mpsc::channel();
    let (output, events) = mpsc::channel();
    sender.send(command).unwrap();
    sender.send(LayerCommand::Shutdown).unwrap();
    thread::Builder::new()
        .name("layer-worker".into())
        .spawn(move || feature_runtime::run_worker(receiver, output, Default::default()))
        .unwrap()
        .join()
        .unwrap();
    events.try_iter().collect()
}

/// Await `expected` on `label`; the timeout only guards against a hang.
fn await_text(label: &Label, expected: &str) {
    if label.text() == expected {
        return;
    }
    let loop_ = glib::MainLoop::new(None, false);
    let done = loop_.clone();
    let wanted = expected.to_owned();
    let signal = label.connect_label_notify(move |label| {
        if label.text() == wanted {
            done.quit();
        }
    });
    let expired = Rc::new(Cell::new(false));
    let flag = expired.clone();
    let guard_loop = loop_.clone();
    let guard = glib::timeout_add_local_once(BOUND, move || {
        flag.set(true);
        guard_loop.quit();
    });
    loop_.run();
    if !expired.get() {
        guard.remove();
    }
    label.disconnect(signal);
    assert!(!expired.get(), "timed out awaiting {expected:?}");
}

/// A window with its runtime connected to fake device and event outputs; layer
/// commands stay in `hold.requests` until a test hands them to a worker.
struct Fixture {
    hold: HoldFixture,
    surface: EventSurface,
    emitted: mpsc::Receiver<MonitorEvent>,
    device: mpsc::Receiver<DeviceAction>,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let hold = HoldFixture::new(tag);
        hold.layers.borrow_mut().create("Other").unwrap();
        let organization = {
            let layers = hold.layers.borrow();
            sdeck_plus::organization::Organization::open_at(layers.root(), &layers)
        };
        let (device, device_requests) = mpsc::channel();
        let (events, emitted) = mpsc::channel();
        hold.runtime.configure(
            organization,
            Ok(sdeck_plus::obs::ObsConfig::default()),
            device,
            events,
        );
        let widgets = hold.widgets.as_ref().unwrap();
        let surface = EventSurface {
            status: widgets.status.clone(),
            connection_detail: widgets.connection_detail.clone(),
            toast: widgets.toast.clone(),
            binding_status: widgets.binding_status.clone(),
            feature_runtime: widgets.feature_runtime.clone(),
            preview: widgets.preview.clone(),
            touch_status: widgets.touch_status.clone(),
            artwork: hold.artwork.clone(),
            organization_writes: widgets.organization_writes.clone(),
            ticks: dial_totals(),
        };
        let fixture = Self {
            hold,
            surface,
            emitted,
            device: device_requests,
        };
        fixture.executed();
        fixture
    }

    fn runtime(&self) -> &feature_runtime::FeatureRuntime {
        &self.hold.runtime
    }

    fn toast(&self) -> &ui::Toast {
        &self.hold.widgets.as_ref().unwrap().toast
    }

    fn active(&self) -> String {
        self.hold.layers.borrow().active().to_owned()
    }

    /// The actions handed to the layer worker since the last call; saves are skipped.
    fn executed(&self) -> Vec<LayerCommand> {
        self.hold
            .requests
            .try_iter()
            .filter(|command| {
                matches!(
                    command,
                    LayerCommand::Execute { .. } | LayerCommand::ExecuteStep { .. }
                )
            })
            .collect()
    }

    /// The one step the layer worker was handed since the last call.
    fn worker_step(&self) -> (String, Action, u64) {
        match self.executed().as_slice() {
            [LayerCommand::ExecuteStep {
                binding_id,
                action,
                step,
            }] => (binding_id.clone(), action.clone(), *step),
            _ => panic!("exactly one step was expected on the layer worker"),
        }
    }

    fn emitted(&self) -> Vec<String> {
        self.emitted
            .try_iter()
            .filter_map(|event| match event {
                MonitorEvent::AppEvent { name } => Some(name),
                _ => None,
            })
            .collect()
    }

    fn apply(&self, events: Vec<MonitorEvent>) {
        for event in events {
            self.surface.apply(event, true);
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // A wait still pending would keep the runtime alive past the window.
        self.hold.runtime.cancel_multi_runs();
    }
}

#[test]
fn the_layer_worker_reports_a_step_by_its_token_instead_of_a_status() {
    let (sender, receiver) = mpsc::channel();
    let (output, events) = mpsc::channel();
    for (step, command) in [(7, "true"), (8, "false")] {
        sender
            .send(LayerCommand::ExecuteStep {
                binding_id: format!("4:Base:KeyPress(1)#step{step}"),
                action: Action::Command(command.into()),
                step,
            })
            .unwrap();
    }
    sender.send(LayerCommand::Shutdown).unwrap();
    feature_runtime::run_worker(receiver, output, Default::default());
    assert_eq!(
        events.try_iter().collect::<Vec<_>>(),
        [
            MonitorEvent::StepFinished {
                step: 7,
                result: Ok(())
            },
            MonitorEvent::ActionDispatched,
            MonitorEvent::StepFinished {
                step: 8,
                result: Err("process exited with exit status: 1".into())
            },
        ]
    );
}

#[test]
fn a_single_layer_action_still_reports_its_status_and_completion() {
    let (sender, receiver) = mpsc::channel();
    let (output, events) = mpsc::channel();
    for command in ["true", "false"] {
        sender
            .send(LayerCommand::Execute {
                binding_id: "4:Base:KeyPress(1)".into(),
                action: Action::Command(command.into()),
            })
            .unwrap();
    }
    sender.send(LayerCommand::Shutdown).unwrap();
    feature_runtime::run_worker(receiver, output, Default::default());
    assert_eq!(
        events.try_iter().collect::<Vec<_>>(),
        [
            MonitorEvent::AppEvent {
                name: "Action dispatched.".into()
            },
            MonitorEvent::ActionDispatched,
            MonitorEvent::LayerError(
                "Could not execute layer action: process exited with exit status: 1".into()
            ),
        ]
    );
}

#[test]
fn a_gtk_step_waits_until_the_worker_finished_the_step_before_it() {
    run_gtk_test(|| {
        let fixture = Fixture::new("multi-order");
        let runtime = fixture.runtime();
        let id = binding_identity("Base", &Trigger::KeyPress(1));
        let written = fixture.hold.root.join("multi-order.txt");
        runtime.execute(
            id.clone(),
            multi(vec![
                step(Action::Command(format!(
                    "printf done > '{}'",
                    written.display()
                ))),
                step(Action::Emit("after the command".into())),
                step(Action::SwitchLayer("Other".into())),
            ]),
        );
        let (binding_id, action, token) = fixture.worker_step();
        assert_eq!(binding_id, format!("{id}#step1"));
        // Until the worker reports the command finished, nothing after it runs.
        assert!(fixture.emitted().is_empty());
        assert_eq!(fixture.active(), "Base");
        let reported = run_on_worker(LayerCommand::ExecuteStep {
            binding_id,
            action,
            step: token,
        });
        assert_eq!(std::fs::read_to_string(&written).unwrap(), "done");
        assert_eq!(
            reported,
            [
                MonitorEvent::StepFinished {
                    step: token,
                    result: Ok(())
                },
                MonitorEvent::ActionDispatched
            ]
        );
        assert!(fixture.emitted().is_empty() && fixture.active() == "Base");
        fixture.apply(reported);
        // Then the GTK-thread steps run in order.
        assert_eq!(fixture.emitted(), ["after the command"]);
        assert_eq!(fixture.active(), "Other");
        assert_eq!(runtime.status.text(), "Multi action finished.");
        assert!(fixture.executed().is_empty());
    });
}

#[test]
fn a_wait_step_delays_the_next_step_without_blocking_gtk_or_the_worker() {
    run_gtk_test(|| {
        let fixture = Fixture::new("multi-wait");
        let runtime = fixture.runtime();
        let started = Instant::now();
        runtime.execute(
            "4:Base:KeyPress(1)".into(),
            multi(vec![
                MultiStep::Wait { milliseconds: 300 },
                step(Action::SwitchLayer("Other".into())),
            ]),
        );
        // The call returned during the wait, before the step after it.
        assert_eq!(fixture.active(), "Base");
        // The worker still takes other actions right away.
        runtime.execute("4:Base:KeyPress(2)".into(), Action::Command("true".into()));
        assert!(matches!(
            fixture.executed().as_slice(),
            [LayerCommand::Execute { binding_id, .. }] if binding_id == "4:Base:KeyPress(2)"
        ));
        assert_eq!(runtime.status.text(), "Action queued.");
        // The GTK main loop keeps running while the sequence waits.
        await_text(&runtime.status, "Multi action finished.");
        assert!(started.elapsed() >= Duration::from_millis(300));
        assert_eq!(fixture.active(), "Other");
    });
}

#[test]
fn a_failed_step_stops_the_sequence_and_names_it_in_the_error_toast() {
    run_gtk_test(|| {
        let fixture = Fixture::new("multi-failure");
        let runtime = fixture.runtime();
        let id = String::from("4:Base:KeyPress(1)");
        runtime.execute_with_return(
            id.clone(),
            multi(vec![
                step(Action::Emit("first".into())),
                step(Action::Command("false".into())),
                step(Action::Emit("never".into())),
            ]),
            Some(PageReturn {
                layer: "Other".into(),
                seconds: 60,
            }),
        );
        assert_eq!(fixture.emitted(), ["first"]);
        let (binding_id, action, token) = fixture.worker_step();
        assert_eq!(binding_id, format!("{id}#step2"));
        fixture.apply(run_on_worker(LayerCommand::ExecuteStep {
            binding_id,
            action,
            step: token,
        }));
        let expected = "Multi action stopped at step 2: process exited with exit status: 1";
        let toast = fixture.toast();
        assert_eq!(toast.label.text(), expected);
        assert!(toast.root.reveals_child());
        assert!(toast.tone.has_css_class("feedback-error"));
        assert_eq!(runtime.status.text(), expected);
        assert!(runtime.status.has_css_class("feedback-error"));
        // The remaining steps never ran and the page return was not armed.
        assert!(fixture.emitted().is_empty());
        assert!(fixture.executed().is_empty());
        assert!(!runtime.page_return_active());
        assert_eq!(fixture.active(), "Base");

        // A step failing on the GTK thread stops the run the same way; the
        // binding runs again because the failed run ended.
        runtime.execute(
            id,
            multi(vec![
                step(Action::Emit("again".into())),
                step(Action::SwitchLayer("Missing page".into())),
            ]),
        );
        assert_eq!(fixture.emitted(), ["again"]);
        assert_eq!(
            toast.label.text(),
            "Multi action stopped at step 2: Destination profile or page is unavailable."
        );
        assert!(toast.tone.has_css_class("feedback-error"));
        assert_eq!(fixture.active(), "Base");
    });
}

#[test]
fn a_plugin_step_waits_for_the_plugin_result_and_its_failure_stops_the_run() {
    run_gtk_test(|| {
        let fixture = Fixture::new("multi-plugin");
        let runtime = fixture.runtime();
        let plugins = fixture.hold.root.join("plugins");
        std::fs::create_dir_all(&plugins).unwrap();
        fixture
            .hold
            .widgets
            .as_ref()
            .unwrap()
            .binding_ui
            .plugins
            .load(plugins);
        let (events, results) = mpsc::channel();
        let mut worker = runtime.start_plugins(&events).unwrap().unwrap();
        runtime.execute(
            "4:Base:KeyPress(1)".into(),
            multi(vec![
                step(Action::Feature(FeatureAction::Plugin(
                    sdeck_plus::plugins::PluginBinding::new(
                        "demo",
                        "run",
                        sdeck_plus::plugins::Settings::new(),
                    ),
                ))),
                step(Action::Emit("after the plugin".into())),
            ]),
        );
        let reported = results.recv_timeout(BOUND).unwrap();
        let MonitorEvent::StepFinished {
            result: Err(error), ..
        } = &reported
        else {
            panic!("the plugin worker reports the step: {reported:?}");
        };
        assert!(
            error.starts_with("Plugin demo/run failed: ") && error.contains("not installed"),
            "{error}"
        );
        let expected = format!("Multi action stopped at step 1: {error}");
        assert!(fixture.emitted().is_empty(), "the plugin had not finished");
        fixture.apply(vec![reported]);
        assert_eq!(fixture.toast().label.text(), expected);
        assert_eq!(runtime.status.text(), expected);
        assert!(fixture.emitted().is_empty());
        assert!(worker.shutdown());

        // Without a plugin worker the step fails at once, naming its position.
        let fixture = Fixture::new("multi-no-plugin");
        fixture.runtime().execute(
            "4:Base:KeyPress(1)".into(),
            multi(vec![
                step(Action::Emit("before".into())),
                step(Action::Feature(FeatureAction::Plugin(
                    sdeck_plus::plugins::PluginBinding::new(
                        "demo",
                        "run",
                        sdeck_plus::plugins::Settings::new(),
                    ),
                ))),
            ]),
        );
        assert_eq!(fixture.emitted(), ["before"]);
        assert_eq!(
            fixture.toast().label.text(),
            "Multi action stopped at step 2: Plugin actions are unavailable."
        );
    });
}

#[test]
fn pressing_the_key_again_while_its_sequence_runs_is_ignored() {
    run_gtk_test(|| {
        let fixture = Fixture::new("multi-repress");
        let runtime = fixture.runtime();
        let sequence = multi(vec![
            step(Action::Command("true".into())),
            step(Action::Emit("done".into())),
        ]);
        for key in [1, 2] {
            fixture
                .hold
                .layers
                .borrow_mut()
                .bind("Base", Trigger::KeyPress(key), sequence.clone())
                .unwrap();
        }
        let press = |index| {
            for pressed in [true, false] {
                runtime.execute_input(&protocol::Event::Key { index, pressed });
            }
        };
        let first = binding_identity("Base", &Trigger::KeyPress(1));
        press(0);
        press(0);
        let (binding_id, _, token) = fixture.worker_step();
        assert_eq!(binding_id, format!("{first}#step1"));
        // Another key's sequence runs independently.
        press(1);
        let (other, _, other_token) = fixture.worker_step();
        assert_eq!(
            other,
            format!("{}#step1", binding_identity("Base", &Trigger::KeyPress(2)))
        );
        assert_ne!(token, other_token);
        fixture.apply(vec![MonitorEvent::StepFinished {
            step: token,
            result: Ok(()),
        }]);
        assert_eq!(fixture.emitted(), ["done"]);
        // Once its sequence finished, the key runs it again.
        press(0);
        let (again, _, next) = fixture.worker_step();
        assert_eq!(again, binding_id);
        assert!(next != token && next != other_token);
    });
}

#[test]
fn the_page_return_starts_only_after_the_last_step_succeeded() {
    run_gtk_test(|| {
        let fixture = Fixture::new("multi-return");
        let runtime = fixture.runtime();
        runtime.execute_with_return(
            "4:Base:KeyPress(1)".into(),
            multi(vec![
                step(Action::Emit("first".into())),
                step(Action::Command("true".into())),
            ]),
            Some(PageReturn {
                layer: "Other".into(),
                seconds: 60,
            }),
        );
        assert_eq!(fixture.emitted(), ["first"]);
        let (_, _, token) = fixture.worker_step();
        assert!(!runtime.page_return_active(), "the last step still runs");
        fixture.apply(vec![MonitorEvent::StepFinished {
            step: token,
            result: Ok(()),
        }]);
        assert!(runtime.page_return_active());
        assert_eq!(fixture.active(), "Base");
        runtime.cancel_timer();
    });
}

#[test]
fn a_configuration_import_cancels_running_sequences() {
    run_gtk_test(|| {
        let fixture = Fixture::new("multi-import");
        let runtime = fixture.runtime();
        let working = String::from("4:Base:KeyPress(1)");
        let waiting = String::from("4:Base:KeyPress(2)");
        let sequence = multi(vec![
            step(Action::Command("true".into())),
            step(Action::Emit("after".into())),
        ]);
        runtime.execute(working.clone(), sequence.clone());
        let (_, _, token) = fixture.worker_step();
        runtime.execute(
            waiting.clone(),
            multi(vec![
                MultiStep::Wait {
                    milliseconds: 60_000,
                },
                step(Action::Emit("late".into())),
            ]),
        );
        assert!(runtime.multi_running(&working) && runtime.multi_running(&waiting));
        runtime.import_configuration(fixture.hold.root.join("missing.sdeck-plus.json"));
        assert!(matches!(
            fixture.hold.requests.try_recv().unwrap(),
            LayerCommand::ImportConfiguration { .. }
        ));
        assert!(!runtime.multi_running(&working) && !runtime.multi_running(&waiting));
        // The step that was running reports back: nothing after it runs.
        fixture.apply(vec![MonitorEvent::StepFinished {
            step: token,
            result: Ok(()),
        }]);
        assert!(fixture.emitted().is_empty());
        let root = fixture.hold.layers.borrow().root().to_owned();
        fixture.apply(vec![MonitorEvent::ConfigurationImportResult {
            root,
            result: Err("missing file".into()),
        }]);
        // The cancelled binding starts afresh.
        runtime.execute(working.clone(), sequence);
        let (binding_id, _, again) = fixture.worker_step();
        assert_eq!(binding_id, format!("{working}#step1"));
        assert_ne!(again, token);
    });
}

#[test]
fn single_actions_keep_their_dispatch_status_and_immediate_page_return() {
    run_gtk_test(|| {
        let fixture = Fixture::new("multi-single");
        let runtime = fixture.runtime();
        let id = String::from("4:Base:KeyPress(1)");
        runtime.execute(id.clone(), Action::Command("true".into()));
        assert!(matches!(
            fixture.executed().as_slice(),
            [LayerCommand::Execute { binding_id, action: Action::Command(command) }]
                if *binding_id == id && command == "true"
        ));
        assert_eq!(runtime.status.text(), "Action queued.");
        runtime.execute(id.clone(), Action::Emit("named".into()));
        assert_eq!(fixture.emitted(), ["named"]);
        runtime.execute(id.clone(), Action::SwitchLayer("Missing page".into()));
        assert_eq!(
            runtime.status.text(),
            "Destination profile or page is unavailable."
        );
        assert!(runtime.status.has_css_class("feedback-error"));
        runtime.execute(
            id.clone(),
            Action::Feature(FeatureAction::Device(DeviceAction::Brightness(40))),
        );
        assert_eq!(
            fixture.device.try_recv().unwrap(),
            DeviceAction::Brightness(40)
        );
        assert_eq!(runtime.status.text(), "Device action queued.");
        runtime.execute(
            id.clone(),
            Action::Feature(FeatureAction::Navigation(NavigationAction::NextPage)),
        );
        assert_eq!(
            runtime.status.text(),
            "No matching page or folder in this view."
        );
        runtime.execute(
            id.clone(),
            Action::Feature(FeatureAction::Plugin(
                sdeck_plus::plugins::PluginBinding::new(
                    "demo",
                    "run",
                    sdeck_plus::plugins::Settings::new(),
                ),
            )),
        );
        assert_eq!(runtime.status.text(), "Plugin actions are unavailable.");
        runtime.execute(id.clone(), Action::SwitchLayer("Other".into()));
        assert_eq!(fixture.active(), "Other");
        assert!(fixture.executed().is_empty());
        // A single action's page return starts as soon as it is dispatched.
        runtime.execute_with_return(
            id.clone(),
            Action::Command("true".into()),
            Some(PageReturn {
                layer: "Base".into(),
                seconds: 60,
            }),
        );
        assert_eq!(fixture.executed().len(), 1);
        assert!(runtime.page_return_active());
        runtime.cancel_timer();
    });
}

#[test]
fn wrong_duplicate_and_stale_step_tokens_are_ignored_while_a_run_is_in_flight() {
    run_gtk_test(|| {
        let fixture = Fixture::new("multi-tokens");
        let runtime = fixture.runtime();
        let id = "4:Base:KeyPress(1)";
        runtime.execute(
            id.into(),
            multi(vec![
                step(Action::Command("true".into())),
                step(Action::Command("true".into())),
                step(Action::Emit("end".into())),
            ]),
        );
        let (_, _, first) = fixture.worker_step();
        // A token no step was given changes nothing, whatever it reports.
        fixture.apply(vec![MonitorEvent::StepFinished {
            step: first + 1000,
            result: Ok(()),
        }]);
        fixture.apply(vec![MonitorEvent::StepFinished {
            step: first + 1000,
            result: Err("bogus".into()),
        }]);
        assert!(fixture.executed().is_empty());
        assert!(runtime.multi_running(id));
        assert_ne!(
            fixture.toast().label.text(),
            "Multi action stopped at step 1: bogus"
        );
        fixture.apply(vec![MonitorEvent::StepFinished {
            step: first,
            result: Ok(()),
        }]);
        let (second_id, _, second) = fixture.worker_step();
        assert_eq!(second_id, format!("{id}#step2"));
        // A duplicate of the first token must not skip step 2.
        fixture.apply(vec![MonitorEvent::StepFinished {
            step: first,
            result: Ok(()),
        }]);
        assert!(fixture.emitted().is_empty() && fixture.executed().is_empty());
        assert!(runtime.multi_running(id));
        fixture.apply(vec![MonitorEvent::StepFinished {
            step: second,
            result: Ok(()),
        }]);
        assert_eq!(fixture.emitted(), ["end"]);
        assert!(!runtime.multi_running(id));
        assert_eq!(runtime.status.text(), "Multi action finished.");
        // A late report after the run finished stops nothing and says nothing.
        fixture.apply(vec![MonitorEvent::StepFinished {
            step: second,
            result: Err("late".into()),
        }]);
        assert_ne!(
            fixture.toast().label.text(),
            "Multi action stopped at step 2: late"
        );
        assert_eq!(runtime.status.text(), "Multi action finished.");
    });
}

#[test]
fn cancelling_after_a_wait_fired_twice_or_mid_wait_runs_nothing_more() {
    run_gtk_test(|| {
        let fixture = Fixture::new("multi-cancel");
        let runtime = fixture.runtime();
        let finished = "4:Base:KeyPress(1)";
        let waiting = "4:Base:KeyPress(2)";
        runtime.execute(
            finished.into(),
            multi(vec![
                MultiStep::Wait { milliseconds: 10 },
                step(Action::Emit("a".into())),
            ]),
        );
        assert_eq!(runtime.status.text(), "Multi action started.");
        await_text(&runtime.status, "Multi action finished.");
        // The fired timer is not removed again, however often runs are cancelled.
        runtime.cancel_multi_runs();
        runtime.cancel_multi_runs();
        assert_eq!(fixture.emitted(), ["a"]);
        // Cancelled mid-wait, twice: the step after the wait never runs.
        runtime.execute(
            waiting.into(),
            multi(vec![
                MultiStep::Wait { milliseconds: 100 },
                step(Action::Emit("b".into())),
            ]),
        );
        assert!(runtime.multi_running(waiting));
        runtime.cancel_multi_runs();
        runtime.cancel_multi_runs();
        assert!(!runtime.multi_running(waiting));
        // The cancelled binding runs afresh; its longer wait outlasts the cancelled one.
        runtime.execute(
            waiting.into(),
            multi(vec![
                MultiStep::Wait { milliseconds: 150 },
                step(Action::Emit("c".into())),
            ]),
        );
        assert_eq!(runtime.status.text(), "Multi action started.");
        await_text(&runtime.status, "Multi action finished.");
        assert_eq!(fixture.emitted(), ["c"]);
        assert!(!runtime.multi_running(waiting));
    });
}

#[test]
fn inline_steps_chain_and_navigation_runs_last_after_a_worker_step() {
    run_gtk_test(|| {
        let fixture = Fixture::new("multi-inline");
        let runtime = fixture.runtime();
        let id = "4:Base:KeyPress(1)";
        runtime.execute_with_return(
            id.into(),
            multi(vec![
                step(Action::Emit("1".into())),
                step(Action::Emit("2".into())),
                step(Action::Feature(FeatureAction::Device(
                    DeviceAction::Brightness(40),
                ))),
                step(Action::Command("true".into())),
                step(Action::Feature(FeatureAction::Navigation(
                    NavigationAction::NextPage,
                ))),
            ]),
            Some(PageReturn {
                layer: "Base".into(),
                seconds: 60,
            }),
        );
        // The GTK-thread and device steps ran in one go, up to the worker step.
        assert_eq!(fixture.emitted(), ["1", "2"]);
        assert_eq!(
            fixture.device.try_iter().collect::<Vec<_>>(),
            [DeviceAction::Brightness(40)]
        );
        let (step_id, _, token) = fixture.worker_step();
        assert_eq!(step_id, format!("{id}#step4"));
        assert!(runtime.multi_running(id));
        assert!(!runtime.page_return_active());
        // Finishing it inside apply runs the navigation last and ends the run.
        fixture.apply(vec![MonitorEvent::StepFinished {
            step: token,
            result: Ok(()),
        }]);
        assert_eq!(runtime.status.text(), "Multi action finished.");
        assert!(!runtime.multi_running(id));
        assert!(runtime.page_return_active());
        assert!(fixture.executed().is_empty());
        runtime.cancel_timer();
    });
}
