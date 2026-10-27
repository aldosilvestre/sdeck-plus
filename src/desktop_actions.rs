//! System actions for the serial action worker. No adapter invokes a shell.
//!
//! Text replaces the Wayland clipboard, then sends physical Ctrl+V; the focused
//! application must support that paste binding. Keyboard/uinput and compositor
//! failures are returned, not treated as successful text entry.

#[path = "desktop_actions/audio.rs"]
mod audio;
#[path = "desktop_actions/audio_output.rs"]
pub mod audio_output;
#[cfg(target_os = "linux")]
#[path = "desktop_actions/mouse.rs"]
mod mouse;
#[path = "desktop_actions/mpris.rs"]
mod mpris;
#[path = "desktop_actions/native.rs"]
mod native;

pub use mpris::{read_players, PlaybackStatus, PlayerState};

use crate::{
    keyboard::{parse_chord, Chord},
    layers::KeyboardSender,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, io, path::PathBuf};

/// Persistable system and multimedia actions.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum DesktopAction {
    ShortcutToggle {
        first: String,
        second: String,
    },
    Text {
        value: String,
        enter: bool,
    },
    Open {
        target: String,
    },
    Website {
        url: String,
    },
    CloseApplication {
        executable: String,
    },
    Media(MediaCommand),
    PlayAudio {
        path: PathBuf,
    },
    StopAudio,
    SetAudioOutput {
        name: String,
    },
    CycleAudioOutput,
    MouseScroll {
        direction: ScrollDirection,
        ticks: u16,
    },
}

/// MPRIS player transport and PipeWire output controls.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaCommand {
    PlayPause,
    Stop,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    Mute,
}

/// Relative wheel direction, independent of pointer position.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

impl DesktopAction {
    /// Checks action data without invoking a desktop adapter.
    ///
    /// # Errors
    /// Returns `InvalidInput` for unsupported chords, unsafe process names,
    /// empty targets, non-HTTP(S) websites, or zero scroll ticks.
    pub fn validate(&self) -> io::Result<()> {
        match self {
            Self::ShortcutToggle { first, second } => {
                chord(first)?;
                chord(second)?;
                Ok(())
            }
            Self::Text { value, .. } => {
                if value.contains('\0') {
                    Err(invalid("text cannot contain NUL"))
                } else {
                    Ok(())
                }
            }
            Self::Open { target } => {
                printable_target(target)?;
                if target.starts_with('-') {
                    Err(invalid("open target cannot begin with an option"))
                } else {
                    Ok(())
                }
            }
            Self::Website { url } => {
                printable_target(url)?;
                let rest = url
                    .strip_prefix("https://")
                    .or_else(|| url.strip_prefix("http://"))
                    .ok_or_else(|| invalid("website requires an http:// or https:// URL"))?;
                let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
                if authority.is_empty()
                    || authority.starts_with(':')
                    || authority.ends_with('@')
                    || url.chars().any(char::is_whitespace)
                {
                    Err(invalid(
                        "website requires a nonempty host and no whitespace",
                    ))
                } else {
                    Ok(())
                }
            }
            Self::CloseApplication { executable } => {
                // Linux comm is limited to 15 bytes. pkill consumes an ERE, even
                // with --exact; only literal basename characters are accepted.
                if executable.is_empty()
                    || executable.len() > 15
                    || executable.starts_with('-')
                    || !executable.bytes().any(|byte| byte.is_ascii_alphanumeric())
                    || !executable.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
                    })
                {
                    Err(invalid(
                        "close requires a literal process basename of 1-15 ASCII bytes",
                    ))
                } else {
                    Ok(())
                }
            }
            Self::PlayAudio { path } => {
                use std::os::unix::ffi::OsStrExt;
                if path.as_os_str().is_empty() || path.as_os_str().as_bytes().contains(&0) {
                    Err(invalid("audio requires a nonempty path without NUL"))
                } else {
                    Ok(())
                }
            }
            Self::SetAudioOutput { name } => printable_target(name),
            Self::MouseScroll { ticks, .. } => {
                if *ticks == 0 {
                    Err(invalid("scroll requires at least one tick"))
                } else {
                    Ok(())
                }
            }
            Self::Media(_) | Self::StopAudio | Self::CycleAudioOutput => Ok(()),
        }
    }

    /// Human-readable action description, with no desktop I/O.
    pub fn summary(&self) -> String {
        match self {
            Self::ShortcutToggle { first, second } => format!("Toggle {first} / {second}"),
            Self::Text { value, enter } => {
                format!("Paste {value:?}{}", if *enter { " + Enter" } else { "" })
            }
            Self::Open { target } => format!("Open {target}"),
            Self::Website { url } => format!("Website {url}"),
            Self::CloseApplication { executable } => format!("Close {executable}"),
            Self::Media(command) => format!("Media {command:?}"),
            Self::PlayAudio { path } => format!("Play audio {}", path.display()),
            Self::StopAudio => "Stop owned audio".into(),
            Self::SetAudioOutput { name } => format!("Set audio output {name}"),
            Self::CycleAudioOutput => "Rotate audio output".into(),
            Self::MouseScroll { direction, ticks } => format!("Scroll {direction:?} {ticks}"),
        }
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn printable_target(value: &str) -> io::Result<()> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        Err(invalid(
            "target must be nonempty and contain no control characters",
        ))
    } else {
        Ok(())
    }
}

