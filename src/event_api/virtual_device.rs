//! The virtual Stream Deck Classic: a device the hub serves exactly like a
//! plugged-in Classic, whose other end is a [`VirtualDeckLink`] (a web page,
//! through the application). Presses on the link become Classic key reports
//! that the hub reads; the images and brightness the hub writes come back as
//! [`VirtualFrame`]s, upright, as a person sees the keys.
//!
//! [`HidController::attach_virtual`](super::HidController::attach_virtual)
//! hands the [`VirtualDeck`] to the hub, which lists it at
//! [`VIRTUAL_DECK_PATH`] in every enumeration until
//! [`HidController::detach_virtual`](super::HidController::detach_virtual)
//! takes it away again, which is an unplug.

use super::{hub::Candidate, wire, Device, DeviceKey, StopSignal};
use crate::{
    device_model::{DeviceModel, ImageFormat, Rotation, VIRTUAL_CLASSIC},
    image_output,
    protocol::{protocol_for, FEATURE_REPORT_LEN},
};
use gtk::gdk_pixbuf::Pixbuf;
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
        Arc, Condvar, Mutex, MutexGuard,
    },
    time::Duration,
};

/// The path the hub lists the virtual deck under; its [`super::DeviceId`]
/// comes from it like a real device's.
pub const VIRTUAL_DECK_PATH: &str = "virtual:classic";

/// How long a read waits for a press, like a HID read timeout.
const READ_WAIT: Duration = Duration::from_millis(100);
/// Key reports waiting for the hub. Each one carries the state of every key,
/// so when the hub lags the oldest go first and the latest state survives.
const INPUT_BACKLOG: usize = 64;
/// Frames waiting for the consumer of [`VirtualDeckLink::frames`]. Newer
/// frames are dropped while it is this far behind; [`VirtualDeckLink::snapshot`]
/// always has the current state.
const FRAME_BACKLOG: usize = 256;
/// The largest picture the deck reassembles.
const IMAGE_LIMIT: usize = 4 << 20;
/// Why reads and writes fail once the deck is gone or the connection is stale.
const DETACHED: &str = "Virtual Deck detached";

/// What the virtual deck shows, for its viewers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VirtualFrame {
    /// The picture of one-based `key` (row-major from the top left), an upright JPEG.
    Key { key: u8, jpeg: Vec<u8> },
    /// The keypad background (the picture behind the keys), an upright JPEG.
    Background { jpeg: Vec<u8> },
    /// Display brightness in percent, 0..=100.
    Brightness(u8),
    /// The device was blanked: every picture shown so far is gone.
    Reset,
}

/// A new virtual Stream Deck Classic: the device end for the hub and the
/// viewer end for the application.
pub fn virtual_deck_pair() -> (VirtualDeck, VirtualDeckLink) {
    let link = Arc::new(Link::new(&VIRTUAL_CLASSIC));
    (VirtualDeck { link: link.clone() }, VirtualDeckLink { link })
}

/// The device end of a virtual deck; give it to
/// [`HidController::attach_virtual`](super::HidController::attach_virtual).
/// Dropping it (a detach, a replacement or the controller ending) unplugs the
/// deck for good.
pub struct VirtualDeck {
    link: Arc<Link>,
}

impl Drop for VirtualDeck {
    fn drop(&mut self) {
        self.link.unplug();
    }
}

/// The viewer end of a virtual deck. Clones share the same deck.
#[derive(Clone)]
pub struct VirtualDeckLink {
    link: Arc<Link>,
}

impl VirtualDeckLink {
    /// Press or release one-based `key` (1..=15); other numbers are ignored,
    /// and so is everything while the hub has the deck closed.
    pub fn press(&self, key: u8, pressed: bool) {
        self.link.press(key, pressed);
    }

    /// The frames from now on. There is one consumer: a later call takes the
    /// stream over and ends the receiver handed out before.
    pub fn frames(&self) -> Receiver<VirtualFrame> {
        self.link.frames()
    }

    /// The current state for a newly connected viewer: [`VirtualFrame::Reset`]
    /// first, then the brightness, the background and every key picture
    /// known. It includes every frame already sent to [`Self::frames`].
    pub fn snapshot(&self) -> Vec<VirtualFrame> {
        self.link.snapshot()
    }
}

/// The wire between the two ends.
struct Link {
    model: &'static DeviceModel,
    state: Mutex<LinkState>,
    /// Wakes a read waiting for a press or for the unplug.
    wake: Condvar,
}

