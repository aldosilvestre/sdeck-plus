//! Device frames for scrolling Now playing titles. The HID owner thread asks
//! for due frames between input reads; nothing here blocks or writes stores.

use super::{command_target, ArtworkCommand};
use crate::{info_render, local_artwork::ArtworkTarget};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// At most about 10 frames per second for each scrolling title.
pub(super) const FRAME: Duration = Duration::from_millis(100);
/// A frame costs about 11-13 ms to draw and encode plus 5-6 HID reports, so
/// one pass between input reads sends at most this many (the stalest first)
/// to keep input latency within a few tens of milliseconds.
pub(super) const FRAMES_PER_PASS: usize = 2;

struct Running {
    command: ArtworkCommand,
    /// Title and column widths, measured once when the command arrived.
    extent: (f64, f64),
    started: Instant,
    /// Whole-pixel offset of the last frame sent.
    offset: f64,
    sent: Instant,
}

/// The targets whose last command shows a scrolling title.
#[derive(Default)]
pub(super) struct Marquees {
    running: HashMap<ArtworkTarget, Running>,
}

/// Where a command's title scrolls, when it does.
fn extent(command: &ArtworkCommand) -> Option<(f64, f64)> {
    match command {
        ArtworkCommand::ComposeInfo {
            target: ArtworkTarget::Keypad(_),
            value,
            ..
        } => info_render::marquee_extent(value, None),
        ArtworkCommand::ComposeInfo {
            target: ArtworkTarget::EncoderZone(_),
            value,
            ..
        } => info_render::marquee_extent(value, Some((false, false))),
        ArtworkCommand::ComposeTouchscreenZone {
            zone,
            value: Some(value),
            previous,
            next,
            ..
        } => {
            info_render::marquee_extent(value, Some((*zone == 1 && *previous, *zone == 4 && *next)))
        }
        _ => None,
    }
}

impl Marquees {
    /// Follow a command just sent to the device (as its phase-zero frame):
    /// it replaces whatever its target showed, and starts a marquee when its
    /// title scrolls. The whole strip covers every section.
    pub(super) fn observe(&mut self, command: &ArtworkCommand, now: Instant) {
        let Some(target) = command_target(command) else {
            return;
        };
        if target == ArtworkTarget::TouchStripBackground {
            self.running
                .retain(|shown, _| !matches!(shown, ArtworkTarget::EncoderZone(_)));
        }
        self.running.remove(&target);
        if let Some(extent) = extent(command) {
            self.running.insert(
                target,
                Running {
                    command: command.clone(),
                    extent,
                    started: now,
                    offset: 0.0,
                    sent: now,
                },
            );
        }
    }

    /// A new snapshot replaces every picture.
    pub(super) fn reset(&mut self) {
        self.running.clear();
    }

    pub(super) fn stop(&mut self, target: ArtworkTarget) {
        self.running.remove(&target);
    }

    pub(super) fn is_idle(&self) -> bool {
        self.running.is_empty()
    }