fn chord(value: &str) -> io::Result<Chord> {
    parse_chord(&[], value).map_err(invalid)
}

// Kept private: test implementations replace only host boundaries, not action
// routing, validation, toggle transitions, or keyboard sends.
trait Backend: Send {
    fn run(&mut self, argv: &[String]) -> io::Result<()>;
    fn media(&mut self, command: MediaCommand) -> io::Result<()>;
    fn open(&mut self, target: &str) -> io::Result<()>;
    fn clipboard(&mut self, value: &str) -> io::Result<()>;
    fn play_audio(&mut self, path: &std::path::Path) -> io::Result<()>;
    fn stop_audio(&mut self) -> io::Result<()>;
    fn set_audio_output(&mut self, name: &str) -> io::Result<()>;
    fn cycle_audio_output(&mut self) -> io::Result<()>;
    fn scroll(&mut self, direction: ScrollDirection, ticks: u16) -> io::Result<()>;
    fn poll_errors(&mut self) -> Vec<String> {
        Vec::new()
    }
}

/// One runtime per serial worker; owns toggle state, audio, and the wheel device.
pub struct DesktopRuntime {
    backend: Box<dyn Backend>,
    second_next: BTreeSet<(String, String, String)>,
}

impl Default for DesktopRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopRuntime {
    /// Constructs a lazy runtime; no processes or input devices are opened.
    pub fn new() -> Self {
        Self {
            backend: Box::new(native::Native::default()),
            second_next: BTreeSet::new(),
        }
    }

    /// Executes one validated action using the worker's existing keyboard.
    ///
    /// Application open and audio playback return after spawn, not application
    /// exit. Adapter success means dispatch succeeded, not that an application
    /// consumed the key, opened a window, or decoded an audio stream.
    ///
    /// # Errors
    /// Returns invalid input, missing tools, compositor/device access errors,
    /// unsuccessful command exits, and keyboard injection failures.
    pub fn execute(
        &mut self,
        action: &DesktopAction,
        keyboard: &mut impl KeyboardSender,
    ) -> io::Result<()> {
        self.execute_for("", action, keyboard)
    }

