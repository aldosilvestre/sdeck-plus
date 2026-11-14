//! A dedicated worker for plugin invocations, separate from the GTK thread and from the layer
//! worker, so a stalled plugin never delays built-in actions.
//!
//! Admission is bounded ([`QUEUE_CAPACITY`]); a full queue rejects instead of waiting. Each
//! request runs once, with the immutable binding taken at submission, under the host's
//! per-request deadline ([`REQUEST_TIMEOUT`]); nothing is retried. Results travel on the
//! existing monitor-event channel. Shutdown drops queued requests rather than draining them.
//!
//! Display `query` requests (the value a plugin shows on a control) have a worker of their own
//! with their own bounded queue, so a burst of refreshes never makes a key press fail with
//! [`Rejected::Busy`]. Identical pending queries are coalesced.
use sdeck_plus::{
    event_api::MonitorEvent,
    local_artwork::ArtworkTarget,
    plugins::{
        InvokeOptions, InvokeOutcome, PluginBinding, PluginError, PluginRegistry, QueryOutcome,
    },
};
use std::{
    collections::VecDeque,
    io,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    thread,
    time::Duration,
};

/// Requests waiting behind the one that is running.
pub(crate) const QUEUE_CAPACITY: usize = 8;
/// Distinct display queries waiting behind the one that is running.
pub(crate) const QUERY_CAPACITY: usize = 64;
/// The host kills a plugin's process group at this deadline.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Shutdown waits for a running request at most this long: its deadline plus a margin.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(6);
const POLL: Duration = Duration::from_millis(100);

/// Runs one request; the real worker opens the registry fresh so enablement is read from disk.
pub(crate) type Invoke =
    Box<dyn FnMut(&PluginBinding, &InvokeOptions) -> Result<InvokeOutcome, PluginError> + Send>;

struct Job {
    request_id: String,
    binding: PluginBinding,
}

/// Why a request was not admitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rejected {
    Busy,
    Stopped,
}

/// Cloneable submitting side, held by the session's runtime.
#[derive(Clone)]
pub(crate) struct PluginSender {
    jobs: mpsc::SyncSender<Job>,
    next: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

impl PluginSender {
    /// Queues a snapshot of `binding` without waiting and returns its request id.
    pub(crate) fn submit(&self, binding: &PluginBinding) -> Result<String, Rejected> {
        if self.stop.load(Ordering::Acquire) {
            return Err(Rejected::Stopped);
        }
        let request_id = format!("sdeck-{}", self.next.fetch_add(1, Ordering::Relaxed) + 1);
        let job = Job {
            request_id: request_id.clone(),
            binding: binding.clone(),
        };
        self.jobs.try_send(job).map_err(|error| match error {
            mpsc::TrySendError::Full(_) => Rejected::Busy,
            mpsc::TrySendError::Disconnected(_) => Rejected::Stopped,
        })?;
        Ok(request_id)
    }
}

/// Owning side: stops and joins the worker at shutdown.
pub(crate) struct PluginWorker {
    stop: Arc<AtomicBool>,
    done: mpsc::Receiver<()>,
    handle: Option<thread::JoinHandle<()>>,
}

impl PluginWorker {
    /// Stops admission so queued requests are dropped; shutdown then joins.
    pub(crate) fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    /// Stops admission, drops queued requests and joins the worker, waiting for a running
    /// request at most [`SHUTDOWN_WAIT`]. Returns false when the worker had to be left behind.
    pub(crate) fn shutdown(&mut self) -> bool {
        self.shutdown_within(SHUTDOWN_WAIT)
    }

