use crate::layers::{Action, LayerStore, SwipeDirection, Trigger};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorError(pub String);

impl From<&str> for EditorError {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    Button,
    Encoder,
    Touch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    ButtonPress,
    EncoderPress,
    EncoderClockwise,
    EncoderCounterClockwise,
    TouchTap,
    TouchPress,
    TouchFlick,
    TouchSwipeLeft,
    TouchSwipeRight,
    TouchSwipeUp,
    TouchSwipeDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    None,
    MoveToLayer,
    RunCommand,
    OpenApplication,
    Shortcut,
    EmitLegacy,
    Extended,
}

pub fn events_for_control(control: ControlKind) -> Vec<EventKind> {
    match control {
        ControlKind::Button => vec![EventKind::ButtonPress],
        ControlKind::Encoder => vec![
            EventKind::EncoderPress,
            EventKind::EncoderClockwise,
            EventKind::EncoderCounterClockwise,
        ],
        ControlKind::Touch => vec![
            EventKind::TouchTap,
            EventKind::TouchPress,
            EventKind::TouchFlick,
            EventKind::TouchSwipeLeft,
            EventKind::TouchSwipeRight,
            EventKind::TouchSwipeUp,
            EventKind::TouchSwipeDown,
        ],
    }
}

pub fn trigger_from_selection(
    control: ControlKind,
    control_number: u8,
    event: EventKind,
) -> Result<Trigger, EditorError> {
    if !events_for_control(control).contains(&event) {
        return Err("event does not match selected control".into());
    }
    match (control, event) {
        (ControlKind::Button, EventKind::ButtonPress) if (1..=8).contains(&control_number) => {
            Ok(Trigger::KeyPress(control_number))
        }
        (ControlKind::Encoder, EventKind::EncoderPress) if (1..=4).contains(&control_number) => {
            Ok(Trigger::EncoderPress(control_number))
        }
        (ControlKind::Encoder, EventKind::EncoderClockwise)
            if (1..=4).contains(&control_number) =>
        {
            Ok(Trigger::EncoderClockwise(control_number))
        }
        (ControlKind::Encoder, EventKind::EncoderCounterClockwise)
            if (1..=4).contains(&control_number) =>
        {
            Ok(Trigger::EncoderCounterClockwise(control_number))
        }
        // Number 0 is the whole strip; 1 through 4 are its sections.
        (ControlKind::Touch, EventKind::TouchTap) if control_number <= 4 => {
            Ok(match control_number {
                0 => Trigger::TouchTap,
                zone => Trigger::ZoneTap(zone),
            })
        }
        (ControlKind::Touch, EventKind::TouchPress) if control_number <= 4 => {
            Ok(match control_number {
                0 => Trigger::TouchPress,
                zone => Trigger::ZonePress(zone),
            })
        }
        (ControlKind::Touch, EventKind::TouchFlick) => Ok(Trigger::TouchFlick),
        (ControlKind::Touch, EventKind::TouchSwipeLeft) => {
            Ok(Trigger::TouchSwipe(SwipeDirection::Left))
        }
        (ControlKind::Touch, EventKind::TouchSwipeRight) => {
            Ok(Trigger::TouchSwipe(SwipeDirection::Right))
        }
        (ControlKind::Touch, EventKind::TouchSwipeUp) => {
            Ok(Trigger::TouchSwipe(SwipeDirection::Up))
        }
        (ControlKind::Touch, EventKind::TouchSwipeDown) => {
            Ok(Trigger::TouchSwipe(SwipeDirection::Down))
        }
        _ => Err("control number is out of range".into()),
    }
}

