//! Persistent app-local input layers and action planning.

use crate::{
    keyboard::{parse_chord, supported_key_names, Chord, Keyboard, Modifier},
    protocol::Event,
};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

const VERSION: &str = "streamdeck-layers-v1";
const DEFAULT_LAYER: &str = "Base";
const LEGACY_DEFAULT_LAYER: &str = "default";
const MAX_LAYERS: usize = 64;
const MAX_FIELD: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SwipeDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Trigger {
    KeyPress(u8),
    EncoderPress(u8),
    Rotation(u8, i8),
    EncoderClockwise(u8),
    EncoderCounterClockwise(u8),
    TouchTap,
    TouchPress,
    TouchFlick,
    TouchSwipe(SwipeDirection),
}

impl Trigger {
    pub fn from_event(event: &Event) -> Option<Self> {
        Self::event_triggers(event).into_iter().next()
    }

    fn event_triggers(event: &Event) -> Vec<Self> {
        match *event {
            Event::Key {
                index,
                pressed: true,
            } => one_based(index).map(Self::KeyPress).into_iter().collect(),
            Event::EncoderButton {
                index,
                pressed: true,
            } => one_based(index)
                .map(Self::EncoderPress)
                .into_iter()
                .collect(),
            Event::Rotation { index, ticks } => {
                let Some(index) = one_based(index) else {
                    return Vec::new();
                };
                let mut triggers = vec![Self::Rotation(index, ticks)];
                if ticks > 0 {
                    triggers.push(Self::EncoderClockwise(index));
                } else if ticks < 0 {
                    triggers.push(Self::EncoderCounterClockwise(index));
                }
                triggers
            }
            Event::TouchTap { .. } => vec![Self::TouchTap],
            Event::TouchPress { .. } => vec![Self::TouchPress],
            Event::TouchFlick {
                start_x,
                start_y,
                end_x,
                end_y,
            } => {
                let mut triggers = Vec::new();
                if let Some(direction) = swipe_direction(start_x, start_y, end_x, end_y) {
                    triggers.push(Self::TouchSwipe(direction));
                }
                triggers.push(Self::TouchFlick);
                triggers
            }
            _ => Vec::new(),
        }
    }
}

fn one_based(index: usize) -> Option<u8> {
    index.checked_add(1)?.try_into().ok()
}

fn swipe_direction(start_x: u16, start_y: u16, end_x: u16, end_y: u16) -> Option<SwipeDirection> {
    let dx = i32::from(end_x) - i32::from(start_x);
    let dy = i32::from(end_y) - i32::from(start_y);
    if dx == 0 && dy == 0 {
        return None;
    }
    if dx.abs() >= dy.abs() {
        if dx >= 0 {
            Some(SwipeDirection::Right)
        } else {
            Some(SwipeDirection::Left)
        }
    } else if dy > 0 {
        Some(SwipeDirection::Down)
    } else {
        Some(SwipeDirection::Up)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    SwitchLayer(String),
    Launch {
        executable: PathBuf,
        args: Vec<String>,
    },
    Command(String),
    KeyPress {
        key: String,
    },
    Shortcut {
        modifiers: Vec<String>,
        key: String,
    },
    Emit(String),
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    pub bindings: BTreeMap<Trigger, Action>,
}

#[derive(Clone, Debug)]
pub struct LayerStore {
    root: PathBuf,
    layers: BTreeMap<String, Layer>,
    active: String,
    diagnostics: Vec<String>,
}

impl LayerStore {
    pub fn open() -> io::Result<Self> {
        let base = std::env::var_os("XDG_DATA_HOME")
            .filter(|p| Path::new(p).is_absolute())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no app data directory"))?;
        Self::open_at(base.join("streamdeck-plus-monitor"))
    }

    pub fn open_at(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        let mut store = Self {
            root,
            layers: BTreeMap::new(),
            active: DEFAULT_LAYER.into(),
            diagnostics: Vec::new(),
        };
        store.layers.insert(
            DEFAULT_LAYER.into(),
            Layer {
                name: DEFAULT_LAYER.into(),
                bindings: BTreeMap::new(),
            },
        );
        store.load()?;
        Ok(store)
    }
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<_> = self.layers.keys().cloned().collect();
        if let Some(base) = names.iter().position(|name| name == DEFAULT_LAYER) {
            names.swap(0, base);
        }
        names
    }
    pub fn active(&self) -> &str {
        &self.active
    }
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
    pub fn layers(&self) -> &BTreeMap<String, Layer> {
        &self.layers
    }
    pub fn select(&mut self, name: &str) -> bool {
        if self.layers.contains_key(name) {
            self.active = name.into();
            true
        } else {
            false
        }
    }
    pub fn create(&mut self, name: &str) -> io::Result<()> {
        let name = name.trim();
        validate_name(name)?;
        if self.layers.contains_key(name) || self.layers.len() >= MAX_LAYERS {
            return Err(invalid("duplicate layer or layer limit reached"));
        }
        self.layers.insert(
            name.into(),
            Layer {
                name: name.into(),
                bindings: BTreeMap::new(),
            },
        );
        Ok(())
    }
    pub fn remove(&mut self, name: &str) -> io::Result<bool> {
        if name == DEFAULT_LAYER || !self.layers.contains_key(name) {
            return Ok(false);
        }
        self.layers.remove(name).expect("checked");
        for layer in self.layers.values_mut() {
            layer.bindings.retain(
                |_, action| !matches!(action, Action::SwitchLayer(target) if target == name),
            );
        }
        if self.active == name {
            self.active = DEFAULT_LAYER.into();
        }
        Ok(true)
    }
    pub fn bind(&mut self, layer: &str, trigger: Trigger, action: Action) -> io::Result<()> {
        if !self.layers.contains_key(layer) {
            return Err(invalid("unknown layer"));
        }
        validate_trigger(&trigger)?;
        validate_action(&action)?;
        let action = normalize_keyboard_action(action)?;
        if let Action::SwitchLayer(target) = &action {
            if !self.layers.contains_key(target) {
                return Err(invalid("unknown target layer"));
            }
        }
        self.layers
            .get_mut(layer)
            .expect("checked")
            .bindings
            .insert(trigger.clone(), action);
        Ok(())
    }
    pub fn unbind(&mut self, layer: &str, trigger: &Trigger) -> io::Result<bool> {
        if !self.layers.contains_key(layer) {
            return Err(invalid("unknown layer"));
        }
        validate_trigger(trigger)?;
        Ok(self
            .layers
            .get_mut(layer)
            .expect("checked")
            .bindings
            .remove(trigger)
            .is_some())
    }
    pub fn matching_action(&self, event: &Event) -> Option<Action> {
        let layer = self.layers.get(&self.active)?;
        Trigger::event_triggers(event)
            .into_iter()
            .find_map(|trigger| layer.bindings.get(&trigger).cloned())
    }
    pub fn apply_switch(&mut self, target: &str) -> bool {
        self.select(target)
    }
    fn load(&mut self) -> io::Result<()> {
        let path = self.root.join("layers.conf");
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        let parsed = parse(&text);
        match parsed {
            Ok((layers, active)) => {
                self.layers = layers;
                self.active = if self.layers.contains_key(&active) {
                    active
                } else {
                    DEFAULT_LAYER.into()
                };
            }
            Err(message) => self
                .diagnostics
                .push(format!("{message}; using the default layer")),
        }
        Ok(())
    }
    pub fn persist(&self) -> io::Result<()> {
        let mut text = format!("{VERSION}\nactive\t{}\n", hex(self.active.as_bytes()));
        for layer in self.layers.values() {
            text.push_str(&format!("layer\t{}\n", hex(layer.name.as_bytes())));
            for (trigger, action) in &layer.bindings {
                text.push_str(&format!(
                    "binding\t{}\t{}\t{}\n",
                    hex(layer.name.as_bytes()),
                    encode_trigger(trigger),
                    encode_action(action)?
                ));
            }
        }
        let temp = self.root.join("layers.conf.tmp");
        let mut file = fs::File::create(&temp)?;
        use std::io::Write;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(temp, self.root.join("layers.conf"))
    }
}