    fn shutdown_within(&mut self, wait: Duration) -> bool {
        self.stop.store(true, Ordering::Release);
        // A disconnected `done` means the worker already ended, which also allows the join.
        let finished = !matches!(
            self.done.recv_timeout(wait),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        match self.handle.take() {
            Some(handle) if finished => {
                if handle.join().is_err() {
                    eprintln!("Plugin worker panicked");
                }
                true
            }
            _ => finished,
        }
    }
}

/// The worker for plugins under `root`; every request re-reads manifests and enabled state.
pub(crate) fn spawn_for_root(
    root: PathBuf,
    results: mpsc::Sender<MonitorEvent>,
) -> io::Result<(PluginSender, PluginWorker)> {
    spawn(
        Box::new(move |binding, options| PluginRegistry::open(&root).invoke(binding, options)),
        results,
    )
}

pub(crate) fn spawn(
    mut invoke: Invoke,
    results: mpsc::Sender<MonitorEvent>,
) -> io::Result<(PluginSender, PluginWorker)> {
    let (jobs, receiver) = mpsc::sync_channel::<Job>(QUEUE_CAPACITY);
    let stop = Arc::new(AtomicBool::new(false));
    let (finish, done) = mpsc::channel();
    let worker_stop = stop.clone();
    let handle = thread::Builder::new()
        .name("plugin-worker".into())
        .spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                let job = match receiver.recv_timeout(POLL) {
                    Ok(job) => job,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let options = InvokeOptions::new(&job.request_id).timeout(REQUEST_TIMEOUT);
                let event = report(&job, invoke(&job.binding, &options));
                // The plugin may have changed what it displays, whether or not it succeeded.
                let finished = MonitorEvent::PluginFinished {
                    plugin_id: job.binding.plugin_id.clone(),
                };
                for event in [event, finished] {
                    if let Err(error) = results.send(event) {
                        eprintln!("Plugin result delivery failed: {error}");
                    }
                }
            }
            // The owner may already have stopped waiting; a lost signal is harmless.
            let _ = finish.send(());
        })?;
    let sender = PluginSender {
        jobs,
        next: Arc::new(AtomicU64::new(0)),
        stop: stop.clone(),
    };
    let worker = PluginWorker {
        stop,
        done,
        handle: Some(handle),
    };
    Ok((sender, worker))
}

/// Runs one display query; the real worker opens the registry fresh so enablement is read
/// from disk.
pub(crate) type Query =
    Box<dyn FnMut(&PluginBinding, &InvokeOptions) -> Result<QueryOutcome, PluginError> + Send>;

/// What one control asks of a plugin; the answer carries it back so stale replies are dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueryRequest {
    pub(crate) layer: String,
    pub(crate) target: ArtworkTarget,
    pub(crate) binding: PluginBinding,
}

struct QueryShared {
    queue: Mutex<VecDeque<QueryRequest>>,
    wake: Condvar,
    next: AtomicU64,
}

/// Cloneable submitting side of the query worker.
#[derive(Clone)]
pub(crate) struct QuerySender {
    shared: Arc<QueryShared>,
    stop: Arc<AtomicBool>,
}

impl QuerySender {
    /// Queues `request` without waiting. An identical request that is still pending already
    /// covers it, so it is not queued twice.
    pub(crate) fn submit(&self, request: QueryRequest) -> Result<(), Rejected> {
        if self.stop.load(Ordering::Acquire) {
            return Err(Rejected::Stopped);
        }
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if queue.contains(&request) {
            return Ok(());
        }
        if queue.len() >= QUERY_CAPACITY {
            return Err(Rejected::Busy);
        }
        queue.push_back(request);
        self.shared.wake.notify_one();
        Ok(())
    }
}

/// The query worker for plugins under `root`.
pub(crate) fn spawn_queries_for_root(
    root: PathBuf,
    results: mpsc::Sender<MonitorEvent>,
) -> io::Result<(QuerySender, PluginWorker)> {
    spawn_queries(
        Box::new(move |binding, options| PluginRegistry::open(&root).query(binding, options)),
        results,
    )
}