pub fn format_application_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.is_empty() {
                r"\empty".to_string()
            } else {
                arg.chars()
                    .map(|ch| match ch {
                        '\\' => r"\\".to_string(),
                        '\n' => r"\n".to_string(),
                        '\r' => r"\r".to_string(),
                        '\t' => r"\t".to_string(),
                        other => other.to_string(),
                    })
                    .collect()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_application_args(text: &str) -> Result<Vec<String>, EditorError> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    text.split('\n')
        .map(|line| {
            if line.is_empty() {
                return Err(EditorError(
                    r"empty arguments must be written as \empty".into(),
                ));
            }
            if line == r"\empty" {
                return Ok(String::new());
            }
            let mut decoded = String::new();
            let mut chars = line.chars();
            while let Some(ch) = chars.next() {
                if ch != '\\' {
                    decoded.push(ch);
                    continue;
                }
                match chars.next() {
                    Some('\\') => decoded.push('\\'),
                    Some('n') => decoded.push('\n'),
                    Some('r') => decoded.push('\r'),
                    Some('t') => decoded.push('\t'),
                    Some(other) => {
                        return Err(EditorError(format!("unsupported escape \\{other}")));
                    }
                    None => return Err(EditorError("trailing escape in argument".into())),
                }
            }
            Ok(decoded)
        })
        .collect()
}

pub fn action_from_fields(
    store: &LayerStore,
    kind: ActionKind,
    layer_destination: &str,
    application_executable: &str,
    application_args: &str,
    text_value: &str,
    shortcut_modifiers: &[String],
) -> Result<Action, EditorError> {
    let text_value = text_value.trim();
    let action = match kind {
        ActionKind::None => Action::None,
        ActionKind::MoveToLayer => {
            let target = layer_destination.trim();
            if !store.layers().contains_key(target) {
                return Err("destination profile or page does not exist".into());
            }
            Action::SwitchLayer(target.into())
        }
        ActionKind::RunCommand => Action::Command(text_value.into()),
        ActionKind::OpenApplication => Action::Launch {
            executable: PathBuf::from(application_executable.trim()),
            args: parse_application_args(application_args)?,
        },
        ActionKind::Shortcut => Action::Shortcut {
            modifiers: shortcut_modifiers.to_vec(),
            key: text_value.into(),
        },
        ActionKind::EmitLegacy => Action::Emit(text_value.into()),
        ActionKind::Extended => {
            return Err("configure the selected action in its category fields".into())
        }
    };
    crate::layers::normalize_keyboard_action(action).map_err(|error| EditorError(error.to_string()))
}

/// Display every editable event, plus legacy exact rotations that can override it.
/// Summaries are presentation only: lookup and execution semantics stay in LayerStore.
pub fn control_summary(
    store: &LayerStore,
    layer: &str,
    control: ControlKind,
    number: u8,
) -> String {
    let sample = match control {
        ControlKind::Touch => Trigger::TouchTap,
        ControlKind::Encoder => Trigger::EncoderPress(number),
        ControlKind::Button => Trigger::KeyPress(number),
    };
    let bindings = store
        .binding_layer(layer, &sample)
        .map(|layer| &layer.bindings);
    let mut lines: Vec<String> = events_for_control(control)
        .into_iter()
        .map(|event| {
            let action = trigger_from_selection(control, number, event)
                .ok()
                .and_then(|trigger| bindings.and_then(|bindings| bindings.get(&trigger)));
            format!("{}: {}", summary_event_label(event), action_summary(action))
        })
        .collect();
    if control == ControlKind::Touch {
        if let Some(bindings) = bindings {
            for zone in 1..=4 {
                for (trigger, label) in [
                    (Trigger::ZoneTap(zone), "Tap"),
                    (Trigger::ZonePress(zone), "Hold"),
                ] {
                    if let Some(action) = bindings.get(&trigger) {
                        lines.push(format!(
                            "Section {zone} {label}: {}",
                            action_summary(Some(action))
                        ));
                    }
                }
            }
        }
    }
    if control == ControlKind::Encoder {
        if let Some(bindings) = bindings {
            for (trigger, action) in bindings {
                if let Trigger::Rotation(index, ticks) = trigger {
                    if *index == number {
                        lines.push(format!(
                            "Exact {ticks:+} ticks: {}",
                            action_summary(Some(action))
                        ));
                    }
                }
            }
        }
    }
    lines.join("\n")
}

