use super::*;
use sdeck_plus::layers::{Action, PageReturn};

const KEY: &str = "4:Base:KeyPress(1)";

fn steps() -> Vec<MultiStep> {
    vec![
        MultiStep::Action(Action::Command("true".into())),
        MultiStep::Wait { milliseconds: 250 },
        MultiStep::Action(Action::Emit("after".into())),
    ]
}

fn back() -> Option<PageReturn> {
    Some(PageReturn {
        layer: "Base".into(),
        seconds: 60,
    })
}

fn action(runs: &mut Runs<u32>, binding: &str) -> (String, Action, u64) {
    match runs.advance(binding) {
        Some(Next::Action { id, action, token }) => (id, action, token),
        other => panic!("an action step was expected, not {other:?}"),
    }
}

fn wait(runs: &mut Runs<u32>, binding: &str) -> (Duration, u64) {
    match runs.advance(binding) {
        Some(Next::Wait { duration, token }) => (duration, token),
        other => panic!("a wait step was expected, not {other:?}"),
    }
}

#[test]
fn steps_start_one_at_a_time_in_order_under_their_own_binding_ids() {
    let mut runs = Runs::<u32>::default();
    assert!(runs.begin(KEY, steps(), back()));
    let (id, first, command) = action(&mut runs, KEY);
    assert_eq!(id, format!("{KEY}#step1"));
    assert_eq!(first, Action::Command("true".into()));
    // Nothing else starts while the first step runs.
    assert_eq!(runs.advance(KEY), None);
    assert_eq!(runs.finish(command, Ok(())), Some(Ok(KEY.to_owned())));
    let (duration, pause) = wait(&mut runs, KEY);
    assert_eq!(duration, Duration::from_millis(250));
    assert_ne!(pause, command);
    assert_eq!(runs.advance(KEY), None);
    assert_eq!(runs.finish(pause, Ok(())), Some(Ok(KEY.to_owned())));
    let (id, last, emit) = action(&mut runs, KEY);
    assert_eq!(id, format!("{KEY}#step3"));
    assert_eq!(last, Action::Emit("after".into()));
    assert!(runs.is_running(KEY));
    assert_eq!(runs.finish(emit, Ok(())), Some(Ok(KEY.to_owned())));
    // The page return is handed over only after the last step succeeded, once.
    assert_eq!(runs.advance(KEY), Some(Next::Finished(back())));
    assert!(!runs.is_running(KEY));
    assert_eq!(runs.advance(KEY), None);
}

#[test]
fn a_failed_step_ends_the_run_and_names_its_position() {
    let mut runs = Runs::<u32>::default();
    assert!(runs.begin(KEY, steps(), back()));
    let (_, _, command) = action(&mut runs, KEY);
    assert_eq!(runs.finish(command, Ok(())), Some(Ok(KEY.to_owned())));
    let (_, pause) = wait(&mut runs, KEY);
    assert_eq!(runs.finish(pause, Ok(())), Some(Ok(KEY.to_owned())));
    let (_, _, emit) = action(&mut runs, KEY);
    assert_eq!(
        runs.finish(emit, Err("Event delivery failed: closed".into())),
        Some(Err(
            "Multi action stopped at step 3: Event delivery failed: closed".into()
        ))
    );
    assert!(!runs.is_running(KEY));
    // No Finished: a failed run never starts its page return.
    assert_eq!(runs.advance(KEY), None);

    assert!(runs.begin(KEY, steps(), back()));
    let (_, _, command) = action(&mut runs, KEY);
    assert_eq!(
        runs.finish(command, Err("process exited with exit status: 1".into())),
        Some(Err(
            "Multi action stopped at step 1: process exited with exit status: 1".into()
        ))
    );
    assert_eq!(runs.advance(KEY), None);
    assert!(!runs.is_running(KEY));
}

#[test]
fn a_binding_runs_once_at_a_time_and_other_bindings_are_independent() {
    let other = "4:Base:KeyPress(2)";
    let mut runs = Runs::<u32>::default();
    assert!(runs.begin(KEY, steps(), None));
    // Pressed again while running: ignored, the running steps are kept.
    assert!(!runs.begin(
        KEY,
        vec![MultiStep::Action(Action::Emit("other".into()))],
        back()
    ));
    assert!(runs.begin(other, steps(), None));
    let (id, first, mine) = action(&mut runs, KEY);
    assert_eq!(
        (id, first),
        (format!("{KEY}#step1"), Action::Command("true".into()))
    );
    let (id, _, theirs) = action(&mut runs, other);
    assert_eq!(id, format!("{other}#step1"));
    assert_ne!(mine, theirs);
    assert_eq!(runs.finish(theirs, Ok(())), Some(Ok(other.to_owned())));
    // Finishing the other binding's step does not move this one.
    assert_eq!(runs.advance(KEY), None);
    assert_eq!(runs.finish(mine, Ok(())), Some(Ok(KEY.to_owned())));
    let (_, pause) = wait(&mut runs, KEY);
    assert_eq!(runs.finish(pause, Ok(())), Some(Ok(KEY.to_owned())));
    let (_, _, emit) = action(&mut runs, KEY);
    assert_eq!(runs.finish(emit, Ok(())), Some(Ok(KEY.to_owned())));
    assert_eq!(runs.advance(KEY), Some(Next::Finished(None)));
    // Once finished, the binding can run again.
    assert!(runs.begin(KEY, steps(), None));
}

