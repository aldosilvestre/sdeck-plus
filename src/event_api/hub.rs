//! The HID hub: one thread enumerates every supported device, one worker thread
//! serves each, and artwork and controls are addressed to a MODEL and fanned
//! out to every live device of it.
//!
//! Lock rule: [`Shared`] is locked only to route messages between channels. It
//! is never held across a HID call, a send that can block, or a join.

use super::{
    command_target,
    device_controls::Controls,
    device_worker::{DeviceWorker, Prepare},
    not_found_message, ArtworkCommand, ConnectionState, Device, DeviceAction, DeviceId, DeviceKey,
    HidController, HidHandle, HubClosed, MonitorEvent, StopSignal,
};
use crate::{device_control::DesiredState, device_model::DeviceModel, image_output};
use std::{
    collections::{HashMap, HashSet},
    marker::PhantomData,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, Receiver, RecvTimeoutError, Sender},
        Arc, Mutex, MutexGuard,
    },
    thread,
    time::{Duration, Instant},
};

/// How often the hub looks at the clock while it waits.
const SLICE: Duration = Duration::from_millis(50);
/// How often a router looks at stop while it waits for a command.
const ROUTE_POLL: Duration = Duration::from_millis(100);

/// How long the hub waits between enumerations.
#[derive(Clone, Copy)]
pub(super) struct Cadence {
    pub(super) search: Duration,
    pub(super) after_unplug: Duration,
}

impl Default for Cadence {
    fn default() -> Self {
        Self {
            search: Duration::from_secs(2),
            after_unplug: Duration::from_secs(1),
        }
    }
}

/// A supported device that was listed; opened only if the hub wants it.
pub(super) struct Candidate<D> {
    pub(super) key: DeviceKey,
    pub(super) open: Box<dyn FnOnce() -> Result<D, String> + Send>,
}

type Pid = u16;

struct Slot {
    id: DeviceId,
    model: &'static DeviceModel,
    artwork: Sender<ArtworkCommand>,
    controls: Sender<DeviceAction>,
}

#[derive(Default)]
struct State {
    /// Live workers in the order they attached; the first is the active one.
    workers: Vec<Slot>,
    /// Paths with a worker that has not finished with its device yet.
    occupied: HashSet<String>,
    /// Work addressed to a model that had no live device, in send order.
    pending: HashMap<Option<Pid>, Vec<ArtworkCommand>>,
    /// The latest complete picture of each model: its last snapshot plus the
    /// single newest command of every target since.
    snapshots: HashMap<Pid, Vec<ArtworkCommand>>,
    /// What each model was last asked for, for devices that attach later.
    desired: HashMap<Pid, DesiredState>,
    /// The model last attached on each path.
    last_model: HashMap<String, Pid>,
    last_active: Option<Pid>,
    served: bool,
    closed: bool,
}

impl State {
    fn refresh_active(&mut self) {
        if let Some(slot) = self.workers.first() {
            self.last_active = Some(slot.model.pid());
        }
    }
}

/// Work queued for a device that is not there, as a channel would keep it: a
/// snapshot supersedes everything before it.
fn hold(list: &mut Vec<ArtworkCommand>, command: ArtworkCommand) {
    if matches!(command, ArtworkCommand::Replace(_)) {
        list.clear();
    }
    list.push(command);
}

/// Keep the newest command per target so the cache stays as small as the
/// device it describes.
fn remember(list: &mut Vec<ArtworkCommand>, command: &ArtworkCommand) {
    let Some(target) = command_target(command) else {
        *list = vec![command.clone()];
        return;
    };
    for held in list.iter_mut() {
        if let ArtworkCommand::Replace(inner) = held {
            inner.retain(|old| command_target(old) != Some(target));
        }
    }
    list.retain(|old| command_target(old).is_none_or(|old| old != target));
    list.push(command.clone());
}

/// What a worker starts from.
pub(super) struct Attachment {
    pub(super) artwork: Receiver<ArtworkCommand>,
    pub(super) controls: Receiver<DeviceAction>,
    pub(super) desired: DesiredState,
}

pub(super) struct Shared {
    state: Mutex<State>,
    events: Sender<MonitorEvent>,
}