/// The one action a dial preview names: turning first (clockwise, then
/// counterclockwise, then exact-tick overrides), else pressing. `None` when the
/// dial has no binding on the layer's current touchscreen page.
pub fn dial_legend(store: &LayerStore, layer: &str, number: u8) -> Option<String> {
    dial_action(store, layer, number).map(|action| action_summary(Some(&action)))
}

/// The action a dial's caption describes: turn, then exact ticks, then press.
pub fn dial_action(store: &LayerStore, layer: &str, number: u8) -> Option<Action> {
    let bindings = &store
        .binding_layer(layer, &Trigger::EncoderPress(number))?
        .bindings;
    [
        Trigger::EncoderClockwise(number),
        Trigger::EncoderCounterClockwise(number),
    ]
    .iter()
    .find_map(|trigger| bindings.get(trigger))
    .or_else(|| {
        bindings.iter().find_map(|(trigger, action)| {
            matches!(trigger, Trigger::Rotation(index, _) if *index == number).then_some(action)
        })
    })
    .or_else(|| bindings.get(&Trigger::EncoderPress(number)))
    .cloned()
}

/// The short caption a key or touch section without artwork shows under its
/// binding icon: the same wording as the editor's summaries.
pub fn action_caption(action: &Action) -> String {
    action_summary(Some(action))
}

/// A whole-strip touch gesture shown as a badge above the preview strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripGesture {
    SwipeLeft,
    SwipeRight,
    Tap,
    Hold,
}

impl StripGesture {
    pub const ALL: [Self; 4] = [Self::SwipeLeft, Self::SwipeRight, Self::Tap, Self::Hold];

    pub fn label(self) -> &'static str {
        match self {
            Self::SwipeLeft => "Swipe left",
            Self::SwipeRight => "Swipe right",
            Self::Tap => "Tap",
            Self::Hold => "Hold",
        }
    }

    /// The gesture's own glyph, drawn before its action's icon.
    pub fn icon(self) -> crate::binding_icons::BindingIcon {
        use crate::binding_icons::BindingIcon;
        match self {
            Self::SwipeLeft => BindingIcon::SwipeLeft,
            Self::SwipeRight => BindingIcon::SwipeRight,
            Self::Tap => BindingIcon::Tap,
            Self::Hold => BindingIcon::Hold,
        }
    }

    fn trigger(self) -> Trigger {
        use crate::layers::SwipeDirection;
        match self {
            Self::SwipeLeft => Trigger::TouchSwipe(SwipeDirection::Left),
            Self::SwipeRight => Trigger::TouchSwipe(SwipeDirection::Right),
            Self::Tap => Trigger::TouchTap,
            Self::Hold => Trigger::TouchPress,
        }
    }

    /// What an unbound gesture does: horizontal swipes change touch page.
    fn default_action(self) -> Option<Action> {
        use crate::features::{FeatureAction, TouchscreenAction};
        let page = match self {
            Self::SwipeLeft => TouchscreenAction::NextPage,
            Self::SwipeRight => TouchscreenAction::PreviousPage,
            Self::Tap | Self::Hold => return None,
        };
        Some(Action::Feature(FeatureAction::Touchscreen(page)))
    }
}

/// One strip gesture that does something on the current touch page.
#[derive(Clone, Debug, PartialEq)]
pub struct GestureBinding {
    pub gesture: StripGesture,
    pub icon: crate::binding_icons::BindingIcon,
    pub summary: String,
    /// The built-in swipe paging of an unbound swipe.
    pub default: bool,
}

impl GestureBinding {
    pub fn description(&self) -> String {
        let default = if self.default { " (default)" } else { "" };
        format!("{}{default}: {}", self.gesture.label(), self.summary)
    }
}