fn validate_name(name: &str) -> io::Result<()> {
    if name.is_empty() || name.len() > 64 || name.chars().any(char::is_control) {
        Err(invalid("layer names must be 1-64 printable characters"))
    } else {
        Ok(())
    }
}
fn validate_trigger(t: &Trigger) -> io::Result<()> {
    match t {
        Trigger::KeyPress(n) if !(1..=8).contains(n) => Err(invalid("key index out of range")),
        Trigger::EncoderPress(n)
        | Trigger::Rotation(n, _)
        | Trigger::EncoderClockwise(n)
        | Trigger::EncoderCounterClockwise(n)
            if !(1..=4).contains(n) =>
        {
            Err(invalid("encoder index out of range"))
        }
        Trigger::Rotation(_, 0) => Err(invalid("rotation ticks cannot be zero")),
        _ => Ok(()),
    }
}
fn validate_action(a: &Action) -> io::Result<()> {
    match a {
        Action::SwitchLayer(n) => validate_name(n),
        Action::Launch { executable, args }
            if executable.as_os_str().is_empty()
                || args.len() > 128
                || args.iter().any(|a| a.len() > MAX_FIELD || a.contains('\0')) =>
        {
            Err(invalid("invalid executable or argument list"))
        }
        Action::Command(command)
            if command.trim().is_empty() || command.len() > MAX_FIELD || command.contains('\0') =>
        {
            Err(invalid(
                "commands must be non-empty shell strings without NUL bytes",
            ))
        }
        Action::KeyPress { key } if !valid_shortcut_atom(key) => {
            Err(invalid("invalid keyboard key field"))
        }
        Action::Shortcut { modifiers, key }
            if modifiers.len() > 8
                || !valid_shortcut_atom(key)
                || modifiers.iter().any(|m| !valid_shortcut_atom(m)) =>
        {
            Err(invalid("invalid keyboard shortcut fields"))
        }
        Action::Emit(n)
            if n.trim().is_empty() || n.len() > MAX_FIELD || n.chars().any(char::is_control) =>
        {
            Err(invalid("invalid internal event name"))
        }
        _ => Ok(()),
    }
}
fn valid_shortcut_atom(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 128
        && !value.contains(',')
        && !value.contains(';')
        && !value.contains('\0')
        && !value.chars().any(char::is_control)
}

