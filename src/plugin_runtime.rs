//! A dedicated worker for plugin invocations, separate from the GTK thread and from the layer
//! worker, so a stalled plugin never delays built-in actions.
//!
//! Admission is bounded ([`QUEUE_CAPACITY`]); a full queue rejects instead of waiting. Each
//! request runs once, with the immutable binding taken at submission, under the host's
//! per-request deadline ([`REQUEST_TIMEOUT`]); nothing is retried. Results travel on the
//! existing monitor-event channel. Shutdown drops queued requests rather than draining them.
use sdeck_plus::{
    event_api::MonitorEvent,
    plugins::{InvokeOptions, InvokeOutcome, PluginBinding, PluginError, PluginRegistry},
};
use std::{
    io,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};

/// Requests waiting behind the one that is running.
pub(crate) const QUEUE_CAPACITY: usize = 8;
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
                if let Err(error) = results.send(event) {
                    eprintln!("Plugin result delivery failed: {error}");
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
}