struct LinkState {
    unplugged: bool,
    /// The number of the connection the hub has open now.
    open: Option<u64>,
    /// How many connections the hub has opened.
    opened: u64,
    /// Whether each key is held down, by zero-based key.
    keys: Vec<bool>,
    inputs: VecDeque<Vec<u8>>,
    shown: Shown,
    frames: Option<SyncSender<VirtualFrame>>,
}

/// The state the frames so far add up to.
#[derive(Default)]
struct Shown {
    brightness: Option<u8>,
    background: Option<Vec<u8>>,
    keys: BTreeMap<u8, Vec<u8>>,
}

impl Shown {
    fn apply(&mut self, frame: &VirtualFrame) {
        match frame {
            VirtualFrame::Key { key, jpeg } => {
                self.keys.insert(*key, jpeg.clone());
            }
            VirtualFrame::Background { jpeg } => self.background = Some(jpeg.clone()),
            VirtualFrame::Brightness(percent) => self.brightness = Some(*percent),
            VirtualFrame::Reset => *self = Self::default(),
        }
    }
}

impl LinkState {
    /// Record `frame` and pass it to the consumer, never waiting for it.
    fn publish(&mut self, frame: VirtualFrame) {
        self.shown.apply(&frame);
        if let Some(frames) = &self.frames {
            match frames.try_send(frame) {
                Ok(()) | Err(TrySendError::Full(_)) => {}
                Err(TrySendError::Disconnected(_)) => self.frames = None,
            }
        }
    }
}