    /// Frames to send at `now`: only titles whose last frame is at least
    /// [`FRAME`] old and whose whole-pixel position moved, so the pause and
    /// unchanged frames are never resent; at most [`FRAMES_PER_PASS`].
    pub(super) fn due(&mut self, now: Instant) -> Vec<(ArtworkTarget, ArtworkCommand, Duration)> {
        let mut frames = Vec::new();
        let mut stale: Vec<_> = self
            .running
            .iter_mut()
            .filter(|(_, running)| now.saturating_duration_since(running.sent) >= FRAME)
            .collect();
        stale.sort_by_key(|(_, running)| running.sent);
        for (target, running) in stale {
            if frames.len() == FRAMES_PER_PASS {
                break;
            }
            let elapsed = now.saturating_duration_since(running.started);
            let (full, column) = running.extent;
            let offset = info_render::marquee_offset(full, column, elapsed).round();
            if offset == running.offset {
                continue;
            }
            running.offset = offset;
            running.sent = now;
            frames.push((*target, running.command.clone(), elapsed));
        }
        frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::info::{Glyph, InfoValue};

    fn title(glyph: Glyph, headline: &str) -> InfoValue {
        InfoValue {
            detail: "Queen".into(),
            inactive: glyph == Glyph::Pause,
            ..InfoValue::text(glyph, headline)
        }
    }

    fn info(target: ArtworkTarget, value: InfoValue) -> ArtworkCommand {
        ArtworkCommand::ComposeInfo {
            target,
            path: None,
            strip_path: None,
            value,
        }
    }

    const LONG: &str = "Bohemian Rhapsody (Remastered 2011)";

    #[test]
    fn ticker_runs_only_while_a_target_shows_a_long_playing_title() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let key = ArtworkTarget::Keypad(2);
        let mut marquees = Marquees::default();
        marquees.observe(&info(key, title(Glyph::Play, "So What")), start);
        assert!(marquees.is_idle(), "a title that fits does not tick");
        marquees.observe(&info(key, title(Glyph::Play, LONG)), start);
        assert!(!marquees.is_idle());
        // Still during the pause: the phase-zero frame is already on the key.
        assert!(marquees.due(at(50)).is_empty());
        assert!(marquees.due(at(1_000)).is_empty());
        // Scrolling: one frame, then nothing until the next frame is due.
        let frames = marquees.due(at(2_000));
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].0, key);
        assert_eq!(frames[0].2, Duration::from_millis(2_000));
        assert!(marquees.due(at(2_050)).is_empty(), "about 10 fps");
        assert_eq!(marquees.due(at(2_100)).len(), 1);
        assert!(
            marquees.due(at(2_100)).is_empty(),
            "never the same frame twice"
        );
        // Pausing or any other picture on the target stops it.
        marquees.observe(&info(key, title(Glyph::Pause, LONG)), at(2_200));
        assert!(marquees.is_idle());
        marquees.observe(&info(key, title(Glyph::Play, LONG)), at(2_300));
        marquees.observe(&ArtworkCommand::Clear { target: key }, at(2_400));
        assert!(marquees.is_idle());
    }

    #[test]
    fn sections_follow_their_indicators_and_the_strip_and_snapshots_stop_them() {
        let start = Instant::now();
        let mut marquees = Marquees::default();
        marquees.observe(
            &ArtworkCommand::ComposeTouchscreenZone {
                zone: 4,
                path: None,
                strip_path: None,
                value: Some(title(Glyph::Play, LONG)),
                previous: false,
                next: true,
            },
            start,
        );
        marquees.observe(
            &info(ArtworkTarget::EncoderZone(2), title(Glyph::Play, LONG)),
            start,
        );
        marquees.observe(
            &info(ArtworkTarget::Keypad(1), title(Glyph::Play, LONG)),
            start,
        );
        let later = start + Duration::from_secs(3);
        // Two frames per pass; the third title goes first in the next pass.
        let first = marquees.due(later);
        assert_eq!(first.len(), FRAMES_PER_PASS);
        let second = marquees.due(later + FRAME);
        let mut targets: Vec<_> = first
            .iter()
            .chain(second.iter().take(1))
            .map(|(target, ..)| format!("{target:?}"))
            .collect();
        targets.sort();
        assert_eq!(targets, ["EncoderZone(2)", "EncoderZone(4)", "Keypad(1)"]);
        // Redrawing the whole strip replaces every section, not the keys.
        marquees.observe(
            &ArtworkCommand::Clear {
                target: ArtworkTarget::TouchStripBackground,
            },
            later,
        );
        let frames = marquees.due(later + Duration::from_secs(1));
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].0, ArtworkTarget::Keypad(1));
        marquees.reset();
        assert!(marquees.is_idle());
    }

    #[test]
    fn hid_owner_sends_only_due_frames_and_stops_a_failing_marquee() {
        use super::super::{
            prepare_artwork, process_artwork_with, send_marquee_frames, MonitorEvent, StopSignal,
        };
        let (queue, commands) = std::sync::mpsc::channel();
        let key = ArtworkTarget::Keypad(3);
        queue.send(info(key, title(Glyph::Play, LONG))).unwrap();
        let (events, received) = std::sync::mpsc::channel();
        let stop = StopSignal::default();
        let mut marquees = Marquees::default();
        let mut written = 0;
        process_artwork_with(
            &commands,
            &stop,
            &events,
            prepare_artwork,
            |report| {
                written += 1;
                Ok(report.len())
            },
            &mut marquees,
        );
        assert!(written > 0, "the phase-zero frame is sent as usual");
        assert!(!marquees.is_idle());
        let later = Instant::now() + Duration::from_secs(3);
        let mut frames = Vec::new();
        let mut send = |marquees: &mut Marquees, at| {
            send_marquee_frames(
                marquees,
                at,
                &stop,
                &events,
                |command, elapsed| {
                    frames.push(elapsed);
                    super::super::prepare_artwork_at(command, elapsed)
                },
                |report| Ok(report.len()),
            );
        };
        send(&mut marquees, later);
        send(&mut marquees, later);
        assert_eq!(frames.len(), 1, "an unchanged frame is never resent");
        // Frames report nothing back while they succeed.
        assert!(received
            .try_iter()
            .all(|event| !matches!(event, MonitorEvent::ArtworkResult { result: Err(_), .. })));
        // A failing write reports once and stops the marquee.
        send_marquee_frames(
            &mut marquees,
            later + Duration::from_secs(1),
            &stop,
            &events,
            super::super::prepare_artwork_at,
            |_| Err("unplugged".into()),
        );
        assert!(marquees.is_idle());
        assert!(matches!(
            received.try_recv(),
            Ok(MonitorEvent::ArtworkResult { result: Err(_), .. })
        ));
    }
}
