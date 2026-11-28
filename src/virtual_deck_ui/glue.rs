//! The owned thread between the hub's virtual deck and the web server: it
//! presses the keys the pages press, sends the deck's pictures to the pages
//! and passes the server's events on to GTK. It never touches a widget.
use sdeck_plus::{
    event_api::{VirtualDeckLink, VirtualFrame},
    virtual_deck::{DeckFrame, DeckInput, ServerEvent, VirtualDeckServer},
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError},
        Arc,
    },
    thread,
    time::Duration,
};

/// The longest a waiting picture or event stays queued; key presses never wait.
const TICK: Duration = Duration::from_millis(10);
/// More frames than this in one tick means the glue fell behind and the
/// deck's bounded queue may have dropped some (even a final Reset): the pages
/// then get one snapshot of the current state instead.
const LAG_BATCH: usize = 64;
/// At most this many stale frames are discarded after a lag; the snapshot
/// already holds what they showed.
const LAG_DRAIN: usize = 4096;

/// The deck's frame as the server sends it; the two enums match one to one.
pub(super) fn deck_frame(frame: VirtualFrame) -> DeckFrame {
    match frame {
        VirtualFrame::Key { key, jpeg } => DeckFrame::Key { key, jpeg },
        VirtualFrame::Background { jpeg } => DeckFrame::Background { jpeg },
        VirtualFrame::Brightness(percent) => DeckFrame::Brightness(percent),
        VirtualFrame::Reset => DeckFrame::Reset,
    }
}

/// What the glue reads.
pub(super) struct Wires {
    /// Keys the pages press, from the server.
    pub(super) inputs: Receiver<DeckInput>,
    /// Pictures and brightness, from the deck.
    pub(super) frames: Receiver<VirtualFrame>,
    /// Viewers and errors, from the server.
    pub(super) events: Receiver<ServerEvent>,
}

/// What the glue drives: the deck link and the server in the app.
pub(super) trait Ends {
    fn press(&self, input: DeckInput);
    fn snapshot(&self) -> Vec<VirtualFrame>;
    fn publish(&self, frame: DeckFrame);
    /// Hand a server event to GTK; never waits.
    fn event(&self, event: ServerEvent);
}

/// The real ends: the hub's deck, the running server and GTK's channel.
struct Live {
    link: VirtualDeckLink,
    server: Arc<VirtualDeckServer>,
    gtk: async_channel::Sender<ServerEvent>,
}

impl Ends for Live {
    fn press(&self, input: DeckInput) {
        self.link.press(input.key, input.pressed);
    }

    fn snapshot(&self) -> Vec<VirtualFrame> {
        self.link.snapshot()
    }

    fn publish(&self, frame: DeckFrame) {
        self.server.publish(frame);
    }

    fn event(&self, event: ServerEvent) {
        // Unbounded: this never blocks. A closed channel means GTK is gone.
        let _ = self.gtk.try_send(event);
    }
}

/// The running glue thread; stopping (or dropping) it joins the thread.
pub(super) struct Glue {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Glue {
    /// Start carrying presses from `inputs` to `link`, the link's frames to
    /// `server` and `events` to `gtk`. Takes over `link`'s frame stream.
    pub(super) fn start(
        link: VirtualDeckLink,
        inputs: Receiver<DeckInput>,
        server: Arc<VirtualDeckServer>,
        events: Receiver<ServerEvent>,
        gtk: async_channel::Sender<ServerEvent>,
    ) -> Result<Self, String> {
        let wires = Wires {
            inputs,
            frames: link.frames(),
            events,
        };
        let ends = Live { link, server, gtk };
        Self::spawn(wires, ends)
    }

    pub(super) fn spawn(wires: Wires, ends: impl Ends + Send + 'static) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = thread::Builder::new()
            .name("virtual-deck-glue".into())
            .spawn(move || run(&flag, &wires, &ends))
            .map_err(|error| format!("Could not start the Virtual Deck: {error}"))?;
        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }

    /// Stop the thread and wait for it (at most one tick).
    pub(super) fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            if handle.join().is_err() {
                eprintln!("The virtual deck glue thread panicked");
            }
        }
    }
}

impl Drop for Glue {
    fn drop(&mut self) {
        self.halt();
    }
}