    /// Executes in a stable control-binding context (for example layer+trigger).
    /// Identical shortcut pairs on distinct controls toggle independently.
    ///
    /// # Errors
    /// Returns the same validation and adapter errors as [`Self::execute`].
    pub fn execute_for(
        &mut self,
        binding_id: &str,
        action: &DesktopAction,
        keyboard: &mut impl KeyboardSender,
    ) -> io::Result<()> {
        action.validate()?;
        match action {
            DesktopAction::ShortcutToggle { first, second } => {
                let pair = (binding_id.to_owned(), first.clone(), second.clone());
                let second_now = self.second_next.contains(&pair);
                keyboard.send(&chord(if second_now { second } else { first })?)?;
                if second_now {
                    self.second_next.remove(&pair);
                } else {
                    self.second_next.insert(pair);
                }
                Ok(())
            }
            DesktopAction::Text { value, enter } => {
                self.backend.clipboard(value)?;
                keyboard.send(&chord("CTRL+V")?)?;
                if *enter {
                    keyboard.send(&chord("ENTER")?)?;
                }
                Ok(())
            }
            DesktopAction::Open { target } => self.backend.open(target),
            DesktopAction::Website { url } => self.backend.open(url),
            DesktopAction::CloseApplication { executable } => self.backend.run(&[
                "pkill".into(),
                "--exact".into(),
                "--signal".into(),
                "TERM".into(),
                "--".into(),
                executable.replace('.', "\\."),
            ]),
            DesktopAction::Media(command) => match command {
                MediaCommand::PlayPause
                | MediaCommand::Stop
                | MediaCommand::Next
                | MediaCommand::Previous => self.backend.media(*command),
                MediaCommand::VolumeUp => self.backend.run(&[
                    "wpctl".into(),
                    "set-volume".into(),
                    "--limit".into(),
                    "1.0".into(),
                    "@DEFAULT_AUDIO_SINK@".into(),
                    "5%+".into(),
                ]),
                MediaCommand::VolumeDown => self.backend.run(&[
                    "wpctl".into(),
                    "set-volume".into(),
                    "@DEFAULT_AUDIO_SINK@".into(),
                    "5%-".into(),
                ]),
                MediaCommand::Mute => self.backend.run(&[
                    "wpctl".into(),
                    "set-mute".into(),
                    "@DEFAULT_AUDIO_SINK@".into(),
                    "toggle".into(),
                ]),
            },
            DesktopAction::PlayAudio { path } => self.backend.play_audio(path),
            DesktopAction::StopAudio => self.backend.stop_audio(),
            DesktopAction::SetAudioOutput { name } => self.backend.set_audio_output(name),
            DesktopAction::CycleAudioOutput => self.backend.cycle_audio_output(),
            DesktopAction::MouseScroll { direction, ticks } => {
                self.backend.scroll(*direction, *ticks)
            }
        }
    }