/// Normalize new bindings only. Decoding and persistence retain legacy names.
pub fn normalize_keyboard_action(action: Action) -> io::Result<Action> {
    let (modifiers, key, key_only) = match &action {
        Action::KeyPress { key } => (&[][..], key.as_str(), true),
        Action::Shortcut { modifiers, key } => (modifiers.as_slice(), key.as_str(), false),
        _ => return Ok(action),
    };
    let chord = parse_chord(modifiers, key).map_err(|error| invalid(&error))?;
    if key_only && !chord.modifiers().is_empty() {
        return Err(invalid(
            "Press key accepts one key only; use Shortcut for modifiers",
        ));
    }
    let key = supported_key_names()
        .find(|name| parse_chord(&[], name).expect("supported key").key() == chord.key())
        .expect("parsed key has a canonical name")
        .to_owned();
    if key_only {
        Ok(Action::KeyPress { key })
    } else {
        Ok(Action::Shortcut {
            modifiers: chord
                .modifiers()
                .iter()
                .map(|modifier| {
                    match modifier {
                        Modifier::Control => "CTRL",
                        Modifier::Shift => "SHIFT",
                        Modifier::Alt => "ALT",
                        Modifier::Super => "SUPER",
                    }
                    .to_owned()
                })
                .collect(),
            key,
        })
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(value: &str) -> Result<String, String> {
    if value.len() > MAX_FIELD * 2 || !value.len().is_multiple_of(2) {
        return Err("invalid encoded field".into());
    }
    let bytes: Result<Vec<_>, _> = value
        .as_bytes()
        .chunks(2)
        .map(|c| {
            u8::from_str_radix(std::str::from_utf8(c).map_err(|_| "invalid hex")?, 16)
                .map_err(|_| "invalid hex".to_string())
        })
        .collect();
    String::from_utf8(bytes?).map_err(|_| "invalid UTF-8".into())
}
fn encode_trigger(t: &Trigger) -> String {
    match t {
        Trigger::KeyPress(n) => format!("k:{n}"),
        Trigger::EncoderPress(n) => format!("b:{n}"),
        Trigger::Rotation(n, ticks) => format!("r:{n}:{ticks}"),
        Trigger::EncoderClockwise(n) => format!("r:{n}:+"),
        Trigger::EncoderCounterClockwise(n) => format!("r:{n}:-"),
        Trigger::TouchTap => "tap".into(),
        Trigger::TouchPress => "press".into(),
        Trigger::TouchFlick => "flick".into(),
        Trigger::TouchSwipe(direction) => format!("swipe:{}", encode_swipe_direction(*direction)),
    }
}
fn encode_swipe_direction(direction: SwipeDirection) -> &'static str {
    match direction {
        SwipeDirection::Left => "left",
        SwipeDirection::Right => "right",
        SwipeDirection::Up => "up",
        SwipeDirection::Down => "down",
    }
}
fn decode_swipe_direction(value: &str) -> Result<SwipeDirection, String> {
    match value {
        "left" => Ok(SwipeDirection::Left),
        "right" => Ok(SwipeDirection::Right),
        "up" => Ok(SwipeDirection::Up),
        "down" => Ok(SwipeDirection::Down),
        _ => Err("invalid swipe direction".into()),
    }
}
fn decode_trigger(s: &str) -> Result<Trigger, String> {
    let p: Vec<_> = s.split(':').collect();
    let t = match p.as_slice() {
        ["k", n] => Trigger::KeyPress(n.parse().map_err(|_| "invalid key")?),
        ["b", n] => Trigger::EncoderPress(n.parse().map_err(|_| "invalid encoder")?),
        ["r", n, "+"] => Trigger::EncoderClockwise(n.parse().map_err(|_| "invalid encoder")?),
        ["r", n, "-"] => {
            Trigger::EncoderCounterClockwise(n.parse().map_err(|_| "invalid encoder")?)
        }
        ["r", n, ticks] => Trigger::Rotation(
            n.parse().map_err(|_| "invalid encoder")?,
            ticks.parse().map_err(|_| "invalid ticks")?,
        ),
        ["tap"] => Trigger::TouchTap,
        ["press"] => Trigger::TouchPress,
        ["flick"] => Trigger::TouchFlick,
        ["swipe", direction] => Trigger::TouchSwipe(decode_swipe_direction(direction)?),
        _ => return Err("invalid trigger".into()),
    };
    validate_trigger(&t).map_err(|e| e.to_string())?;
    Ok(t)
}
fn encode_action(a: &Action) -> io::Result<String> {
    validate_action(a)?;
    Ok(match a {
        Action::SwitchLayer(n) => format!("switch:{}", hex(n.as_bytes())),
        Action::Emit(n) => format!("emit:{}", hex(n.as_bytes())),
        Action::Launch { executable, args } => format!(
            "launchv2:{}:{}:{}",
            hex(executable.to_string_lossy().as_bytes()),
            args.len(),
            args.iter()
                .map(|a| hex(a.as_bytes()))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Action::Command(command) => format!("command:{}", hex(command.as_bytes())),
        Action::KeyPress { key } => format!("key:{}", hex(key.as_bytes())),
        Action::Shortcut { modifiers, key } => format!(
            "shortcut:{}:{}",
            modifiers
                .iter()
                .map(|m| hex(m.as_bytes()))
                .collect::<Vec<_>>()
                .join(","),
            hex(key.as_bytes())
        ),
    })
}
fn decode_action(s: &str) -> Result<Action, String> {
    let action = if let Some(n) = s.strip_prefix("switch:") {
        Action::SwitchLayer(unhex(n)?)
    } else if let Some(n) = s.strip_prefix("emit:") {
        Action::Emit(unhex(n)?)
    } else if let Some(rest) = s.strip_prefix("launchv2:") {
        let mut parts = rest.splitn(3, ':');
        let exe = parts.next().ok_or("invalid launch action")?;
        let count = parts
            .next()
            .ok_or("invalid launch action")?
            .parse::<usize>()
            .map_err(|_| "invalid launch argument count")?;
        let encoded_args = parts.next().ok_or("invalid launch action")?;
        let args = if count == 0 {
            if !encoded_args.is_empty() {
                return Err("invalid launch argument count".into());
            }
            Vec::new()
        } else {
            let args = encoded_args
                .split(',')
                .map(unhex)
                .collect::<Result<Vec<_>, _>>()?;
            if args.len() != count {
                return Err("invalid launch argument count".into());
            }
            args
        };
        Action::Launch {
            executable: PathBuf::from(unhex(exe)?),
            args,
        }
    } else if let Some(rest) = s.strip_prefix("launch:") {
        let (exe, args) = rest.split_once(':').ok_or("invalid launch action")?;
        Action::Launch {
            executable: PathBuf::from(unhex(exe)?),
            args: if args.is_empty() {
                Vec::new()
            } else {
                args.split(',').map(unhex).collect::<Result<_, _>>()?
            },
        }
    } else if let Some(command) = s.strip_prefix("command:") {
        Action::Command(unhex(command)?)
    } else if let Some(key) = s.strip_prefix("key:") {
        Action::KeyPress { key: unhex(key)? }
    } else if let Some(rest) = s.strip_prefix("shortcut:") {
        let (modifiers, key) = rest.split_once(':').ok_or("invalid shortcut action")?;
        Action::Shortcut {
            modifiers: if modifiers.is_empty() {
                Vec::new()
            } else {
                modifiers.split(',').map(unhex).collect::<Result<_, _>>()?
            },
            key: unhex(key)?,
        }
    } else {
        return Err("invalid action".into());
    };
    validate_action(&action).map_err(|e| e.to_string())?;
    Ok(action)
}
fn parse(text: &str) -> Result<(BTreeMap<String, Layer>, String), String> {
    let mut lines = text.lines();
    if lines.next() != Some(VERSION) {
        return Err("unsupported layer configuration version".into());
    }
    let mut layers = BTreeMap::new();
    layers.insert(
        DEFAULT_LAYER.into(),
        Layer {
            name: DEFAULT_LAYER.into(),
            bindings: BTreeMap::new(),
        },
    );
    let mut active = DEFAULT_LAYER.to_string();
    for row in lines {
        let cells: Vec<_> = row.split('\t').collect();
        match cells.as_slice() {
            ["active", value] => active = canonical_layer_name(&unhex(value)?),
            ["layer", value] => {
                let name = canonical_layer_name(&unhex(value)?);
                validate_name(&name).map_err(|e| e.to_string())?;
                let exists = layers.contains_key(&name);
                if layers.len() >= MAX_LAYERS || (exists && name != DEFAULT_LAYER) {
                    return Err("duplicate layer or layer limit".into());
                }
                layers.insert(
                    name.clone(),
                    Layer {
                        name,
                        bindings: BTreeMap::new(),
                    },
                );
            }
            ["binding", layer, trigger, action] => {
                let layer = canonical_layer_name(&unhex(layer)?);
                let trigger = decode_trigger(trigger)?;
                let mut action = decode_action(action)?;
                if let Action::SwitchLayer(target) = &mut action {
                    *target = canonical_layer_name(target);
                }
                let data = layers
                    .get_mut(&layer)
                    .ok_or("binding references missing layer")?;
                if data.bindings.insert(trigger, action).is_some() {
                    return Err("duplicate binding".into());
                }
            }
            _ => return Err("malformed layer configuration".into()),
        }
    }
    if layers
        .values()
        .flat_map(|layer| layer.bindings.values())
        .any(|action| matches!(action, Action::SwitchLayer(target) if !layers.contains_key(target)))
    {
        return Err("binding references missing target layer".into());
    }
    if !layers.contains_key(&active) {
        active = DEFAULT_LAYER.into();
    }
    Ok((layers, active))
}

fn canonical_layer_name(name: &str) -> String {
    if name == LEGACY_DEFAULT_LAYER {
        DEFAULT_LAYER.into()
    } else {
        name.into()
    }
}

pub trait ProcessRunner {
    fn run(&mut self, argv: &[String]) -> io::Result<()>;
}

#[derive(Default)]
pub struct SystemProcessRunner;

impl ProcessRunner for SystemProcessRunner {
    fn run(&mut self, argv: &[String]) -> io::Result<()> {
        let Some((program, args)) = argv.split_first() else {
            return Err(invalid("empty process argv"));
        };
        let status = Command::new(program).args(args).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("process exited with {status}")))
        }
    }
}