pub(crate) fn spawn_queries(
    mut query: Query,
    results: mpsc::Sender<MonitorEvent>,
) -> io::Result<(QuerySender, PluginWorker)> {
    let shared = Arc::new(QueryShared {
        queue: Mutex::new(VecDeque::new()),
        wake: Condvar::new(),
        next: AtomicU64::new(0),
    });
    let stop = Arc::new(AtomicBool::new(false));
    let (finish, done) = mpsc::channel();
    let (worker_shared, worker_stop) = (shared.clone(), stop.clone());
    let handle = thread::Builder::new()
        .name("plugin-query-worker".into())
        .spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                let request = {
                    let mut queue = worker_shared
                        .queue
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if queue.is_empty() {
                        queue = worker_shared
                            .wake
                            .wait_timeout(queue, POLL)
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .0;
                    }
                    match queue.pop_front() {
                        Some(request) => request,
                        None => continue,
                    }
                };
                let id = format!(
                    "sdeck-q-{}",
                    worker_shared.next.fetch_add(1, Ordering::Relaxed) + 1
                );
                let options = InvokeOptions::new(&id).timeout(REQUEST_TIMEOUT);
                let result = query(&request.binding, &options).map_err(|error| error.to_string());
                let event = MonitorEvent::PluginValue {
                    layer: request.layer,
                    target: request.target,
                    binding: request.binding,
                    result,
                };
                if let Err(error) = results.send(event) {
                    eprintln!("Plugin value delivery failed: {error}");
                }
            }
            let _ = finish.send(());
        })?;
    let worker = PluginWorker {
        stop: stop.clone(),
        done,
        handle: Some(handle),
    };
    Ok((QuerySender { shared, stop }, worker))
}