/// Swipe left/right, tap and hold of the whole strip on `layer`'s touch page,
/// in that order. Unbound swipes report their default paging; explicit
/// `None` bindings and unassigned taps or holds are omitted.
pub fn strip_gestures(store: &LayerStore, layer: &str) -> Vec<GestureBinding> {
    use crate::binding_icons::BindingIcon;
    StripGesture::ALL
        .into_iter()
        .filter_map(|gesture| {
            let bound = store.binding(layer, &gesture.trigger());
            let default = bound.is_none();
            let action = bound.or_else(|| gesture.default_action())?;
            let icon = BindingIcon::for_action(&action);
            icon.shown_on_controls().then(|| GestureBinding {
                gesture,
                icon,
                summary: action_summary(Some(&action)),
                default,
            })
        })
        .collect()
}

fn summary_event_label(event: EventKind) -> &'static str {
    match event {
        EventKind::ButtonPress | EventKind::EncoderPress => "Press",
        EventKind::EncoderClockwise => "CW",
        EventKind::EncoderCounterClockwise => "CCW",
        EventKind::TouchTap => "Tap",
        EventKind::TouchPress => "Hold",
        EventKind::TouchFlick => "Flick",
        EventKind::TouchSwipeLeft => "Swipe left",
        EventKind::TouchSwipeRight => "Swipe right",
        EventKind::TouchSwipeUp => "Swipe up",
        EventKind::TouchSwipeDown => "Swipe down",
    }
}

fn action_summary(action: Option<&Action>) -> String {
    let text = match action {
        None => return "Unassigned".into(),
        Some(Action::None) => "None".into(),
        Some(Action::SwitchLayer(target)) => format!("Go to {target}"),
        Some(Action::Command(command)) => format!("Run {command}"),
        Some(Action::KeyPress { key }) => format!("Shortcut {key}"),
        Some(Action::Launch { executable, .. }) => format!("Open {}", executable.display()),
        Some(Action::Shortcut { modifiers, key }) => {
            let mut keys = modifiers.clone();
            keys.push(key.clone());
            format!("Shortcut {}", keys.join("+"))
        }
        Some(Action::Emit(name)) => format!("Emit {name}"),
        Some(Action::Feature(action)) => action.summary(),
    };
    // Normalize whitespace and truncate by characters, never UTF-8 byte offsets.
    let single_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = single_line.chars();
    let mut concise: String = chars.by_ref().take(47).collect();
    if chars.next().is_some() {
        concise.push('…');
    }
    concise
}