impl Shared {
    fn new(events: Sender<MonitorEvent>) -> Self {
        Self {
            state: Mutex::default(),
            events,
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap()
    }

    /// Send artwork to every live device of the model; with none, hold it for
    /// the next. `None` is "no model yet", for work sent before any device.
    pub(super) fn send_artwork(
        &self,
        model: Option<Pid>,
        command: ArtworkCommand,
    ) -> Result<(), HubClosed> {
        let mut guard = self.lock();
        let state = &mut *guard;
        if state.closed {
            return Err(HubClosed);
        }
        let mut delivered = false;
        if let Some(pid) = model {
            remember(state.snapshots.entry(pid).or_default(), &command);
            for slot in state.workers.iter().filter(|slot| slot.model.pid() == pid) {
                delivered |= slot.artwork.send(command.clone()).is_ok();
            }
        }
        if !delivered {
            hold(state.pending.entry(model).or_default(), command);
        }
        Ok(())
    }

    /// Send a control to every live device of the model. The model remembers
    /// it either way, and a model with no device says so.
    pub(super) fn send_control(&self, model: Pid, action: DeviceAction) -> Result<(), HubClosed> {
        let mut guard = self.lock();
        let state = &mut *guard;
        if state.closed {
            return Err(HubClosed);
        }
        let applied = state
            .desired
            .entry(model)
            .or_default()
            .apply(&action)
            .map_err(|error| error.to_string());
        let mut delivered = false;
        for slot in state
            .workers
            .iter()
            .filter(|slot| slot.model.pid() == model)
        {
            delivered |= slot.controls.send(action.clone()).is_ok();
        }
        drop(guard);
        if !delivered {
            let result = applied
                .and_then(|()| Err("HID device disconnected; control intent retained".to_owned()));
            let _ = self.events.send(MonitorEvent::DeviceResult {
                device: None,
                action,
                result,
            });
        }
        Ok(())
    }

    /// The model the one-session adapter addresses: the first connected
    /// device's, or the last one's while none is.
    fn active_model(&self) -> Option<Pid> {
        self.lock().last_active
    }

    fn live(&self) -> usize {
        self.lock().workers.len()
    }

    /// Reserve a path; false when a worker still has it.
    fn claim(&self, path: &str) -> bool {
        self.lock().occupied.insert(path.to_owned())
    }

    pub(super) fn release(&self, path: &str) {
        self.lock().occupied.remove(path);
    }

    /// Start routing to a new device. A device that is not a mirror of a live
    /// one gets what was held for its model; a mirror, or a path never seen
    /// before, gets the model's whole current picture.
    fn attach(&self, key: &DeviceKey) -> Attachment {
        let mut guard = self.lock();
        let state = &mut *guard;
        let pid = key.model.pid();
        let seen = state.last_model.insert(key.path.clone(), pid);
        if let Some(previous) = seen.filter(|previous| *previous != pid) {
            // The same port now has another model: what waited for the old one
            // means nothing to it.
            state.pending.remove(&Some(previous));
        }
        let mirrored = state.workers.iter().any(|slot| slot.model.pid() == pid);
        let unaddressed = if state.served {
            Vec::new()
        } else {
            state.pending.remove(&None).unwrap_or_default()
        };
        for command in &unaddressed {
            remember(state.snapshots.entry(pid).or_default(), command);
        }
        let held = state.pending.remove(&Some(pid)).unwrap_or_default();
        let replay = if mirrored || seen.is_none() {
            let mut all = state.snapshots.get(&pid).cloned().unwrap_or_default();
            all.extend(unaddressed);
            all
        } else {
            let mut all = unaddressed;
            all.extend(held);
            all
        };
        state.served = true;
        let (artwork, artwork_commands) = channel();
        let (controls, control_commands) = channel();
        for command in replay {
            let _ = artwork.send(command);
        }
        state.workers.push(Slot {
            id: key.id(),
            model: key.model,
            artwork,
            controls,
        });
        state.refresh_active();
        Attachment {
            artwork: artwork_commands,
            controls: control_commands,
            desired: state.desired.get(&pid).cloned().unwrap_or_default(),
        }
    }

    /// Stop routing to a worker; what it never got is held for the next device
    /// of its model, as it would have waited in a queue.
    pub(super) fn detach(&self, id: DeviceId, artwork: &Receiver<ArtworkCommand>) {
        let mut guard = self.lock();
        let state = &mut *guard;
        let Some(at) = state.workers.iter().position(|slot| slot.id == id) else {
            return;
        };
        let slot = state.workers.remove(at);
        let held = state.pending.entry(Some(slot.model.pid())).or_default();
        for command in artwork.try_iter() {
            hold(held, command);
        }
        state.refresh_active();
    }

    fn close(&self) {
        self.lock().closed = true;
    }
}

fn route_artwork(shared: &Shared, stop: &StopSignal, commands: Receiver<ArtworkCommand>) {
    while stop.live() {
        match commands.recv_timeout(ROUTE_POLL) {
            Ok(command) => {
                let model = shared.active_model();
                if shared.send_artwork(model, command).is_err() {
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn route_controls(shared: &Shared, stop: &StopSignal, commands: Receiver<DeviceAction>) {
    while stop.live() {
        match commands.recv_timeout(ROUTE_POLL) {
            Ok(action) => {
                // Controls have no "no model yet": there is nothing to remember.
                let Some(model) = shared.active_model() else {
                    continue;
                };
                if shared.send_control(model, action).is_err() {
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// What the hub last told its consumer while no device was open.
#[derive(PartialEq)]
enum Announced {
    Nothing,
    Searching,
    Unavailable(String),
    Connected,
}

struct Running {
    stop: StopSignal,
    thread: thread::JoinHandle<()>,
}

struct Hub<D, E> {
    enumerate: E,
    sender: Sender<MonitorEvent>,
    prepare: Prepare,
    cadence: Cadence,
    stop: StopSignal,
    shared: Arc<Shared>,
    ended: Arc<AtomicBool>,
    workers: Vec<Running>,
    announced: Announced,
    devices: PhantomData<fn() -> D>,
}

impl<D, E> Hub<D, E>
where
    D: Device + Send + 'static,
    E: FnMut(&StopSignal) -> Result<Vec<Candidate<D>>, String>,
{
    fn serve(&mut self) {
        while self.stop.live() {
            self.reap();
            if !self.say(MonitorEvent::Heartbeat) {
                return;
            }
            if self.shared.live() == 0
                && matches!(self.announced, Announced::Nothing | Announced::Connected)
            {
                self.announced = Announced::Searching;
                if !self.say(MonitorEvent::Connection(ConnectionState::Searching)) {
                    return;
                }
            }
            let failure = self.cycle();
            if !self.stop.live() {
                return;
            }
            if let Some(reason) = failure.filter(|_| self.shared.live() == 0) {
                let unchanged = self.announced == Announced::Unavailable(reason.clone());
                if !unchanged {
                    self.announced = Announced::Unavailable(reason.clone());
                    let state = ConnectionState::Unavailable(reason);
                    if !self.say(MonitorEvent::Connection(state)) {
                        return;
                    }
                }
            }
            if !self.pause() {
                return;
            }
        }
    }

    /// One enumeration: open every listed device that has no worker yet. A
    /// device that cannot be used never keeps the others from starting.
    fn cycle(&mut self) -> Option<String> {
        let mut candidates = match (self.enumerate)(&self.stop) {
            Ok(candidates) => candidates,
            Err(error) => return Some(error),
        };
        let mut failure = candidates.is_empty().then(not_found_message);
        // The path is the order, whatever order the OS listed them in.
        candidates.sort_by(|a, b| a.key.path.cmp(&b.key.path));
        for candidate in candidates {
            if !self.stop.live() {
                return None;
            }
            if !self.shared.claim(&candidate.key.path) {
                continue;
            }
            if let Err(error) = self.start(candidate) {
                failure.get_or_insert(error);
            }
        }
        failure
    }

    fn start(&mut self, candidate: Candidate<D>) -> Result<(), String> {
        let Candidate { key, open } = candidate;
        let release = |shared: &Shared| shared.release(&key.path);
        let device = match open() {
            Ok(device) => device,
            Err(error) => {
                release(&self.shared);
                return Err(error);
            }
        };
        // Never drive a family without a handler.
        if let Err(error) = key.model.ensure_protocol() {
            drop(device);
            release(&self.shared);
            return Err(error);
        }
        let (id, model) = (key.id(), key.model);
        if !self.stop.live() {
            drop(device);
            release(&self.shared);
            return Ok(());
        }
        self.announced = Announced::Connected;
        if !self.say(MonitorEvent::Connection(ConnectionState::Connected {
            device: id,
            model,
        })) {
            drop(device);
            release(&self.shared);
            return Ok(());
        }
        let attachment = self.shared.attach(&key);
        let stop = self.stop.child();
        let worker = DeviceWorker {
            id,
            path: key.path.clone(),
            model,
            device,
            stop: stop.clone(),
            hub_stop: self.stop.clone(),
            sender: self.sender.clone(),
            artwork: attachment.artwork,
            controls: Controls::for_device(id, attachment.controls, attachment.desired),
            prepare: self.prepare.clone(),
            shared: self.shared.clone(),
            ended: self.ended.clone(),
        };
        let spawned = thread::Builder::new()
            .name(format!("streamdeck-hid-{id}"))
            .spawn(move || worker.run());
        match spawned {
            Ok(thread) => {
                self.workers.push(Running { stop, thread });
                Ok(())
            }
            Err(error) => {
                // The worker never ran, so nothing else will free its slot.
                self.shared.detach(id, &channel().1);
                release(&self.shared);
                Err(format!("Could not start the HID worker: {error}"))
            }
        }
    }

    /// Join the workers that have finished.
    fn reap(&mut self) {
        let mut at = 0;
        while at < self.workers.len() {
            if self.workers[at].thread.is_finished() {
                let _ = self.workers.swap_remove(at).thread.join();
            } else {
                at += 1;
            }
        }
    }

    /// Tell the consumer; losing it stops everything.
    fn say(&self, event: MonitorEvent) -> bool {
        let delivered = self.sender.send(event).is_ok();
        if !delivered {
            self.stop.request();
        }
        delivered
    }

    /// Wait for the next enumeration: the search interval, or the shorter one
    /// once a device has gone. False when stopped.
    fn pause(&self) -> bool {
        let mut deadline = Instant::now() + self.cadence.search;
        loop {
            if self.ended.swap(false, Ordering::AcqRel) {
                deadline = deadline.min(Instant::now() + self.cadence.after_unplug);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            if !self.stop.wait(left.min(SLICE)) {
                return false;
            }
        }
    }

    /// Stop every worker, then wait for each to blank and release its device.
    fn finish(&mut self) {
        self.stop.request();
        self.shared.close();
        for worker in &self.workers {
            worker.stop.request();
        }
        for worker in self.workers.drain(..) {
            let _ = worker.thread.join();
        }
    }
}

/// Start the hub on its own thread. `enumerate` lists the supported devices
/// without opening them; `prepare` turns artwork into reports for a model.
pub(super) fn spawn<D, E, P>(
    sender: Sender<MonitorEvent>,
    enumerate: E,
    prepare: P,
    cadence: Cadence,
) -> HidController
where
    D: Device + Send + 'static,
    E: FnMut(&StopSignal) -> Result<Vec<Candidate<D>>, String> + Send + 'static,
    P: Fn(&DeviceModel, ArtworkCommand) -> Result<Vec<image_output::OutputReport>, String>
        + Send
        + Sync
        + 'static,
{
    let (artwork, artwork_commands) = channel();
    let (controls, control_commands) = channel();
    let stop = StopSignal::default();
    let shared = Arc::new(Shared::new(sender.clone()));
    let mut hub = Hub {
        enumerate,
        sender,
        prepare: Arc::new(prepare),
        cadence,
        stop: stop.clone(),
        shared: shared.clone(),
        ended: Arc::default(),
        workers: Vec::new(),
        announced: Announced::Nothing,
        devices: PhantomData,
    };
    let thread = thread::Builder::new()
        .name("streamdeck-hid".into())
        .spawn(move || {
            let shared = hub.shared.clone();
            let stop = hub.stop.clone();
            thread::scope(|scope| {
                scope.spawn(|| route_artwork(&shared, &stop, artwork_commands));
                scope.spawn(|| route_controls(&shared, &stop, control_commands));
                hub.serve();
                hub.finish();
            });
        })
        .expect("failed to start HID reader thread");
    HidController {
        stop,
        hub: Some(thread),
        handle: HidHandle {
            artwork,
            controls,
            shared: Some(shared),
        },
    }
}