pub trait KeyboardSender {
    fn send(&mut self, chord: &Chord) -> io::Result<()>;
}

impl KeyboardSender for Keyboard {
    fn send(&mut self, chord: &Chord) -> io::Result<()> {
        Keyboard::send(self, chord)
    }
}

/// One instance belongs to the serial layer worker, never to a GTK callback.
pub struct ActionExecutor<P, K> {
    process: P,
    keyboard: K,
}

impl<P: ProcessRunner, K: KeyboardSender> ActionExecutor<P, K> {
    pub fn new(process: P, keyboard: K) -> Self {
        Self { process, keyboard }
    }

    pub fn execute(&mut self, action: &Action) -> io::Result<()> {
        validate_action(action)?;
        match action {
            Action::KeyPress { key } => {
                let chord = parse_chord(&[], key).map_err(|error| invalid(&error))?;
                if !chord.modifiers().is_empty() {
                    return Err(invalid(
                        "Press key accepts one key only; use Shortcut for modifiers",
                    ));
                }
                self.keyboard.send(&chord)
            }
            Action::Shortcut { modifiers, key } => {
                let chord = parse_chord(modifiers, key).map_err(|error| invalid(&error))?;
                self.keyboard.send(&chord)
            }
            _ => self.process.run(&action_argv(action)?),
        }
    }
}