    /// Drains failed background open exits and checks owned audio for completion.
    /// Call on the serial worker's receive timeout as well as after dispatch.
    /// Successful exits are reaped silently; each failed exit is reported once.
    pub fn poll_errors(&mut self) -> Vec<String> {
        self.backend.poll_errors()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Call {
        Run(Vec<String>),
        Media(MediaCommand),
        Open(String),
        Clipboard(String),
        Play(PathBuf),
        Stop,
        SetAudioOutput(String),
        CycleAudioOutput,
        Scroll(ScrollDirection, u16),
        Key(Chord),
    }

    #[derive(Default)]
    struct State {
        calls: Vec<Call>,
        fail_backend: bool,
        errors: Vec<String>,
    }

    struct FakeBackend(Arc<Mutex<State>>);
    impl FakeBackend {
        fn call(&self, call: Call) -> io::Result<()> {
            let mut state = self.0.lock().unwrap();
            state.calls.push(call);
            if state.fail_backend {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "adapter unavailable",
                ))
            } else {
                Ok(())
            }
        }
    }
    impl Backend for FakeBackend {
        fn run(&mut self, argv: &[String]) -> io::Result<()> {
            self.call(Call::Run(argv.to_vec()))
        }
        fn media(&mut self, command: MediaCommand) -> io::Result<()> {
            self.call(Call::Media(command))
        }
        fn open(&mut self, value: &str) -> io::Result<()> {
            self.call(Call::Open(value.into()))
        }
        fn clipboard(&mut self, value: &str) -> io::Result<()> {
            self.call(Call::Clipboard(value.into()))
        }
        fn play_audio(&mut self, path: &std::path::Path) -> io::Result<()> {
            self.call(Call::Play(path.into()))
        }
        fn stop_audio(&mut self) -> io::Result<()> {
            self.call(Call::Stop)
        }
        fn set_audio_output(&mut self, name: &str) -> io::Result<()> {
            self.call(Call::SetAudioOutput(name.into()))
        }
        fn cycle_audio_output(&mut self) -> io::Result<()> {
            self.call(Call::CycleAudioOutput)
        }
        fn scroll(&mut self, direction: ScrollDirection, ticks: u16) -> io::Result<()> {
            self.call(Call::Scroll(direction, ticks))
        }
        fn poll_errors(&mut self) -> Vec<String> {
            std::mem::take(&mut self.0.lock().unwrap().errors)
        }
    }

    struct FakeKeyboard {
        state: Arc<Mutex<State>>,
        fail_at: Option<usize>,
        sends: usize,
    }
    impl KeyboardSender for FakeKeyboard {
        fn send(&mut self, value: &Chord) -> io::Result<()> {
            self.state
                .lock()
                .unwrap()
                .calls
                .push(Call::Key(value.clone()));
            let index = self.sends;
            self.sends += 1;
            if self.fail_at == Some(index) {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "uinput denied",
                ))
            } else {
                Ok(())
            }
        }
    }

    fn fixture() -> (DesktopRuntime, FakeKeyboard, Arc<Mutex<State>>) {
        let state = Arc::new(Mutex::new(State::default()));
        (
            DesktopRuntime {
                backend: Box::new(FakeBackend(state.clone())),
                second_next: BTreeSet::new(),
            },
            FakeKeyboard {
                state: state.clone(),
                fail_at: None,
                sends: 0,
            },
            state,
        )
    }

    #[test]
    fn toggle_retries_same_chord_when_send_fails() {
        // Given a failed first send, and a repeat of the same toggle.
        let (mut runtime, mut keyboard, state) = fixture();
        keyboard.fail_at = Some(0);
        let action = DesktopAction::ShortcutToggle {
            first: "CTRL+A".into(),
            second: "ALT+B".into(),
        };
        assert!(runtime.execute(&action, &mut keyboard).is_err());
        // When the action is retried successfully.
        runtime.execute(&action, &mut keyboard).unwrap();
        // Then the first chord was retried instead of advancing.
        assert_eq!(
            state.lock().unwrap().calls,
            [
                Call::Key(chord("CTRL+A").unwrap()),
                Call::Key(chord("CTRL+A").unwrap()),
            ]
        );
    }

    #[test]
    fn toggle_alternates_after_successful_send() {
        // Given a completed first send.
        let (mut runtime, mut keyboard, state) = fixture();
        let action = DesktopAction::ShortcutToggle {
            first: "A".into(),
            second: "B".into(),
        };
        runtime.execute(&action, &mut keyboard).unwrap();
        // When the same toggle is sent again.
        runtime.execute(&action, &mut keyboard).unwrap();
        // Then its second chord is sent.
        assert_eq!(
            state.lock().unwrap().calls.last(),
            Some(&Call::Key(chord("B").unwrap()))
        );
    }

    #[test]
    fn identical_toggle_pairs_are_independent_per_binding() {
        // Given the same shortcuts already advanced on one control.
        let (mut runtime, mut keyboard, state) = fixture();
        let action = DesktopAction::ShortcutToggle {
            first: "A".into(),
            second: "B".into(),
        };
        runtime
            .execute_for("Base:Key1", &action, &mut keyboard)
            .unwrap();
        // When a distinct control executes that identical action.
        runtime
            .execute_for("Base:Key2", &action, &mut keyboard)
            .unwrap();
        // Then its first shortcut is sent independently.
        assert_eq!(
            state.lock().unwrap().calls,
            [
                Call::Key(chord("A").unwrap()),
                Call::Key(chord("A").unwrap()),
            ]
        );
    }

    #[test]
    fn asynchronous_errors_are_drained_without_action_dispatch() {
        // Given a completed background adapter failure.
        let (mut runtime, keyboard, state) = fixture();
        state.lock().unwrap().errors.push("exit failure".into());
        // When the worker polls once.
        let errors = runtime.poll_errors();
        // Then the error is returned with no input or process dispatch.
        assert_eq!(errors.len(), 1);
        assert!(state.lock().unwrap().errors.is_empty());
        assert_eq!(keyboard.sends, 0);
        assert!(state.lock().unwrap().calls.is_empty());
    }

    #[test]
    fn toggle_pairs_do_not_share_state_when_different() {
        // Given a prior successful toggle for another pair.
        let (mut runtime, mut keyboard, state) = fixture();
        runtime
            .execute(
                &DesktopAction::ShortcutToggle {
                    first: "A".into(),
                    second: "B".into(),
                },
                &mut keyboard,
            )
            .unwrap();
        // When a different pair is executed.
        runtime
            .execute(
                &DesktopAction::ShortcutToggle {
                    first: "C".into(),
                    second: "D".into(),
                },
                &mut keyboard,
            )
            .unwrap();
        // Then it starts at its first chord.
        assert_eq!(
            state.lock().unwrap().calls.last(),
            Some(&Call::Key(chord("C").unwrap()))
        );
    }

    #[test]
    fn text_preserves_unicode_and_orders_paste_before_enter() {
        // Given exact Unicode clipboard content.
        let (mut runtime, mut keyboard, state) = fixture();
        let value = "  日本語\nλ  ";
        // When text with Enter is dispatched.
        runtime
            .execute(
                &DesktopAction::Text {
                    value: value.into(),
                    enter: true,
                },
                &mut keyboard,
            )
            .unwrap();
        // Then clipboard ownership precedes paste and Enter.
        assert_eq!(
            state.lock().unwrap().calls,
            [
                Call::Clipboard(value.into()),
                Call::Key(chord("CTRL+V").unwrap()),
                Call::Key(chord("ENTER").unwrap()),
            ]
        );
    }

    #[test]
    fn text_without_enter_sends_only_paste() {
        // Given a text action without Enter.
        let (mut runtime, mut keyboard, state) = fixture();
        // When the action is dispatched.
        runtime
            .execute(
                &DesktopAction::Text {
                    value: "x".into(),
                    enter: false,
                },
                &mut keyboard,
            )
            .unwrap();
        // Then it has no Enter key.
        assert_eq!(
            state.lock().unwrap().calls,
            [
                Call::Clipboard("x".into()),
                Call::Key(chord("CTRL+V").unwrap()),
            ]
        );
    }

    #[test]
    fn text_sends_no_keys_when_clipboard_adapter_fails() {
        // Given a missing clipboard adapter or compositor.
        let (mut runtime, mut keyboard, state) = fixture();
        state.lock().unwrap().fail_backend = true;
        // When text is dispatched.
        let error = runtime
            .execute(
                &DesktopAction::Text {
                    value: "x".into(),
                    enter: true,
                },
                &mut keyboard,
            )
            .unwrap_err();
        // Then the error propagates before any key send.
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(state.lock().unwrap().calls, [Call::Clipboard("x".into())]);
    }

    #[test]
    fn text_does_not_send_enter_when_paste_fails() {
        // Given unavailable uinput on the paste send.
        let (mut runtime, mut keyboard, state) = fixture();
        keyboard.fail_at = Some(0);
        // When text is dispatched.
        let error = runtime
            .execute(
                &DesktopAction::Text {
                    value: "x".into(),
                    enter: true,
                },
                &mut keyboard,
            )
            .unwrap_err();
        // Then failure is explicit and Enter is not sent.
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(state.lock().unwrap().calls.len(), 2);
    }

    #[test]
    fn close_uses_an_exact_escaped_basename_and_term() {
        // Given a literal name containing an ERE metacharacter.
        let (mut runtime, mut keyboard, state) = fixture();
        // When the close action is dispatched.
        runtime
            .execute(
                &DesktopAction::CloseApplication {
                    executable: "app.bin".into(),
                },
                &mut keyboard,
            )
            .unwrap();
        // Then no shell, options, or broad regexp can change the target.
        assert_eq!(
            state.lock().unwrap().calls,
            [Call::Run(
                ["pkill", "--exact", "--signal", "TERM", "--", "app\\.bin"]
                    .map(String::from)
                    .to_vec(),
            )]
        );
    }

    #[test]
    fn audio_output_routes_stable_name_without_keyboard_input() {
        let (mut runtime, mut keyboard, state) = fixture();
        runtime
            .execute(
                &DesktopAction::SetAudioOutput {
                    name: "alsa_output.usb".into(),
                },
                &mut keyboard,
            )
            .unwrap();
        assert_eq!(
            state.lock().unwrap().calls,
            [Call::SetAudioOutput("alsa_output.usb".into())]
        );
        assert_eq!(keyboard.sends, 0);
    }

    #[test]
    fn invalid_audio_output_names_never_reach_backend() {
        for name in ["", "   ", "sink\nname", "sink\0name", "sink\u{7f}"] {
            let (mut runtime, mut keyboard, state) = fixture();
            let error = runtime
                .execute(
                    &DesktopAction::SetAudioOutput { name: name.into() },
                    &mut keyboard,
                )
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert!(state.lock().unwrap().calls.is_empty());
        }
    }

    #[test]
    fn audio_output_rotation_routes_each_binding_without_keyboard_input() {
        // Given two controls sharing the same rotation action.
        let (mut runtime, mut keyboard, state) = fixture();
        // When either control is pressed repeatedly.
        for binding in ["Base:Key1", "Base:Key2", "Base:Key1"] {
            runtime
                .execute_for(binding, &DesktopAction::CycleAudioOutput, &mut keyboard)
                .unwrap();
        }
        // Then every press reaches the backend without binding-local state.
        assert_eq!(state.lock().unwrap().calls, vec![Call::CycleAudioOutput; 3]);
        assert!(runtime.second_next.is_empty());
        assert_eq!(keyboard.sends, 0);
    }

    #[test]
    fn audio_output_serialized_shapes_remain_distinct() {
        // Given the existing named selection and the new unit action.
        for (action, expected) in [
            (
                DesktopAction::SetAudioOutput {
                    name: "sink".into(),
                },
                r#"{"SetAudioOutput":{"name":"sink"}}"#,
            ),
            (DesktopAction::CycleAudioOutput, r#""CycleAudioOutput""#),
        ] {
            // When serialized.
            let encoded = serde_json::to_string(&action).unwrap();
            // Then the persisted representation is unchanged for named outputs.
            assert_eq!(encoded, expected);
            assert_eq!(
                serde_json::from_str::<DesktopAction>(&encoded).unwrap(),
                action
            );
        }
    }

    #[test]
    fn invalid_inputs_never_reach_adapters() {
        // Given invalid input classes including patterns and options.
        let actions = [
            DesktopAction::CloseApplication {
                executable: ".*".into(),
            },
            DesktopAction::CloseApplication {
                executable: "-f".into(),
            },
            DesktopAction::CloseApplication {
                executable: "/bin/app".into(),
            },
            DesktopAction::CloseApplication {
                executable: "a|b".into(),
            },
            DesktopAction::CloseApplication {
                executable: "abcdefghijklmnop".into(),
            },
            DesktopAction::Open {
                target: "-help".into(),
            },
            DesktopAction::Open {
                target: "\n".into(),
            },
            DesktopAction::Website {
                url: "file:///etc/passwd".into(),
            },
            DesktopAction::Website {
                url: "https://".into(),
            },
            DesktopAction::Text {
                value: "a\0b".into(),
                enter: false,
            },
            DesktopAction::ShortcutToggle {
                first: "A".into(),
                second: "λ".into(),
            },
            DesktopAction::MouseScroll {
                direction: ScrollDirection::Up,
                ticks: 0,
            },
            DesktopAction::PlayAudio {
                path: PathBuf::new(),
            },
        ];
        for action in actions {
            let (mut runtime, mut keyboard, state) = fixture();
            // When each invalid action is executed.
            assert_eq!(
                runtime.execute(&action, &mut keyboard).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            // Then no host boundary is invoked.
            assert!(state.lock().unwrap().calls.is_empty());
        }
    }

    #[test]
    fn transport_commands_use_media_backend_without_keyboard_input() {
        // Given each transport operation and an injectable media backend.
        for command in [
            MediaCommand::PlayPause,
            MediaCommand::Stop,
            MediaCommand::Next,
            MediaCommand::Previous,
        ] {
            let (mut runtime, mut keyboard, state) = fixture();
            // When the media action is dispatched.
            runtime
                .execute(&DesktopAction::Media(command), &mut keyboard)
                .unwrap();
            // Then only media transport is used, independent of compositor keys.
            assert_eq!(state.lock().unwrap().calls, [Call::Media(command)]);
            assert_eq!(keyboard.sends, 0);
        }
    }

    #[test]
    fn output_controls_use_pipewire_default_sink() {
        // Given all output controls and their expected argv.
        for (command, expected) in [
            (
                MediaCommand::VolumeUp,
                vec![
                    "wpctl",
                    "set-volume",
                    "--limit",
                    "1.0",
                    "@DEFAULT_AUDIO_SINK@",
                    "5%+",
                ],
            ),
            (
                MediaCommand::VolumeDown,
                vec!["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"],
            ),
            (
                MediaCommand::Mute,
                vec!["wpctl", "set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"],
            ),
        ] {
            let (mut runtime, mut keyboard, state) = fixture();
            // When the output control is dispatched.
            runtime
                .execute(&DesktopAction::Media(command), &mut keyboard)
                .unwrap();
            // Then an explicit PipeWire command is invoked.
            assert_eq!(
                state.lock().unwrap().calls,
                [Call::Run(expected.into_iter().map(String::from).collect())]
            );
        }
    }

    #[test]
    fn adapter_errors_propagate_for_system_actions() {
        // Given an unavailable host adapter for each system action.
        for action in [
            DesktopAction::Open {
                target: "document.txt".into(),
            },
            DesktopAction::Website {
                url: "https://example.com".into(),
            },
            DesktopAction::CloseApplication {
                executable: "app".into(),
            },
            DesktopAction::Media(MediaCommand::Mute),
            DesktopAction::Media(MediaCommand::PlayPause),
            DesktopAction::Media(MediaCommand::Stop),
            DesktopAction::Media(MediaCommand::Next),
            DesktopAction::Media(MediaCommand::Previous),
            DesktopAction::PlayAudio {
                path: "clip.wav".into(),
            },
            DesktopAction::StopAudio,
            DesktopAction::CycleAudioOutput,
            DesktopAction::SetAudioOutput {
                name: "alsa_output.usb".into(),
            },
            DesktopAction::MouseScroll {
                direction: ScrollDirection::Right,
                ticks: 3,
            },
        ] {
            let (mut runtime, mut keyboard, state) = fixture();
            state.lock().unwrap().fail_backend = true;
            // When the action is dispatched.
            let error = runtime.execute(&action, &mut keyboard).unwrap_err();
            // Then the adapter's error is returned and no keys are sent.
            assert_eq!(error.kind(), io::ErrorKind::NotFound);
            assert_eq!(keyboard.sends, 0);
        }
    }

    #[test]
    fn all_action_shapes_roundtrip_through_serde() {
        // Given all persisted shapes with exact user fields.
        let mut actions = vec![
            DesktopAction::ShortcutToggle {
                first: "CTRL+A".into(),
                second: "B".into(),
            },
            DesktopAction::Text {
                value: "λ\n 日本語 ".into(),
                enter: true,
            },
            DesktopAction::Open {
                target: "/tmp/a b".into(),
            },
            DesktopAction::Website {
                url: "https://example.com?q=a".into(),
            },
            DesktopAction::CloseApplication {
                executable: "app".into(),
            },
            DesktopAction::PlayAudio {
                path: "/tmp/clip.wav".into(),
            },
            DesktopAction::StopAudio,
            DesktopAction::CycleAudioOutput,
            DesktopAction::SetAudioOutput {
                name: "alsa_output.usb".into(),
            },
        ];
        actions.extend(
            [
                MediaCommand::PlayPause,
                MediaCommand::Stop,
                MediaCommand::Next,
                MediaCommand::Previous,
                MediaCommand::VolumeUp,
                MediaCommand::VolumeDown,
                MediaCommand::Mute,
            ]
            .map(DesktopAction::Media),
        );
        actions.extend(
            [
                ScrollDirection::Up,
                ScrollDirection::Down,
                ScrollDirection::Left,
                ScrollDirection::Right,
            ]
            .map(|direction| DesktopAction::MouseScroll {
                direction,
                ticks: u16::MAX,
            }),
        );
        for action in actions {
            // When an action is serialized and deserialized.
            let encoded = serde_json::to_string(&action).unwrap();
            let decoded: DesktopAction = serde_json::from_str(&encoded).unwrap();
            // Then all fields survive exactly.
            assert_eq!(decoded, action);
            decoded.validate().unwrap();
        }
    }
}