pub fn existing_action(store: &LayerStore, layer: &str, trigger: &Trigger) -> Option<Action> {
    store.binding(layer, trigger)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layers::{Action, LayerStore, Trigger};
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEMP_ID: AtomicUsize = AtomicUsize::new(0);

    fn temp_store() -> (std::path::PathBuf, LayerStore) {
        let path = std::env::temp_dir().join(format!(
            "streamdeck-editor-red-{}-{}",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        (path.clone(), LayerStore::open_at(path).unwrap())
    }

    #[test]
    fn summaries_cover_actions_events_and_legacy_rotation_without_inheritance() {
        let (path, mut store) = temp_store();
        store.create("Studio").unwrap();
        store
            .bind(
                "Base",
                Trigger::KeyPress(8),
                Action::Command("echo\nhello".into()),
            )
            .unwrap();
        assert_eq!(
            control_summary(&store, "Base", ControlKind::Button, 8),
            "Press: Run echo hello"
        );
        assert_eq!(
            control_summary(&store, "Studio", ControlKind::Button, 8),
            "Press: Unassigned"
        );
        store
            .bind(
                "Base",
                Trigger::EncoderPress(4),
                Action::KeyPress {
                    key: "Return".into(),
                },
            )
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::EncoderClockwise(4),
                Action::SwitchLayer("Studio".into()),
            )
            .unwrap();
        store
            .bind("Base", Trigger::Rotation(4, -2), Action::Emit("old".into()))
            .unwrap();
        assert_eq!(
            control_summary(&store, "Base", ControlKind::Encoder, 4),
            "Press: Shortcut ENTER\nCW: Go to Studio\nCCW: Unassigned\nExact -2 ticks: Emit old"
        );
        store
            .bind(
                "Base",
                Trigger::TouchTap,
                Action::Launch {
                    executable: "/usr/bin/firefox".into(),
                    args: vec!["url".into()],
                },
            )
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::TouchSwipe(SwipeDirection::Left),
                Action::Shortcut {
                    modifiers: vec!["CTRL".into()],
                    key: "Tab".into(),
                },
            )
            .unwrap();
        assert_eq!(control_summary(&store, "Base", ControlKind::Touch, 0),
            "Tap: Open /usr/bin/firefox\nHold: Unassigned\nFlick: Unassigned\nSwipe left: Shortcut CTRL+TAB\nSwipe right: Unassigned\nSwipe up: Unassigned\nSwipe down: Unassigned");
        store
            .bind("Base", Trigger::ZoneTap(2), Action::Command("run".into()))
            .unwrap();
        assert!(control_summary(&store, "Base", ControlKind::Touch, 0)
            .ends_with("Swipe down: Unassigned\nSection 2 Tap: Run run"));
        assert_eq!(
            control_summary(&store, "Missing", ControlKind::Button, 1),
            "Press: Unassigned"
        );
        assert_eq!(
            control_summary(&store, "Base", ControlKind::Button, 9),
            "Press: Unassigned"
        );
        store
            .bind(
                "Base",
                Trigger::KeyPress(1),
                Action::Command("é".repeat(80)),
            )
            .unwrap();
        let summary = control_summary(&store, "Base", ControlKind::Button, 1);
        assert!(summary.ends_with('…'));
        assert!(summary.chars().count() <= 55);
        let button_summary = summary;
        let organization = crate::organization::Organization::open_at(&path, &store).unwrap();
        let mut artwork =
            crate::local_artwork::ArtworkStore::open_at_for_layer(&path, Some("Base")).unwrap();
        store
            .initialize_touchscreens(&organization, &mut artwork)
            .unwrap();
        let first = store.touchscreen_owner("Base").unwrap().to_owned();
        store
            .bind(
                "Base",
                Trigger::EncoderPress(4),
                Action::Emit("page-owned".into()),
            )
            .unwrap();
        assert_eq!(
            control_summary(&store, "Base", ControlKind::Encoder, 4),
            "Press: Emit page-owned\nCW: Go to Studio\nCCW: Unassigned\nExact -2 ticks: Emit old"
        );
        store.create_touchscreen_page("Base").unwrap();
        assert_eq!(
            control_summary(&store, "Base", ControlKind::Encoder, 4),
            "Press: Unassigned\nCW: Unassigned\nCCW: Unassigned"
        );
        assert_eq!(
            control_summary(&store, "Base", ControlKind::Button, 1),
            button_summary
        );
        store.select_touchscreen_page("Base", &first);
        assert!(control_summary(&store, "Base", ControlKind::Encoder, 4).contains("page-owned"));
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn physical_shortcuts_normalize_save_and_reload() {
        let (path, mut store) = temp_store();
        for (text, boxes, expected_mods, key) in [
            ("return", vec![], vec![], "ENTER"),
            ("d", vec![], vec![], "D"),
            ("CTRL + D", vec![], vec!["CTRL"], "D"),
            ("SUPER + RETURN", vec![], vec!["SUPER"], "ENTER"),
            (
                "win + Enter",
                vec!["CTRL", "SUPER", "CTRL"],
                vec!["CTRL", "SUPER"],
                "ENTER",
            ),
            ("d", vec!["SHIFT", "CTRL"], vec!["CTRL", "SHIFT"], "D"),
            ("meta + return", vec!["Windows"], vec!["SUPER"], "ENTER"),
            ("XF86AudioLowerVolume", vec![], vec![], "VOLUMEDOWN"),
            ("XF86AudioRaiseVolume", vec![], vec![], "VOLUMEUP"),
            ("XF86AudioMute", vec![], vec![], "MUTE"),
            ("mute_mic", vec![], vec![], "MICMUTE"),
            ("XF86AudioMicMute", vec![], vec![], "MICMUTE"),
            ("XF86AudioPlay", vec![], vec![], "PLAY"),
            ("XF86AudioPause", vec![], vec![], "PAUSECD"),
            ("play_pause", vec![], vec![], "PLAYPAUSE"),
            ("XF86AudioNext", vec![], vec![], "NEXTSONG"),
            ("previous", vec![], vec![], "PREVIOUSSONG"),
            ("XF86AudioStop", vec![], vec![], "STOPCD"),
            ("XF86AudioForward", vec![], vec![], "FASTFORWARD"),
            ("XF86AudioRewind", vec![], vec![], "REWIND"),
            ("XF86AudioRecord", vec![], vec![], "RECORD"),
            ("XF86Eject", vec![], vec![], "EJECTCD"),
            ("XF86MonBrightnessUp", vec![], vec![], "BRIGHTNESSUP"),
            ("XF86MonBrightnessDown", vec![], vec![], "BRIGHTNESSDOWN"),
            ("XF86KbdBrightnessUp", vec![], vec![], "KBDILLUMUP"),
            ("XF86KbdBrightnessDown", vec![], vec![], "KBDILLUMDOWN"),
            ("XF86KbdLightOnOff", vec![], vec![], "KBDILLUMTOGGLE"),
            (
                "CTRL + XF86AudioMicMute",
                vec!["CTRL"],
                vec!["CTRL"],
                "MICMUTE",
            ),
        ] {
            let boxes = boxes.into_iter().map(String::from).collect::<Vec<_>>();
            let action =
                action_from_fields(&store, ActionKind::Shortcut, "", "", "", text, &boxes).unwrap();
            assert_eq!(
                action,
                Action::Shortcut {
                    modifiers: expected_mods.into_iter().map(String::from).collect(),
                    key: key.into(),
                }
            );
            store
                .bind("Base", Trigger::KeyPress(1), action.clone())
                .unwrap();
            store.persist().unwrap();
            let loaded = LayerStore::open_at(&path).unwrap();
            assert_eq!(
                existing_action(&loaded, "Base", &Trigger::KeyPress(1)),
                Some(action)
            );
        }
        for text in [
            "",
            "CTRL",
            "CTRL +",
            "+ D",
            "A + B",
            "unknown-key",
            "D++CTRL",
        ] {
            assert!(
                action_from_fields(&store, ActionKind::Shortcut, "", "", "", text, &[]).is_err(),
                "{text}"
            );
        }
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn options_are_limited_by_control_kind() {
        assert_eq!(
            events_for_control(ControlKind::Button),
            vec![EventKind::ButtonPress]
        );
        assert_eq!(
            events_for_control(ControlKind::Encoder),
            vec![
                EventKind::EncoderPress,
                EventKind::EncoderClockwise,
                EventKind::EncoderCounterClockwise,
            ]
        );
        assert!(events_for_control(ControlKind::Touch).contains(&EventKind::TouchSwipeLeft));
    }

    #[test]
    fn typed_trigger_and_action_creation_validates_indices_and_layers() {
        let (path, mut store) = temp_store();
        store.create("Studio").unwrap();
        assert_eq!(
            trigger_from_selection(ControlKind::Button, 1, EventKind::ButtonPress).unwrap(),
            Trigger::KeyPress(1)
        );
        assert!(trigger_from_selection(ControlKind::Button, 9, EventKind::ButtonPress).is_err());
        assert!(trigger_from_selection(ControlKind::Button, 1, EventKind::EncoderPress).is_err());
        assert_eq!(
            action_from_fields(&store, ActionKind::MoveToLayer, "Studio", "", "", "", &[]).unwrap(),
            Action::SwitchLayer("Studio".into())
        );
        assert!(
            action_from_fields(&store, ActionKind::MoveToLayer, "Missing", "", "", "", &[])
                .is_err()
        );
        assert_eq!(
            action_from_fields(
                &store,
                ActionKind::OpenApplication,
                "",
                "/bin/echo",
                "one\ntwo",
                "",
                &[]
            )
            .unwrap(),
            Action::Launch {
                executable: "/bin/echo".into(),
                args: vec!["one".into(), "two".into()]
            }
        );
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn application_argument_text_is_lossless_and_explicit() {
        let args = vec![
            String::new(),
            " leading".into(),
            "trailing ".into(),
            "two  spaces".into(),
            "line\nbreak".into(),
            r"\empty".into(),
        ];
        let text = format_application_args(&args);
        assert!(text.contains(r"\empty"));
        assert!(text.contains(r"\\empty"));
        assert_eq!(parse_application_args(&text).unwrap(), args);
        assert_eq!(parse_application_args("").unwrap(), Vec::<String>::new());
        assert_eq!(
            parse_application_args(
                r"\empty
\empty"
            )
            .unwrap(),
            vec!["", ""]
        );
        assert!(parse_application_args("literal\n\nblank").is_err());
    }

    #[test]
    fn open_application_action_uses_lossless_argument_text() {
        let (path, store) = temp_store();
        assert_eq!(
            action_from_fields(
                &store,
                ActionKind::OpenApplication,
                "",
                "/bin/echo",
                " trailing ",
                "",
                &[]
            )
            .unwrap(),
            Action::Launch {
                executable: "/bin/echo".into(),
                args: vec![" trailing ".into()]
            }
        );
        assert_eq!(
            action_from_fields(
                &store,
                ActionKind::OpenApplication,
                "",
                "/bin/echo",
                r"\empty",
                "",
                &[]
            )
            .unwrap(),
            Action::Launch {
                executable: "/bin/echo".into(),
                args: vec![String::new()]
            }
        );
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn layer_changes_load_and_remove_the_selected_binding_without_touching_legacy_exact_ticks() {
        let (path, mut store) = temp_store();
        store.create("Studio").unwrap();
        store
            .bind(
                "Base",
                Trigger::Rotation(1, 2),
                Action::Emit("legacy-exact".into()),
            )
            .unwrap();
        store
            .bind(
                "Studio",
                Trigger::KeyPress(1),
                Action::Command("date".into()),
            )
            .unwrap();

        assert_eq!(
            existing_action(&store, "Studio", &Trigger::KeyPress(1)),
            Some(Action::Command("date".into()))
        );
        assert!(store.unbind("Studio", &Trigger::KeyPress(1)).unwrap());
        assert_eq!(
            existing_action(&store, "Studio", &Trigger::KeyPress(1)),
            None
        );
        assert_eq!(
            existing_action(&store, "Base", &Trigger::Rotation(1, 2)),
            Some(Action::Emit("legacy-exact".into()))
        );
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn strip_gestures_list_swipes_tap_and_hold_with_default_paging() {
        use crate::binding_icons::BindingIcon;
        use crate::layers::SwipeDirection;
        let (path, mut store) = temp_store();
        let gestures = strip_gestures(&store, "Base");
        let shown: Vec<_> = gestures
            .iter()
            .map(|gesture| (gesture.gesture, gesture.icon, gesture.default))
            .collect();
        assert_eq!(
            shown,
            [
                (StripGesture::SwipeLeft, BindingIcon::NextPage, true),
                (StripGesture::SwipeRight, BindingIcon::PreviousPage, true),
            ]
        );
        store
            .bind(
                "Base",
                Trigger::TouchSwipe(SwipeDirection::Left),
                Action::None,
            )
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::TouchSwipe(SwipeDirection::Right),
                Action::Emit("lights".into()),
            )
            .unwrap();
        store
            .bind("Base", Trigger::TouchTap, Action::Command("ls".into()))
            .unwrap();
        store
            .bind("Base", Trigger::TouchPress, Action::None)
            .unwrap();
        let gestures = strip_gestures(&store, "Base");
        assert_eq!(
            gestures
                .iter()
                .map(|gesture| (gesture.gesture, gesture.icon, gesture.default))
                .collect::<Vec<_>>(),
            [
                (StripGesture::SwipeRight, BindingIcon::NamedEvent, false),
                (StripGesture::Tap, BindingIcon::Command, false),
            ],
            "an explicit None hides the default and shows nothing"
        );
        assert_eq!(gestures[1].description(), "Tap: Run ls");
        let _ = fs::remove_dir_all(path);
    }
}