fn action_argv(action: &Action) -> io::Result<Vec<String>> {
    Ok(match action {
        Action::Launch { executable, args } => {
            let mut argv = Vec::with_capacity(args.len() + 1);
            argv.push(executable.to_string_lossy().into_owned());
            argv.extend(args.iter().cloned());
            argv
        }
        Action::Command(command) => vec!["/bin/sh".into(), "-c".into(), command.clone()],
        Action::KeyPress { .. }
        | Action::Shortcut { .. }
        | Action::SwitchLayer(_)
        | Action::Emit(_) => {
            return Err(invalid("action does not execute an external process"));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static TEMP_ID: AtomicUsize = AtomicUsize::new(0);
    fn temp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "streamdeck-layers-{}-{}",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn persistence_selector_switch_and_safe_removal() {
        let p = temp();
        let mut s = LayerStore::open_at(&p).unwrap();
        s.create("work").unwrap();
        s.bind(
            "Base",
            Trigger::KeyPress(1),
            Action::SwitchLayer("work".into()),
        )
        .unwrap();
        assert!(!s.select("missing"));
        assert_eq!(
            s.matching_action(&Event::Key {
                index: 0,
                pressed: false
            }),
            None
        );
        let action = s.matching_action(&Event::Key {
            index: 0,
            pressed: true,
        });
        assert_eq!(action, Some(Action::SwitchLayer("work".into())));
        let Some(Action::SwitchLayer(target)) = action else {
            panic!("Base key binding should switch to the named layer");
        };
        assert!(s.apply_switch(&target));
        assert_eq!(s.active(), "work");
        s.persist().unwrap();
        drop(s);
        let mut s = LayerStore::open_at(&p).unwrap();
        assert_eq!(s.active(), "work");
        assert!(s.remove("work").unwrap());
        assert_eq!(s.active(), "Base");
        assert_eq!(
            s.matching_action(&Event::Key {
                index: 0,
                pressed: true
            }),
            None
        );
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn launch_arguments_persist_losslessly_for_empty_and_space_values() {
        let cases = vec![
            ("zero", Vec::<String>::new()),
            ("one-empty", vec![String::new()]),
            (
                "mixed",
                vec![String::new(), " leading".into(), "trailing ".into()],
            ),
        ];
        for (name, args) in cases {
            let p = temp();
            let mut s = LayerStore::open_at(&p).unwrap();
            s.bind(
                "Base",
                Trigger::KeyPress(1),
                Action::Launch {
                    executable: PathBuf::from(format!("/bin/{name}")),
                    args: args.clone(),
                },
            )
            .unwrap();
            s.persist().unwrap();
            drop(s);

            let s = LayerStore::open_at(&p).unwrap();
            assert_eq!(
                s.layers()["Base"].bindings.get(&Trigger::KeyPress(1)),
                Some(&Action::Launch {
                    executable: PathBuf::from(format!("/bin/{name}")),
                    args,
                })
            );
            let _ = fs::remove_dir_all(p);
        }
    }

    #[test]
    fn legacy_launch_records_decode_unchanged() {
        let action = decode_action(&format!(
            "launch:{}:{}",
            hex(b"/bin/echo"),
            [hex(b"one"), hex(b"two")].join(",")
        ))
        .unwrap();
        assert_eq!(
            action,
            Action::Launch {
                executable: PathBuf::from("/bin/echo"),
                args: vec!["one".into(), "two".into()],
            }
        );
        assert_eq!(
            decode_action(&format!("launch:{}:", hex(b"/bin/echo"))).unwrap(),
            Action::Launch {
                executable: PathBuf::from("/bin/echo"),
                args: Vec::new(),
            }
        );
    }

    #[test]
    fn unbind_removes_one_binding_and_persists_without_touching_exact_rotation() {
        let p = temp();
        let mut s = LayerStore::open_at(&p).unwrap();
        s.create("work").unwrap();
        s.bind("work", Trigger::KeyPress(1), Action::Command("date".into()))
            .unwrap();
        s.bind(
            "work",
            Trigger::Rotation(1, 2),
            Action::Emit("legacy-exact".into()),
        )
        .unwrap();

        assert!(s.unbind("work", &Trigger::KeyPress(1)).unwrap());
        assert!(!s.unbind("work", &Trigger::KeyPress(1)).unwrap());
        assert!(s.unbind("missing", &Trigger::KeyPress(1)).is_err());
        assert!(s.unbind("work", &Trigger::KeyPress(9)).is_err());
        s.persist().unwrap();
        drop(s);

        let s = LayerStore::open_at(&p).unwrap();
        assert_eq!(s.layers()["work"].bindings.get(&Trigger::KeyPress(1)), None);
        assert_eq!(
            s.layers()["work"].bindings.get(&Trigger::Rotation(1, 2)),
            Some(&Action::Emit("legacy-exact".into()))
        );
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn directional_encoder_and_swipe_triggers_are_resolved_deterministically() {
        let p = temp();
        let mut store = LayerStore::open_at(&p).unwrap();
        store
            .bind(
                "Base",
                Trigger::EncoderClockwise(1),
                Action::Emit("clockwise-any".into()),
            )
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::EncoderCounterClockwise(2),
                Action::Emit("counter-any".into()),
            )
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::Rotation(1, 1),
                Action::Emit("clockwise-exact".into()),
            )
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::TouchSwipe(SwipeDirection::Right),
                Action::Emit("swipe-right".into()),
            )
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::TouchSwipe(SwipeDirection::Left),
                Action::Emit("swipe-left".into()),
            )
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::TouchSwipe(SwipeDirection::Up),
                Action::Emit("swipe-up".into()),
            )
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::TouchSwipe(SwipeDirection::Down),
                Action::Emit("swipe-down".into()),
            )
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::TouchFlick,
                Action::Emit("generic-flick".into()),
            )
            .unwrap();

        assert_eq!(
            store.matching_action(&Event::Rotation { index: 0, ticks: 1 }),
            Some(Action::Emit("clockwise-exact".into()))
        );
        assert_eq!(
            store.matching_action(&Event::Rotation { index: 0, ticks: 7 }),
            Some(Action::Emit("clockwise-any".into()))
        );
        assert_eq!(
            store.matching_action(&Event::Rotation {
                index: 1,
                ticks: -9,
            }),
            Some(Action::Emit("counter-any".into()))
        );
        assert_eq!(
            store.matching_action(&Event::Rotation { index: 0, ticks: 0 }),
            None
        );
        assert_eq!(
            store.matching_action(&Event::Rotation { index: 9, ticks: 7 }),
            None
        );
        assert_eq!(
            store.matching_action(&Event::EncoderButton {
                index: 0,
                pressed: false,
            }),
            None
        );
        assert_eq!(
            store.matching_action(&Event::TouchFlick {
                start_x: 10,
                start_y: 10,
                end_x: 20,
                end_y: 12,
            }),
            Some(Action::Emit("swipe-right".into()))
        );
        assert_eq!(
            store.matching_action(&Event::TouchFlick {
                start_x: 20,
                start_y: 10,
                end_x: 10,
                end_y: 12,
            }),
            Some(Action::Emit("swipe-left".into()))
        );
        assert_eq!(
            store.matching_action(&Event::TouchFlick {
                start_x: 10,
                start_y: 20,
                end_x: 12,
                end_y: 10,
            }),
            Some(Action::Emit("swipe-up".into()))
        );
        assert_eq!(
            store.matching_action(&Event::TouchFlick {
                start_x: 10,
                start_y: 10,
                end_x: 12,
                end_y: 20,
            }),
            Some(Action::Emit("swipe-down".into()))
        );
        assert_eq!(
            store.matching_action(&Event::TouchFlick {
                start_x: 10,
                start_y: 10,
                end_x: 20,
                end_y: 20,
            }),
            Some(Action::Emit("swipe-right".into()))
        );
        assert_eq!(
            store.matching_action(&Event::TouchFlick {
                start_x: 10,
                start_y: 10,
                end_x: 10,
                end_y: 10,
            }),
            Some(Action::Emit("generic-flick".into()))
        );
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn directional_triggers_persist_and_layers_stay_isolated() {
        let p = temp();
        let mut store = LayerStore::open_at(&p).unwrap();
        store.create("Studio").unwrap();
        store
            .bind(
                "Base",
                Trigger::KeyPress(1),
                Action::SwitchLayer("Studio".into()),
            )
            .unwrap();
        store
            .bind(
                "Studio",
                Trigger::EncoderClockwise(1),
                Action::Emit("studio-clockwise".into()),
            )
            .unwrap();
        store
            .bind(
                "Studio",
                Trigger::TouchSwipe(SwipeDirection::Left),
                Action::Emit("studio-left".into()),
            )
            .unwrap();
        store.persist().unwrap();

        let mut store = LayerStore::open_at(&p).unwrap();
        assert_eq!(
            store.matching_action(&Event::Rotation { index: 0, ticks: 3 }),
            None
        );
        let Some(Action::SwitchLayer(target)) = store.matching_action(&Event::Key {
            index: 0,
            pressed: true,
        }) else {
            panic!("Base key should select the independently configured Studio layer");
        };
        assert!(store.apply_switch(&target));
        assert_eq!(store.active(), "Studio");
        assert_eq!(
            store.matching_action(&Event::Rotation { index: 0, ticks: 3 }),
            Some(Action::Emit("studio-clockwise".into()))
        );
        assert_eq!(
            store.matching_action(&Event::TouchFlick {
                start_x: 40,
                start_y: 10,
                end_x: 10,
                end_y: 30,
            }),
            Some(Action::Emit("studio-left".into()))
        );
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn rejects_incompatible_directional_trigger_indices() {
        assert!(validate_trigger(&Trigger::EncoderClockwise(0)).is_err());
        assert!(validate_trigger(&Trigger::EncoderCounterClockwise(5)).is_err());
        assert!(decode_trigger("r:1:+").is_ok());
        assert!(decode_trigger("r:1:-").is_ok());
        assert!(decode_trigger("swipe:left").is_ok());
        assert!(decode_trigger("swipe:diagonal").is_err());
    }

    #[test]
    fn event_capabilities_match_only_their_typed_trigger() {
        let p = temp();
        let mut store = LayerStore::open_at(&p).unwrap();
        let cases = [
            (
                Trigger::KeyPress(1),
                Event::Key {
                    index: 0,
                    pressed: true,
                },
                Some(Action::Emit("key-press".into())),
            ),
            (
                Trigger::EncoderPress(1),
                Event::EncoderButton {
                    index: 0,
                    pressed: true,
                },
                Some(Action::Emit("encoder-press".into())),
            ),
            (
                Trigger::Rotation(1, 2),
                Event::Rotation { index: 0, ticks: 2 },
                Some(Action::Emit("rotation-positive".into())),
            ),
            (
                Trigger::Rotation(1, -2),
                Event::Rotation {
                    index: 0,
                    ticks: -2,
                },
                Some(Action::Emit("rotation-negative".into())),
            ),
            (
                Trigger::TouchTap,
                Event::TouchTap { x: 10, y: 20 },
                Some(Action::Emit("touch-tap".into())),
            ),
            (
                Trigger::TouchPress,
                Event::TouchPress { x: 10, y: 20 },
                Some(Action::Emit("touch-press".into())),
            ),
            (
                Trigger::TouchFlick,
                Event::TouchFlick {
                    start_x: 10,
                    start_y: 20,
                    end_x: 30,
                    end_y: 40,
                },
                Some(Action::Emit("touch-flick".into())),
            ),
        ];

        for (trigger, _, action) in &cases {
            store
                .bind("Base", trigger.clone(), action.clone().unwrap())
                .unwrap();
        }

        for (trigger, event, expected) in &cases {
            if !matches!(trigger, Trigger::TouchFlick) {
                assert_eq!(Trigger::from_event(event), Some(trigger.clone()));
            }
            assert_eq!(store.matching_action(event), expected.clone());
        }

        let non_triggers = [
            Event::Key {
                index: 0,
                pressed: false,
            },
            Event::EncoderButton {
                index: 0,
                pressed: false,
            },
        ];
        for event in &non_triggers {
            assert_eq!(Trigger::from_event(event), None);
            assert_eq!(store.matching_action(event), None);
        }

        assert_eq!(
            store.matching_action(&Event::EncoderButton {
                index: 0,
                pressed: true,
            }),
            Some(Action::Emit("encoder-press".into()))
        );
        assert_ne!(
            Trigger::from_event(&Event::Rotation { index: 0, ticks: 2 }),
            Trigger::from_event(&Event::EncoderButton {
                index: 0,
                pressed: true,
            })
        );
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn invalid_config_and_actions_are_rejected_safely() {
        let p = temp();
        fs::write(p.join("layers.conf"), "bad\n").unwrap();
        let mut s = LayerStore::open_at(&p).unwrap();
        assert_eq!(s.active(), "Base");
        assert!(!s.diagnostics().is_empty());
        assert!(decode_trigger("k:9").is_err());
        assert!(decode_action("switch:6e6f7065").is_ok());
        assert!(s
            .bind("Base", Trigger::KeyPress(1), Action::Emit("\n".into()))
            .is_err());
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn migrates_legacy_default_layer_and_references_to_base() {
        let p = temp();
        let config = concat!(
            "streamdeck-layers-v1\n",
            "active\t64656661756c74\n",
            "layer\t64656661756c74\n",
            "layer\t576f726b\n",
            "binding\t64656661756c74\tk:1\tswitch:576f726b\n",
            "binding\t576f726b\tk:2\tswitch:64656661756c74\n",
        );
        fs::write(p.join("layers.conf"), config).unwrap();

        let mut store = LayerStore::open_at(&p).unwrap();
        assert_eq!(store.active(), "Base");
        assert_eq!(store.names(), vec!["Base", "Work"]);
        assert_eq!(
            store.matching_action(&Event::Key {
                index: 0,
                pressed: true,
            }),
            Some(Action::SwitchLayer("Work".into()))
        );
        store.apply_switch("Work");
        assert_eq!(
            store.matching_action(&Event::Key {
                index: 1,
                pressed: true,
            }),
            Some(Action::SwitchLayer("Base".into()))
        );
        store.persist().unwrap();
        let persisted = fs::read_to_string(p.join("layers.conf")).unwrap();
        assert!(!persisted.contains("64656661756c74"));
        assert_eq!(LayerStore::open_at(&p).unwrap().active(), "Work");
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn launches_with_argv_without_shell() {
        let args = vec!["a;touch /tmp/nope".to_string()];
        let mut command = Command::new("/usr/bin/printf");
        command.args(&args);
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![std::ffi::OsStr::new(&args[0])]
        );
    }

    #[test]
    fn persists_command_key_and_shortcut_actions() {
        let p = temp();
        let mut store = LayerStore::open_at(&p).unwrap();
        store
            .bind(
                "Base",
                Trigger::KeyPress(1),
                Action::Command("notify-send hi".into()),
            )
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::KeyPress(2),
                Action::KeyPress { key: "A".into() },
            )
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::KeyPress(3),
                Action::Shortcut {
                    modifiers: vec!["SUPER".into(), "SHIFT".into()],
                    key: "S".into(),
                },
            )
            .unwrap();
        store.persist().unwrap();

        let store = LayerStore::open_at(&p).unwrap();
        assert_eq!(
            store.matching_action(&Event::Key {
                index: 0,
                pressed: true
            }),
            Some(Action::Command("notify-send hi".into()))
        );
        assert_eq!(
            store.matching_action(&Event::Key {
                index: 1,
                pressed: true
            }),
            Some(Action::KeyPress { key: "A".into() })
        );
        assert_eq!(
            store.matching_action(&Event::Key {
                index: 2,
                pressed: true
            }),
            Some(Action::Shortcut {
                modifiers: vec!["SHIFT".into(), "SUPER".into()],
                key: "S".into(),
            })
        );
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn executor_routes_keyboard_events_and_preserves_process_argv() {
        let mut executor = ActionExecutor::new(MockRunner::default(), MockKeyboard::default());
        executor
            .execute(&Action::Launch {
                executable: "/usr/bin/printf".into(),
                args: vec!["a;touch /tmp/nope".into(), "".into(), " leading ".into()],
            })
            .unwrap();
        executor
            .execute(&Action::Command("echo $HOME; true".into()))
            .unwrap();
        executor
            .execute(&Action::KeyPress { key: "A".into() })
            .unwrap();
        executor
            .execute(&Action::Shortcut {
                modifiers: vec![],
                key: "CTRL + D".into(),
            })
            .unwrap();
        executor
            .execute(&Action::Shortcut {
                modifiers: vec![],
                key: "SUPER + RETURN".into(),
            })
            .unwrap();
        executor
            .execute(&Action::Shortcut {
                modifiers: vec!["SHIFT".into(), "CTRL".into()],
                key: "SUPER + RETURN".into(),
            })
            .unwrap();
        assert_eq!(
            executor.process.argv,
            vec![
                vec!["/usr/bin/printf", "a;touch /tmp/nope", "", " leading "],
                vec!["/bin/sh", "-c", "echo $HOME; true"],
            ]
        );
        use crate::keyboard::Event::{Key, Sync};
        let down = |code| Key {
            code,
            pressed: true,
        };
        let up = |code| Key {
            code,
            pressed: false,
        };
        assert_eq!(
            executor.keyboard.events,
            vec![
                Sync,
                down(30),
                Sync,
                up(30),
                Sync,
                Sync,
                down(29),
                Sync,
                down(32),
                Sync,
                up(32),
                Sync,
                up(29),
                Sync,
                down(125),
                Sync,
                down(28),
                Sync,
                up(28),
                Sync,
                up(125),
                Sync,
                down(29),
                down(42),
                down(125),
                Sync,
                down(28),
                Sync,
                up(28),
                Sync,
                up(125),
                up(42),
                up(29),
                Sync,
            ]
        );
    }

    #[test]
    fn legacy_key_records_and_modifier_free_shortcuts_keep_physical_triggers() {
        let trigger = decode_trigger("k:8").unwrap();
        assert_eq!(trigger, Trigger::KeyPress(8));
        assert_eq!(encode_trigger(&trigger), "k:8");
        for key in ["return", "XF86Unsupported"] {
            let record = format!("key:{}", hex(key.as_bytes()));
            let action = decode_action(&record).unwrap();
            assert_eq!(action, Action::KeyPress { key: key.into() });
            assert_eq!(encode_action(&action).unwrap(), record);
        }
        let action = normalize_keyboard_action(Action::Shortcut {
            modifiers: vec![],
            key: "return".into(),
        })
        .unwrap();
        assert_eq!(encode_action(&action).unwrap(), "shortcut::454e544552");
        assert_eq!(
            decode_action(&encode_action(&action).unwrap()).unwrap(),
            action
        );
        let mut executor = ActionExecutor::new(MockRunner::default(), MockKeyboard::default());
        executor
            .execute(&Action::KeyPress {
                key: "return".into(),
            })
            .unwrap();
        executor.execute(&action).unwrap();
        assert!(executor.process.argv.is_empty());
        assert_eq!(executor.keyboard.events.len(), 12);
        assert_eq!(executor.keyboard.events[..6], executor.keyboard.events[6..]);
    }

    #[test]
    fn unsupported_legacy_names_load_without_overwrite_and_fail_without_process() {
        let p = temp();
        let config = format!(
            "{VERSION}\nlayer\t{}\nbinding\t{}\tk:1\tkey:{}\nbinding\t{}\tk:2\tshortcut:{}:{}\n",
            hex(b"Base"),
            hex(b"Base"),
            hex(b"XF86Unsupported"),
            hex(b"Base"),
            hex(b"CONTROL"),
            hex(b"LegacyName")
        );
        fs::write(p.join("layers.conf"), &config).unwrap();
        let mut store = LayerStore::open_at(&p).unwrap();
        assert!(store.diagnostics().is_empty());
        let legacy = store.layers()["Base"].bindings.clone();
        assert!(store
            .bind(
                "Base",
                Trigger::KeyPress(1),
                Action::KeyPress {
                    key: "unknown-new-key".into()
                },
            )
            .is_err());
        assert_eq!(store.layers()["Base"].bindings, legacy);
        let mut executor = ActionExecutor::new(MockRunner::default(), MockKeyboard::default());
        for action in store.layers()["Base"].bindings.values() {
            let error = executor.execute(action).unwrap_err().to_string();
            assert!(error.contains("Unsupported physical key"));
            assert!(error.contains("use ASCII"));
        }
        assert!(executor.process.argv.is_empty());
        assert!(executor.keyboard.events.is_empty());
        assert_eq!(fs::read_to_string(p.join("layers.conf")).unwrap(), config);
        store.persist().unwrap();
        assert_eq!(
            fs::read_to_string(p.join("layers.conf")).unwrap(),
            config.replacen(
                &format!("{VERSION}\n"),
                &format!("{VERSION}\nactive\t{}\n", hex(b"Base")),
                1
            )
        );
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn keyboard_failure_has_no_process_fallback() {
        let mut executor = ActionExecutor::new(
            MockRunner::default(),
            MockKeyboard {
                fail: true,
                ..Default::default()
            },
        );
        let error = executor
            .execute(&Action::Shortcut {
                modifiers: vec![],
                key: "SUPER + RETURN".into(),
            })
            .unwrap_err();
        assert!(error.to_string().contains("uinput access denied"));
        assert!(executor.process.argv.is_empty());
        assert!(executor.execute(&Action::Emit("internal".into())).is_err());
        assert!(executor
            .execute(&Action::SwitchLayer("Base".into()))
            .is_err());
        assert!(executor.process.argv.is_empty());
    }

    #[derive(Default)]
    struct MockKeyboard {
        events: Vec<crate::keyboard::Event>,
        fail: bool,
    }
    impl crate::keyboard::EventSink for MockKeyboard {
        fn emit(&mut self, event: crate::keyboard::Event) -> io::Result<()> {
            self.events.push(event);
            Ok(())
        }
        fn invalidate(&mut self) {}
    }
    impl KeyboardSender for MockKeyboard {
        fn send(&mut self, chord: &Chord) -> io::Result<()> {
            if self.fail {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "uinput access denied",
                ));
            }
            crate::keyboard::emit_chord(self, chord)
        }
    }

    #[test]
    fn rejects_malformed_new_keys_and_accepts_modifier_aliases() {
        assert!(validate_action(&Action::KeyPress { key: "A,B".into() }).is_err());
        assert!(validate_action(&Action::Shortcut {
            modifiers: vec!["SUPER;exec".into()],
            key: "S".into(),
        })
        .is_err());
        assert!(normalize_keyboard_action(Action::Shortcut {
            modifiers: vec!["META".into()],
            key: "S".into(),
        })
        .is_ok());
        assert!(normalize_keyboard_action(Action::KeyPress {
            key: "unknown".into()
        })
        .is_err());
        assert!(normalize_keyboard_action(Action::Shortcut {
            modifiers: vec!["HYPER".into()],
            key: "S".into()
        })
        .is_err());
        assert!(validate_action(&Action::Shortcut {
            modifiers: vec!["SUPER".into(), "SHIFT".into()],
            key: "S".into(),
        })
        .is_ok());
    }

    #[test]
    fn reports_process_nonzero_as_action_failure() {
        let runner = MockRunner {
            next_error: Some("exit status: 2".into()),
            ..Default::default()
        };
        let mut executor = ActionExecutor::new(runner, MockKeyboard::default());
        let err = executor
            .execute(&Action::Command("false".into()))
            .expect_err("nonzero process status should be reported");
        assert!(err.to_string().contains("exit status: 2"));
    }

    #[derive(Default)]
    struct MockRunner {
        argv: Vec<Vec<String>>,
        next_error: Option<String>,
    }

    impl ProcessRunner for MockRunner {
        fn run(&mut self, argv: &[String]) -> io::Result<()> {
            self.argv.push(argv.to_vec());
            match self.next_error.take() {
                Some(error) => Err(io::Error::other(error)),
                None => Ok(()),
            }
        }
    }
}