/// Until `stop` is set or the server is gone: presses as they come, frames and
/// events at least every tick.
fn run(stop: &AtomicBool, wires: &Wires, ends: &impl Ends) {
    while !stop.load(Ordering::Acquire) {
        match wires.inputs.recv_timeout(TICK) {
            Ok(input) => {
                ends.press(input);
                for input in wires.inputs.try_iter() {
                    ends.press(input);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            // The server ended: no page can press or watch any more.
            Err(RecvTimeoutError::Disconnected) => break,
        }
        forward_frames(&wires.frames, ends);
        for event in wires.events.try_iter() {
            ends.event(event);
        }
    }
}

/// Publish what the deck showed since the last tick. After a Reset, or when
/// the glue fell behind, the pages get the current snapshot instead, which
/// also covers any frame the deck's full queue dropped.
fn forward_frames(frames: &Receiver<VirtualFrame>, ends: &impl Ends) {
    let mut batch = Vec::new();
    let mut lagging = false;
    while let Ok(frame) = frames.try_recv() {
        if batch.len() == LAG_BATCH {
            lagging = true;
            break;
        }
        batch.push(frame);
    }
    if lagging {
        frames.try_iter().take(LAG_DRAIN).for_each(drop);
    }
    if lagging || batch.contains(&VirtualFrame::Reset) {
        for frame in ends.snapshot() {
            ends.publish(deck_frame(frame));
        }
    } else {
        for frame in batch {
            ends.publish(deck_frame(frame));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        mpsc::{channel, Sender},
        Mutex,
    };

    /// Upper bound of every wait on a real signal.
    const WAIT: Duration = Duration::from_secs(10);

    #[derive(Debug, PartialEq)]
    enum Seen {
        Press(DeckInput),
        Publish(DeckFrame),
        Event(ServerEvent),
    }

    /// Records what the glue does and reports it on a channel to await.
    struct Recorder {
        seen: Mutex<Sender<Seen>>,
        snapshot: Vec<VirtualFrame>,
    }

    impl Ends for Recorder {
        fn press(&self, input: DeckInput) {
            let _ = self.seen.lock().unwrap().send(Seen::Press(input));
        }

        fn snapshot(&self) -> Vec<VirtualFrame> {
            self.snapshot.clone()
        }

        fn publish(&self, frame: DeckFrame) {
            let _ = self.seen.lock().unwrap().send(Seen::Publish(frame));
        }

        fn event(&self, event: ServerEvent) {
            let _ = self.seen.lock().unwrap().send(Seen::Event(event));
        }
    }

    struct Rig {
        inputs: Option<Sender<DeckInput>>,
        frames: Sender<VirtualFrame>,
        events: Sender<ServerEvent>,
        seen: Receiver<Seen>,
        glue: Option<Glue>,
    }

    impl Rig {
        fn start(snapshot: Vec<VirtualFrame>) -> Self {
            let (inputs, input_wire) = channel();
            let (frames, frame_wire) = channel();
            let (events, event_wire) = channel();
            let (sender, seen) = channel();
            let glue = Glue::spawn(
                Wires {
                    inputs: input_wire,
                    frames: frame_wire,
                    events: event_wire,
                },
                Recorder {
                    seen: Mutex::new(sender),
                    snapshot,
                },
            )
            .unwrap();
            Self {
                inputs: Some(inputs),
                frames,
                events,
                seen,
                glue: Some(glue),
            }
        }

        fn next(&self) -> Seen {
            self.seen.recv_timeout(WAIT).expect("the glue acts")
        }
    }

    fn key(key: u8) -> VirtualFrame {
        VirtualFrame::Key {
            key,
            jpeg: vec![key],
        }
    }

    fn published(key: u8) -> Seen {
        Seen::Publish(DeckFrame::Key {
            key,
            jpeg: vec![key],
        })
    }

    #[test]
    fn frames_map_one_to_one_onto_server_frames() {
        assert_eq!(
            deck_frame(key(3)),
            DeckFrame::Key {
                key: 3,
                jpeg: vec![3]
            }
        );
        assert_eq!(
            deck_frame(VirtualFrame::Background { jpeg: vec![7] }),
            DeckFrame::Background { jpeg: vec![7] }
        );
        assert_eq!(
            deck_frame(VirtualFrame::Brightness(40)),
            DeckFrame::Brightness(40)
        );
        assert_eq!(deck_frame(VirtualFrame::Reset), DeckFrame::Reset);
    }

    #[test]
    fn presses_frames_and_events_are_forwarded_in_order() {
        let rig = Rig::start(Vec::new());
        let input = DeckInput {
            key: 15,
            pressed: true,
        };
        rig.inputs.as_ref().unwrap().send(input).unwrap();
        assert_eq!(rig.next(), Seen::Press(input));
        rig.frames.send(key(1)).unwrap();
        rig.frames.send(VirtualFrame::Brightness(60)).unwrap();
        assert_eq!(rig.next(), published(1));
        assert_eq!(rig.next(), Seen::Publish(DeckFrame::Brightness(60)));
        rig.events.send(ServerEvent::ViewersChanged).unwrap();
        assert_eq!(rig.next(), Seen::Event(ServerEvent::ViewersChanged));
    }

    #[test]
    fn a_reset_sends_the_snapshot_instead_of_the_frames() {
        let rig = Rig::start(vec![VirtualFrame::Reset, key(2)]);
        rig.frames.send(key(9)).unwrap();
        assert_eq!(rig.next(), published(9));
        // A Reset brings the current snapshot, never the Reset frame alone.
        rig.frames.send(VirtualFrame::Reset).unwrap();
        assert_eq!(rig.next(), Seen::Publish(DeckFrame::Reset));
        assert_eq!(rig.next(), published(2));
    }

    #[test]
    fn falling_behind_sends_one_snapshot_instead_of_the_backlog() {
        // Three batches queued before the glue looks.
        let (frames, frame_wire) = channel();
        for _ in 0..(LAG_BATCH * 3) {
            frames.send(key(1)).unwrap();
        }
        let (sender, seen) = channel();
        let recorder = Recorder {
            seen: Mutex::new(sender),
            snapshot: vec![VirtualFrame::Reset, key(4)],
        };
        forward_frames(&frame_wire, &recorder);
        let published: Vec<Seen> = seen.try_iter().collect();
        assert_eq!(
            published,
            [Seen::Publish(DeckFrame::Reset), self::published(4)],
            "one snapshot, no backlog"
        );
        assert_eq!(frame_wire.try_iter().count(), 0, "the backlog is dropped");
    }

    #[test]
    fn stopping_joins_the_thread_and_a_closed_server_ends_it() {
        let mut rig = Rig::start(Vec::new());
        rig.glue.take().unwrap().stop();
        let mut rig2 = Rig::start(Vec::new());
        // The server's input sender goes away: the thread ends by itself.
        drop(rig2.inputs.take());
        let glue = rig2.glue.take().unwrap();
        let (done, finished) = channel();
        thread::spawn(move || {
            glue.stop();
            let _ = done.send(());
        });
        finished.recv_timeout(WAIT).expect("the glue ends");
        drop(rig);
    }
}