#[test]
fn unknown_or_stale_tokens_change_nothing() {
    let mut runs = Runs::<u32>::default();
    assert_eq!(runs.finish(99, Ok(())), None);
    assert!(runs.begin(KEY, steps(), None));
    let (_, _, command) = action(&mut runs, KEY);
    assert_eq!(runs.finish(command + 100, Ok(())), None);
    assert_eq!(runs.finish(command + 100, Err("late".into())), None);
    assert!(runs.is_running(KEY));
    assert_eq!(runs.advance(KEY), None, "the command is still running");
    assert_eq!(runs.finish(command, Ok(())), Some(Ok(KEY.to_owned())));
    // A token reports once.
    assert_eq!(runs.finish(command, Ok(())), None);
}

#[test]
fn cancelling_ends_every_run_and_hands_back_the_pending_wait_timers() {
    let waiting = "4:Base:KeyPress(2)";
    let mut runs = Runs::<u32>::default();
    assert!(runs.begin(
        waiting,
        vec![
            MultiStep::Wait {
                milliseconds: 60_000
            },
            MultiStep::Action(Action::Emit("late".into())),
        ],
        back()
    ));
    let (_, pause) = wait(&mut runs, waiting);
    assert_eq!(runs.hold_timer(pause, 7), None, "the run keeps its timer");
    assert!(runs.begin(KEY, steps(), back()));
    let (_, _, command) = action(&mut runs, KEY);
    assert_eq!(runs.cancel_all(), vec![7]);
    assert!(!runs.is_running(KEY) && !runs.is_running(waiting));
    // What was in flight no longer belongs to any run.
    assert_eq!(runs.finish(command, Ok(())), None);
    assert_eq!(runs.finish(pause, Ok(())), None);
    assert_eq!(runs.advance(KEY), None);
    // A timer for a run that is gone is handed back to be removed.
    assert_eq!(runs.hold_timer(pause, 8), Some(8));
    assert!(runs.begin(KEY, steps(), back()));
}

#[test]
fn a_wait_that_ended_forgets_its_timer_instead_of_handing_it_back() {
    let mut runs = Runs::<u32>::default();
    assert!(runs.begin(KEY, steps(), None));
    let (_, _, command) = action(&mut runs, KEY);
    assert_eq!(runs.finish(command, Ok(())), Some(Ok(KEY.to_owned())));
    let (_, pause) = wait(&mut runs, KEY);
    assert_eq!(runs.hold_timer(pause, 5), None);
    assert_eq!(runs.finish(pause, Ok(())), Some(Ok(KEY.to_owned())));
    // The fired timer is gone already; removing it again would be an error.
    assert_eq!(runs.cancel_all(), Vec::<u32>::new());
}

#[test]
fn a_duplicate_finish_cannot_skip_the_step_now_in_flight() {
    let mut runs = Runs::<u32>::default();
    assert!(runs.begin(KEY, steps(), None));
    let (_, _, command) = action(&mut runs, KEY);
    assert_eq!(runs.finish(command, Ok(())), Some(Ok(KEY.to_owned())));
    let (_, pause) = wait(&mut runs, KEY);
    // A duplicate or late report of the first step while the wait is in flight.
    assert_eq!(runs.finish(command, Ok(())), None);
    assert_eq!(runs.finish(command, Err("late".into())), None);
    assert_eq!(runs.advance(KEY), None, "the wait still runs");
    assert_eq!(runs.finish(pause, Ok(())), Some(Ok(KEY.to_owned())));
}

#[test]
fn tokens_stay_unique_across_runs_cancels_and_failures() {
    let mut runs = Runs::<u32>::default();
    let mut seen = std::collections::BTreeSet::new();
    for round in 0..5 {
        assert!(runs.begin(KEY, steps(), None));
        let (_, _, token) = action(&mut runs, KEY);
        assert!(seen.insert(token), "token {token} reused");
        match round % 3 {
            0 => assert_eq!(runs.cancel_all(), Vec::<u32>::new()),
            1 => assert_eq!(
                runs.finish(token, Err("x".into())),
                Some(Err("Multi action stopped at step 1: x".into()))
            ),
            _ => {
                assert_eq!(runs.finish(token, Ok(())), Some(Ok(KEY.to_owned())));
                let (_, pause) = wait(&mut runs, KEY);
                assert!(seen.insert(pause), "token {pause} reused");
                assert_eq!(runs.cancel_all(), Vec::<u32>::new());
            }
        }
        // A stale token of the previous run never touches the new run.
        assert!(runs.begin(KEY, steps(), None));
        let (_, _, fresh) = action(&mut runs, KEY);
        assert!(seen.insert(fresh), "token {fresh} reused");
        assert_eq!(runs.finish(token, Ok(())), None);
        assert!(runs.is_running(KEY));
        assert_eq!(runs.advance(KEY), None, "the fresh step still runs");
        assert_eq!(runs.cancel_all(), Vec::<u32>::new());
    }
}

#[test]
fn a_finished_run_hands_back_a_late_timer() {
    let mut runs = Runs::<u32>::default();
    assert!(runs.begin(KEY, steps(), None));
    let (_, _, command) = action(&mut runs, KEY);
    assert_eq!(runs.finish(command, Ok(())), Some(Ok(KEY.to_owned())));
    let (_, pause) = wait(&mut runs, KEY);
    assert_eq!(runs.finish(pause, Ok(())), Some(Ok(KEY.to_owned())));
    // A timer registered for a wait that already ended is orphaned: handed back.
    assert_eq!(runs.hold_timer(pause, 3), Some(3));
}