fn report(job: &Job, result: Result<InvokeOutcome, PluginError>) -> MonitorEvent {
    let name = format!("{}/{}", job.binding.plugin_id, job.binding.action_id);
    match result {
        Ok(outcome) => MonitorEvent::AppEvent {
            name: match outcome.message {
                Some(message) => format!("Plugin {name}: {message}"),
                None => format!("Plugin {name} finished."),
            },
        },
        Err(error) => MonitorEvent::LayerError(format!(
            "Plugin {name} failed: {error} (request {})",
            job.request_id
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdeck_plus::plugins::Settings;
    use std::sync::Mutex;

    const BOUND: Duration = Duration::from_secs(10);

    fn binding(action: &str) -> PluginBinding {
        PluginBinding::new("demo", action, Settings::new())
    }

    fn ok() -> Result<InvokeOutcome, PluginError> {
        Ok(InvokeOutcome {
            request_id: String::new(),
            message: Some("done".into()),
            diagnostics: String::new(),
        })
    }

    /// An invoker that reports each start, then waits for `gate` to open.
    fn gated(
        started: mpsc::Sender<(String, String, Option<String>)>,
        gate: mpsc::Receiver<()>,
    ) -> Invoke {
        Box::new(move |binding, options| {
            let thread_name = thread::current().name().map(str::to_owned);
            started
                .send((
                    binding.action_id.clone(),
                    format!("{options:?}"),
                    thread_name,
                ))
                .unwrap();
            let _ = gate.recv_timeout(BOUND);
            ok()
        })
    }

    #[test]
    fn requests_run_on_the_dedicated_worker_with_unique_ids_and_a_deadline() {
        let (results, events) = mpsc::channel();
        let (started, runs) = mpsc::channel();
        let (open, gate) = mpsc::channel();
        let (sender, mut worker) = spawn(gated(started, gate), results).unwrap();
        let mut snapshot = binding("first");
        let first = sender.submit(&snapshot).unwrap();
        snapshot.action_id = "mutated".into();
        let second = sender.submit(&binding("second")).unwrap();
        assert_ne!(first, second);
        let (action, options, thread_name) = runs.recv_timeout(BOUND).unwrap();
        assert_eq!(action, "first", "the queued snapshot is immutable");
        assert!(
            options.contains(&first) && options.contains("5s"),
            "{options}"
        );
        assert_eq!(thread_name.as_deref(), Some("plugin-worker"));
        open.send(()).unwrap();
        open.send(()).unwrap();
        for _ in 0..2 {
            let MonitorEvent::AppEvent { name } = events.recv_timeout(BOUND).unwrap() else {
                panic!("a success is reported as an app event");
            };
            assert!(matches!(
                events.recv_timeout(BOUND).unwrap(),
                MonitorEvent::PluginFinished { .. }
            ));
            assert!(
                name.starts_with("Plugin demo/") && name.ends_with("done"),
                "{name}"
            );
        }
        assert!(worker.shutdown());
    }

    #[test]
    fn admission_is_bounded_and_never_blocks_the_submitter() {
        let (results, _events) = mpsc::channel();
        let (started, runs) = mpsc::channel();
        let (open, gate) = mpsc::channel();
        let (sender, mut worker) = spawn(gated(started, gate), results).unwrap();
        sender.submit(&binding("running")).unwrap();
        runs.recv_timeout(BOUND).unwrap();
        for _ in 0..QUEUE_CAPACITY {
            sender.submit(&binding("queued")).unwrap();
        }
        assert_eq!(sender.submit(&binding("extra")), Err(Rejected::Busy));
        drop(open);
        assert!(worker.shutdown());
    }

    #[test]
    fn shutdown_drops_queued_requests_instead_of_draining_them() {
        let (results, _events) = mpsc::channel();
        let ran = Arc::new(Mutex::new(Vec::new()));
        let (started, runs) = mpsc::channel::<()>();
        let (open, gate) = mpsc::channel::<()>();
        let log = ran.clone();
        let invoke: Invoke = Box::new(move |binding, _| {
            log.lock().unwrap().push(binding.action_id.clone());
            started.send(()).unwrap();
            let _ = gate.recv_timeout(BOUND);
            ok()
        });
        let (sender, mut worker) = spawn(invoke, results).unwrap();
        sender.submit(&binding("running")).unwrap();
        runs.recv_timeout(BOUND).unwrap();
        sender.submit(&binding("queued-1")).unwrap();
        sender.submit(&binding("queued-2")).unwrap();
        open.send(()).unwrap();
        assert!(worker.shutdown());
        assert_eq!(*ran.lock().unwrap(), ["running"]);
        assert_eq!(sender.submit(&binding("late")), Err(Rejected::Stopped));
    }

    #[test]
    fn a_stalled_request_makes_shutdown_give_up_after_its_bounded_wait() {
        let (results, _events) = mpsc::channel();
        let (started, runs) = mpsc::channel();
        let (open, gate) = mpsc::channel();
        let (sender, mut worker) = spawn(gated(started, gate), results).unwrap();
        sender.submit(&binding("stalled")).unwrap();
        runs.recv_timeout(BOUND).unwrap();
        assert!(!worker.shutdown_within(Duration::from_millis(50)));
        open.send(()).unwrap();
        // The released request finishes and the worker exits on its own.
        assert!(worker.done.recv_timeout(BOUND).is_ok());
    }

    #[test]
    fn failures_are_reported_once_with_their_request_id_and_never_retried() {
        let (results, events) = mpsc::channel();
        let calls = Arc::new(Mutex::new(0u32));
        let counter = calls.clone();
        let invoke: Invoke = Box::new(move |_, _| {
            *counter.lock().unwrap() += 1;
            Err(PluginError::Disabled("demo".into()))
        });
        let (sender, mut worker) = spawn(invoke, results).unwrap();
        let id = sender.submit(&binding("send")).unwrap();
        let MonitorEvent::LayerError(message) = events.recv_timeout(BOUND).unwrap() else {
            panic!("a failure is reported as a layer error");
        };
        assert!(
            message.contains("demo/send") && message.contains("disabled"),
            "{message}"
        );
        assert!(message.contains(&id), "{message}");
        assert!(worker.shutdown());
        assert_eq!(*calls.lock().unwrap(), 1);
    }

    #[test]
    fn the_real_worker_reports_a_missing_plugin_without_running_anything() {
        let root = std::env::temp_dir().join(format!("sdeck-plugin-worker-{}", std::process::id()));
        let (results, events) = mpsc::channel();
        let (sender, mut worker) = spawn_for_root(root, results).unwrap();
        sender.submit(&binding("send")).unwrap();
        let MonitorEvent::LayerError(message) = events.recv_timeout(BOUND).unwrap() else {
            panic!("a missing plugin is reported as a layer error");
        };
        assert!(message.contains("not installed"), "{message}");
        assert!(worker.shutdown());
    }

    // ---- display queries ----------------------------------------------------------------

    fn request(layer: &str, key: u8, action: &str) -> QueryRequest {
        QueryRequest {
            layer: layer.into(),
            target: ArtworkTarget::Keypad(key),
            binding: binding(action),
        }
    }

    fn answer(headline: &str) -> Result<QueryOutcome, PluginError> {
        Ok(QueryOutcome {
            headline: headline.into(),
            detail: Some("more".into()),
            level: Some(40),
        })
    }

    /// A query runner that reports each start, then waits for `gate`.
    fn gated_query(
        started: mpsc::Sender<(String, String, Option<String>)>,
        gate: mpsc::Receiver<()>,
    ) -> Query {
        Box::new(move |binding, options| {
            let thread_name = thread::current().name().map(str::to_owned);
            started
                .send((
                    binding.action_id.clone(),
                    format!("{options:?}"),
                    thread_name,
                ))
                .unwrap();
            let _ = gate.recv_timeout(BOUND);
            answer("40%")
        })
    }

    #[test]
    fn queries_run_on_their_own_worker_with_a_deadline_and_return_the_control_they_answer() {
        let (results, events) = mpsc::channel();
        let (started, runs) = mpsc::channel();
        let (open, gate) = mpsc::channel();
        let (sender, mut worker) = spawn_queries(gated_query(started, gate), results).unwrap();
        sender.submit(request("Base", 3, "show")).unwrap();
        let (action, options, thread_name) = runs.recv_timeout(BOUND).unwrap();
        assert_eq!(action, "show");
        assert!(options.contains("5s"), "{options}");
        assert_eq!(thread_name.as_deref(), Some("plugin-query-worker"));
        open.send(()).unwrap();
        let MonitorEvent::PluginValue {
            layer,
            target,
            binding: answered,
            result,
        } = events.recv_timeout(BOUND).unwrap()
        else {
            panic!("a query answer is a plugin value event");
        };
        assert_eq!(
            (layer.as_str(), target, answered),
            ("Base", ArtworkTarget::Keypad(3), binding("show"))
        );
        assert_eq!(result, answer("40%").map_err(|e| e.to_string()));
        assert!(worker.shutdown());
    }

    #[test]
    fn a_pending_duplicate_is_coalesced_but_another_control_or_binding_is_not() {
        let (results, events) = mpsc::channel();
        let (started, runs) = mpsc::channel();
        let (open, gate) = mpsc::channel();
        let (sender, mut worker) = spawn_queries(gated_query(started, gate), results).unwrap();
        sender.submit(request("Base", 1, "running")).unwrap();
        runs.recv_timeout(BOUND).unwrap();
        for _ in 0..5 {
            sender.submit(request("Base", 2, "show")).unwrap();
        }
        sender.submit(request("Base", 3, "show")).unwrap();
        sender.submit(request("Base", 2, "other")).unwrap();
        sender.submit(request("Studio", 2, "show")).unwrap();
        for _ in 0..5 {
            open.send(()).unwrap();
        }
        let mut answered = Vec::new();
        while answered.len() < 5 {
            let MonitorEvent::PluginValue {
                layer,
                target,
                binding,
                ..
            } = events.recv_timeout(BOUND).unwrap()
            else {
                panic!("only answers are expected");
            };
            answered.push((layer, target, binding.action_id));
        }
        assert_eq!(answered.len(), 5, "1 running + 4 distinct queued requests");
        assert!(
            events.recv_timeout(Duration::from_millis(300)).is_err(),
            "the four duplicates of one control ran once"
        );
        assert!(worker.shutdown());
    }

    #[test]
    fn query_admission_is_bounded_and_never_blocks_the_submitter_or_the_invoke_queue() {
        let (results, _events) = mpsc::channel();
        let (started, runs) = mpsc::channel();
        let (open, gate) = mpsc::channel();
        let (sender, mut worker) = spawn_queries(gated_query(started, gate), results).unwrap();
        sender.submit(request("Base", 0, "running")).unwrap();
        runs.recv_timeout(BOUND).unwrap();
        for key in 0..QUERY_CAPACITY {
            sender
                .submit(request("Base", u8::try_from(key).unwrap() + 1, "show"))
                .unwrap();
        }
        assert_eq!(
            sender.submit(request("Base", 200, "show")),
            Err(Rejected::Busy)
        );
        // A full query queue leaves the invoke queue untouched.
        let (invoke_results, _invoke_events) = mpsc::channel();
        let (invoke_sender, mut invoke_worker) =
            spawn(Box::new(|_, _| ok()), invoke_results).unwrap();
        assert!(invoke_sender.submit(&binding("press")).is_ok());
        assert!(invoke_worker.shutdown());
        drop(open);
        assert!(worker.shutdown());
    }

    #[test]
    fn shutdown_drops_queued_queries_and_refuses_new_ones() {
        let (results, _events) = mpsc::channel();
        let ran = Arc::new(Mutex::new(Vec::new()));
        let (started, runs) = mpsc::channel::<()>();
        let (open, gate) = mpsc::channel::<()>();
        let log = ran.clone();
        let query: Query = Box::new(move |binding, _| {
            log.lock().unwrap().push(binding.action_id.clone());
            started.send(()).unwrap();
            let _ = gate.recv_timeout(BOUND);
            answer("x")
        });
        let (sender, mut worker) = spawn_queries(query, results).unwrap();
        sender.submit(request("Base", 1, "running")).unwrap();
        runs.recv_timeout(BOUND).unwrap();
        sender.submit(request("Base", 2, "queued")).unwrap();
        open.send(()).unwrap();
        assert!(worker.shutdown());
        assert_eq!(*ran.lock().unwrap(), ["running"]);
        assert_eq!(
            sender.submit(request("Base", 3, "late")),
            Err(Rejected::Stopped)
        );
    }

    #[test]
    fn a_failed_query_is_reported_once_as_text_and_never_retried() {
        let (results, events) = mpsc::channel();
        let calls = Arc::new(Mutex::new(0u32));
        let counter = calls.clone();
        let query: Query = Box::new(move |_, _| {
            *counter.lock().unwrap() += 1;
            Err(PluginError::Disabled("demo".into()))
        });
        let (sender, mut worker) = spawn_queries(query, results).unwrap();
        sender.submit(request("Base", 1, "show")).unwrap();
        let MonitorEvent::PluginValue { result, .. } = events.recv_timeout(BOUND).unwrap() else {
            panic!("a failure still answers the control");
        };
        assert!(result.unwrap_err().contains("disabled"));
        assert!(worker.shutdown());
        assert_eq!(*calls.lock().unwrap(), 1);
    }

    #[test]
    fn the_real_query_worker_answers_a_missing_plugin_without_running_anything() {
        let root = std::env::temp_dir().join(format!("sdeck-query-worker-{}", std::process::id()));
        let (results, events) = mpsc::channel();
        let (sender, mut worker) = spawn_queries_for_root(root, results).unwrap();
        sender.submit(request("Base", 1, "show")).unwrap();
        let MonitorEvent::PluginValue { result, .. } = events.recv_timeout(BOUND).unwrap() else {
            panic!("a missing plugin answers with an error");
        };
        assert!(result.unwrap_err().contains("not installed"));
        assert!(worker.shutdown());
    }

    #[test]
    fn an_invoke_announces_which_plugin_finished_after_its_report_even_on_failure() {
        let (results, events) = mpsc::channel();
        let invoke: Invoke = Box::new(|binding, _| {
            if binding.action_id == "bad" {
                Err(PluginError::Disabled("demo".into()))
            } else {
                ok()
            }
        });
        let (sender, mut worker) = spawn(invoke, results).unwrap();
        for action in ["good", "bad"] {
            sender.submit(&binding(action)).unwrap();
            let first = events.recv_timeout(BOUND).unwrap();
            assert!(matches!(
                first,
                MonitorEvent::AppEvent { .. } | MonitorEvent::LayerError(_)
            ));
            assert_eq!(
                events.recv_timeout(BOUND).unwrap(),
                MonitorEvent::PluginFinished {
                    plugin_id: "demo".into()
                }
            );
        }
        assert!(worker.shutdown());
    }
}