impl Link {
    fn new(model: &'static DeviceModel) -> Self {
        Self {
            model,
            state: Mutex::new(LinkState {
                unplugged: false,
                open: None,
                opened: 0,
                keys: vec![false; usize::from(model.keys())],
                inputs: VecDeque::new(),
                shown: Shown::default(),
                frames: None,
            }),
            wake: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, LinkState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn press(&self, key: u8, pressed: bool) {
        if !self.model.is_valid_key(key) {
            return;
        }
        let mut state = self.lock();
        state.keys[usize::from(key - 1)] = pressed;
        // Like a device on no host: nobody reads what happens meanwhile.
        if state.open.is_none() {
            return;
        }
        let keys: Vec<u8> = state.keys.iter().map(|down| u8::from(*down)).collect();
        if state.inputs.len() >= INPUT_BACKLOG {
            state.inputs.pop_front();
        }
        state
            .inputs
            .push_back(wire::input_report(self.model, 0x00, &keys));
        drop(state);
        self.wake.notify_all();
    }

    fn frames(&self) -> Receiver<VirtualFrame> {
        let (sender, receiver) = sync_channel(FRAME_BACKLOG);
        self.lock().frames = Some(sender);
        receiver
    }

    fn snapshot(&self) -> Vec<VirtualFrame> {
        let state = self.lock();
        let shown = &state.shown;
        let mut frames = vec![VirtualFrame::Reset];
        frames.extend(shown.brightness.map(VirtualFrame::Brightness));
        frames.extend(
            shown
                .background
                .clone()
                .map(|jpeg| VirtualFrame::Background { jpeg }),
        );
        frames.extend(shown.keys.iter().map(|(key, jpeg)| VirtualFrame::Key {
            key: *key,
            jpeg: jpeg.clone(),
        }));
        frames
    }

    /// The hub opens the deck: a fresh connection starts blank.
    fn open(self: &Arc<Self>) -> Result<Connection, String> {
        let mut state = self.lock();
        if state.unplugged {
            return Err(format!("Device found but could not open: {DETACHED}"));
        }
        state.opened += 1;
        let number = state.opened;
        state.open = Some(number);
        state.inputs.clear();
        state.publish(VirtualFrame::Reset);
        Ok(Connection {
            link: self.clone(),
            number,
            images: RefCell::default(),
        })
    }

    /// The deck is gone for good: the open connection fails like a pulled cable.
    fn unplug(&self) {
        let mut state = self.lock();
        if state.unplugged {
            return;
        }
        state.unplugged = true;
        state.open = None;
        state.inputs.clear();
        state.publish(VirtualFrame::Reset);
        drop(state);
        self.wake.notify_all();
    }
}

/// One connection of the hub to the deck: the [`Device`] a worker drives.
pub(super) struct Connection {
    link: Arc<Link>,
    number: u64,
    /// Pictures being reassembled, by command and slot.
    images: RefCell<Assembly>,
}

impl Connection {
    fn current(&self, state: &LinkState) -> bool {
        state.open == Some(self.number)
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let mut state = self.link.lock();
        if self.current(&state) {
            state.open = None;
            state.inputs.clear();
        }
    }
}

impl Device for Connection {
    fn read(&self, buffer: &mut [u8]) -> Result<usize, String> {
        let state = self.link.lock();
        let mut state = self
            .link
            .wake
            .wait_timeout_while(state, READ_WAIT, |state| {
                self.current(state) && state.inputs.is_empty()
            })
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .0;
        if !self.current(&state) {
            return Err(DETACHED.into());
        }
        Ok(match state.inputs.pop_front() {
            Some(report) => {
                let size = report.len().min(buffer.len());
                buffer[..size].copy_from_slice(&report[..size]);
                size
            }
            None => 0,
        })
    }

    fn write(&self, report: &[u8]) -> Result<usize, String> {
        if !self.current(&self.link.lock()) {
            return Err(DETACHED.into());
        }
        let model = self.link.model;
        let finished = self.images.borrow_mut().add(model, report);
        // Pictures are turned upright outside the lock.
        if let Some((command, slot, picture)) = finished {
            match upright_frame(model, command, slot, picture) {
                Ok(frame) => {
                    let mut state = self.link.lock();
                    if self.current(&state) {
                        state.publish(frame);
                    }
                }
                Err(error) => eprintln!("Could not show a virtual deck picture: {error}"),
            }
        }
        Ok(report.len())
    }

    fn write_feature(&self, report: &[u8]) -> Result<usize, String> {
        let mut state = self.link.lock();
        if !self.current(&state) {
            return Err(DETACHED.into());
        }
        if let Some(percent) = brightness_in(self.link.model, report) {
            state.publish(VirtualFrame::Brightness(percent));
        }
        Ok(report.len())
    }
}

/// The level of a brightness feature report in the model family's layout.
fn brightness_in(model: &DeviceModel, report: &[u8]) -> Option<u8> {
    let family = protocol_for(model.family);
    let (dark, bright) = (family.brightness_report(0), family.brightness_report(100));
    let at = dark
        .iter()
        .zip(bright)
        .position(|(dark, bright)| *dark != bright)?;
    let level = *report.get(at)?;
    (report.len() == FEATURE_REPORT_LEN && family.brightness_report(level)[..] == *report)
        .then_some(level)
}

/// Image chunks collected per `(command, slot)`, with the next chunk index.
#[derive(Default)]
struct Assembly(HashMap<(u8, u8), (u16, Vec<u8>)>);

impl Assembly {
    /// Add one output report; a finished key image (`02 07`) or keypad
    /// background (`02 08`) comes back as `(command, slot, picture)`. Other
    /// reports, and out-of-order or oversized uploads, are dropped.
    fn add(&mut self, model: &DeviceModel, report: &[u8]) -> Option<(u8, u8, Vec<u8>)> {
        let chunk = wire::expanded_chunk(model, report)?;
        if !matches!(chunk.command, 0x07 | 0x08) {
            return None;
        }
        let at = (chunk.command, chunk.slot);
        let data = chunk
            .header
            .checked_add(chunk.length)
            .and_then(|end| report.get(chunk.header..end));
        if chunk.index == 0 {
            self.0.insert(at, (0, Vec::new()));
        }
        let (next, picture) = self.0.get_mut(&at)?;
        let Some(data) =
            data.filter(|data| *next == chunk.index && picture.len() + data.len() <= IMAGE_LIMIT)
        else {
            self.0.remove(&at);
            return None;
        };
        picture.extend_from_slice(data);
        *next = next.wrapping_add(1);
        if !chunk.last {
            return None;
        }
        let (_, picture) = self.0.remove(&at)?;
        Some((chunk.command, chunk.slot, picture))
    }
}

/// The frame a finished upload shows, turned back as a person sees it.
fn upright_frame(
    model: &DeviceModel,
    command: u8,
    slot: u8,
    picture: Vec<u8>,
) -> Result<VirtualFrame, String> {
    match command {
        0x07 => {
            let key = slot
                .checked_add(1)
                .filter(|key| model.is_valid_key(*key))
                .ok_or_else(|| format!("key index {slot} out of range"))?;
            let jpeg = upright(model, picture, model.key_rotation, model.key_flip_y)?;
            Ok(VirtualFrame::Key { key, jpeg })
        }
        _ => {
            let rotation = model
                .keypad_background
                .map_or(Rotation::None, |background| background.rotation);
            let jpeg = upright(model, picture, rotation, false)?;
            Ok(VirtualFrame::Background { jpeg })
        }
    }
}

/// `picture` as it was before the device's `rotation` (then flip) was
/// applied, as a JPEG.
fn upright(
    model: &DeviceModel,
    picture: Vec<u8>,
    rotation: Rotation,
    flipped: bool,
) -> Result<Vec<u8>, String> {
    if rotation == Rotation::None && !flipped && model.image_format == ImageFormat::Jpeg {
        return Ok(picture);
    }
    let image = Pixbuf::from_read(std::io::Cursor::new(picture))
        .map_err(|error| format!("could not decode the picture: {error}"))?;
    let image = if flipped {
        image
            .flip(false)
            .ok_or_else(|| "could not flip the picture".to_string())?
    } else {
        image
    };
    let undone = match rotation {
        Rotation::Cw90 => Rotation::Ccw90,
        Rotation::Ccw90 => Rotation::Cw90,
        other => other,
    };
    let bytes = image_output::rotated(&image, undone)?
        .save_to_bufferv("jpeg", &[("quality", "92")])
        .map_err(|error| error.to_string())?;
    if bytes.is_empty() {
        return Err("JPEG encoder returned an empty image".into());
    }
    Ok(bytes)
}

/// A device the hub serves: one its enumeration found, or the virtual deck.
pub(super) enum Served<D> {
    Found(D),
    Virtual(Connection),
}

impl<D: Device> Device for Served<D> {
    fn read(&self, buffer: &mut [u8]) -> Result<usize, String> {
        match self {
            Self::Found(device) => device.read(buffer),
            Self::Virtual(device) => device.read(buffer),
        }
    }

    fn write(&self, report: &[u8]) -> Result<usize, String> {
        match self {
            Self::Found(device) => device.write(report),
            Self::Virtual(device) => device.write(report),
        }
    }

    fn write_feature(&self, report: &[u8]) -> Result<usize, String> {
        match self {
            Self::Found(device) => device.write_feature(report),
            Self::Virtual(device) => device.write_feature(report),
        }
    }
}

/// Where the controller leaves the attached deck for the hub's enumerations.
/// Clones share it.
#[derive(Clone, Default)]
pub(super) struct VirtualSlot {
    deck: Arc<Mutex<Option<VirtualDeck>>>,
    /// Set by an attach so the hub lists again without waiting its interval.
    rescan: Arc<AtomicBool>,
}

impl VirtualSlot {
    fn lock(&self) -> MutexGuard<'_, Option<VirtualDeck>> {
        self.deck
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Serve `deck` from the next enumeration on; a deck attached before is
    /// unplugged.
    pub(super) fn attach(&self, deck: VirtualDeck) {
        let replaced = self.lock().replace(deck);
        drop(replaced);
        self.rescan.store(true, Ordering::Release);
    }

    /// Unplug the attached deck, if any.
    pub(super) fn detach(&self) {
        let detached = self.lock().take();
        drop(detached);
    }

    /// Whether an attach asked for an enumeration since the last call.
    pub(super) fn take_rescan(&self) -> bool {
        self.rescan.swap(false, Ordering::AcqRel)
    }

    /// `found` plus the attached deck. A failing listing does not hide the
    /// deck: the hub then serves it alone.
    pub(super) fn around<D, E>(
        &self,
        mut found: E,
    ) -> impl FnMut(&StopSignal) -> Result<Vec<Candidate<Served<D>>>, String>
    where
        D: 'static,
        E: FnMut(&StopSignal) -> Result<Vec<Candidate<D>>, String>,
    {
        let slot = self.clone();
        move |stop| {
            let listed = found(stop).map(|candidates| {
                candidates
                    .into_iter()
                    .map(|Candidate { key, open }| Candidate {
                        key,
                        open: Box::new(move || open().map(Served::Found)),
                    })
                    .collect::<Vec<_>>()
            });
            let Some(deck) = slot.candidate() else {
                return listed;
            };
            let mut listed = listed.unwrap_or_default();
            listed.push(deck);
            Ok(listed)
        }
    }

    fn candidate<D>(&self) -> Option<Candidate<Served<D>>> {
        let link = self.lock().as_ref()?.link.clone();
        Some(Candidate {
            key: DeviceKey {
                path: VIRTUAL_DECK_PATH.into(),
                serial: None,
                model: link.model,
            },
            open: Box::new(move || link.open().map(Served::Virtual)),
        })
    }
}

#[cfg(test)]
mod tests;
