//! Persistent app-local input layers and action planning.

use crate::{
    keyboard::{parse_chord, supported_key_names, Chord, Keyboard, Modifier},
    protocol::Event,
};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command},
};

mod touchscreen;

const VERSION: &str = "streamdeck-layers-v1";
pub const DEFAULT_LAYER: &str = "Base";
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
    /// Tap on one of the four 200px sections of the touch strip.
    ZoneTap(u8),
    /// Press and hold on one of the four sections of the touch strip.
    ZonePress(u8),
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
            // A section binding wins over the whole-strip binding.
            Event::TouchTap { x, .. } => touch_zone(x)
                .map(Self::ZoneTap)
                .into_iter()
                .chain([Self::TouchTap])
                .collect(),
            Event::TouchPress { x, .. } => touch_zone(x)
                .map(Self::ZonePress)
                .into_iter()
                .chain([Self::TouchPress])
                .collect(),
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

/// Section (1 through 4) of the 800px touch strip that contains `x`.
fn touch_zone(x: u16) -> Option<u8> {
    u8::try_from(x / 200)
        .ok()
        .filter(|zone| *zone < 4)
        .map(|zone| zone + 1)
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
    None,
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
    Feature(crate::features::FeatureAction),
}

/// Optional follow-up of any binding: move once to the page stored in layer
/// `layer` (its key names the profile, folder and page) after `seconds`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageReturn {
    pub layer: String,
    pub seconds: u32,
}

impl PageReturn {
    /// # Errors
    /// Rejects invalid layer names and delays outside 1 second to 24 hours.
    pub fn validate(&self) -> io::Result<()> {
        validate_name(&self.layer)?;
        if !(1..=86_400).contains(&self.seconds) {
            return Err(invalid(
                "return delay must be between 1 second and 24 hours",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    pub bindings: BTreeMap<Trigger, Action>,
    pub returns: BTreeMap<Trigger, PageReturn>,
    pub displays: BTreeMap<Trigger, crate::info::InfoSource>,
}

#[derive(Clone, Debug)]
pub struct LayerStore {
    conflicts: Vec<ConfigConflict>,
    root: PathBuf,
    layers: BTreeMap<String, Layer>,
    active: String,
    diagnostics: Vec<String>,
    touchscreens: touchscreen::Touchscreens,
}

#[cfg(test)]
thread_local! {
    pub(crate) static INTERRUPT_TOUCH_SAVE: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

impl LayerStore {
    pub fn open() -> io::Result<Self> {
        let base = std::env::var_os("XDG_DATA_HOME")
            .filter(|p| Path::new(p).is_absolute())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no app data directory"))?;
        Self::open_at(base.join("sdeck-plus"))
    }

    pub fn open_at(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        touchscreen::recover(&root)?;
        let mut store = Self {
            root,
            layers: BTreeMap::new(),
            active: DEFAULT_LAYER.into(),
            diagnostics: Vec::new(),
            conflicts: Vec::new(),
            touchscreens: touchscreen::Touchscreens::default(),
        };
        store.layers.insert(
            DEFAULT_LAYER.into(),
            Layer {
                name: DEFAULT_LAYER.into(),
                bindings: BTreeMap::new(),
                returns: BTreeMap::new(),
                displays: BTreeMap::new(),
            },
        );
        store.load()?;
        match touchscreen::Touchscreens::load(&store.root, &store.layers) {
            Ok(touchscreens) => store.touchscreens = touchscreens,
            Err(error) => {
                store
                    .diagnostics
                    .push(format!("Invalid touchscreen configuration: {error}"));
                store.touchscreens.invalid = true;
            }
        }
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
    /// Shared storage root for organization and integration settings.
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
    /// Button settings removed while loading; also written to the conflict log.
    pub fn conflicts(&self) -> &[ConfigConflict] {
        &self.conflicts
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
        if self.has_configuration_owner(name) || self.layers.len() >= MAX_LAYERS {
            return Err(invalid("duplicate layer or layer limit reached"));
        }
        self.layers.insert(
            name.into(),
            Layer {
                name: name.into(),
                bindings: BTreeMap::new(),
                returns: BTreeMap::new(),
                displays: BTreeMap::new(),
            },
        );
        Ok(())
    }
    pub fn remove(&mut self, name: &str) -> io::Result<bool> {
        if name == DEFAULT_LAYER || !self.layers.contains_key(name) {
            return Ok(false);
        }
        self.layers.remove(name).expect("checked");
        self.touchscreens.remove_references(name);
        for layer in self.layers.values_mut() {
            layer.bindings.retain(
                |_, action| !matches!(action, Action::SwitchLayer(target) if target == name),
            );
            layer.returns.retain(|_, value| value.layer != name);
        }
        if self.active == name {
            self.active = DEFAULT_LAYER.into();
        }
        Ok(true)
    }
    pub fn bind(&mut self, layer: &str, trigger: Trigger, action: Action) -> io::Result<()> {
        let owner = self
            .binding_layer(layer, &trigger)
            .ok_or_else(|| invalid("unknown layer"))?
            .name
            .clone();
        self.bind_configuration(&owner, trigger, action)
    }

    /// Address an exact stored owner, without redirecting legacy touch rows.
    /// Bundle audio relocation must rewrite selected and unselected pages alike.
    pub(crate) fn bind_configuration(
        &mut self,
        owner: &str,
        trigger: Trigger,
        action: Action,
    ) -> io::Result<()> {
        validate_trigger(&trigger)?;
        validate_action(&action)?;
        validate_binding_action(&trigger, &action)?;
        let action = normalize_keyboard_action(action)?;
        if let Action::SwitchLayer(target) = &action {
            if !self.layers.contains_key(target) {
                return Err(invalid("unknown target layer"));
            }
        }
        let data = if self.layers.contains_key(owner) {
            self.layers.get_mut(owner)
        } else {
            self.touchscreens.page_mut(owner)
        }
        .ok_or_else(|| invalid("unknown configuration owner"))?;
        data.bindings.insert(trigger, action);
        Ok(())
    }
    pub fn unbind(&mut self, layer: &str, trigger: &Trigger) -> io::Result<bool> {
        if self.binding_layer(layer, trigger).is_none() {
            return Err(invalid("unknown layer"));
        }
        validate_trigger(trigger)?;
        let data = self.binding_layer_mut(layer, trigger).expect("checked");
        data.returns.remove(trigger);
        data.displays.remove(trigger);
        Ok(data.bindings.remove(trigger).is_some())
    }
    /// Set or clear the page return that follows an existing binding.
    ///
    /// # Errors
    /// Unknown layers, invalid values and triggers without a binding.
    pub fn set_return(
        &mut self,
        layer: &str,
        trigger: &Trigger,
        value: Option<PageReturn>,
    ) -> io::Result<()> {
        validate_trigger(trigger)?;
        if let Some(value) = &value {
            value.validate()?;
            if !self.layers.contains_key(&value.layer) {
                return Err(invalid("unknown return page"));
            }
        }
        let data = self
            .binding_layer_mut(layer, trigger)
            .ok_or_else(|| invalid("unknown layer"))?;
        match value {
            Some(value) => {
                if !data.bindings.contains_key(trigger) {
                    return Err(invalid("no binding for this control"));
                }
                data.returns.insert(trigger.clone(), value);
            }
            None => {
                data.returns.remove(trigger);
            }
        }
        Ok(())
    }
    /// Set or clear the live information drawn on a key bound to `trigger`.
    ///
    /// # Errors
    /// Unknown layers, non-key triggers and triggers without a binding.
    pub fn set_info(
        &mut self,
        layer: &str,
        trigger: &Trigger,
        value: Option<crate::info::InfoSource>,
    ) -> io::Result<()> {
        validate_trigger(trigger)?;
        let data = self
            .binding_layer_mut(layer, trigger)
            .ok_or_else(|| invalid("unknown layer"))?;
        match value {
            Some(source) => {
                if !shows_information(trigger) {
                    return Err(invalid(
                        "live information is available on keys, dials and the touch strip only",
                    ));
                }
                if !data.bindings.contains_key(trigger) {
                    return Err(invalid("no binding for this control"));
                }
                data.displays.insert(trigger.clone(), source);
            }
            None => {
                data.displays.remove(trigger);
            }
        }
        Ok(())
    }
    pub fn info_source(&self, layer: &str, trigger: &Trigger) -> Option<crate::info::InfoSource> {
        self.binding_layer(layer, trigger)?
            .displays
            .get(trigger)
            .copied()
    }
    /// Information drawn on one section of the touch strip: a dial's own
    /// section (press, then turns) or a tap or hold on that section.
    pub fn zone_info_source(&self, layer: &str, zone: u8) -> Option<crate::info::InfoSource> {
        [
            Trigger::EncoderPress(zone),
            Trigger::EncoderClockwise(zone),
            Trigger::EncoderCounterClockwise(zone),
            Trigger::ZoneTap(zone),
            Trigger::ZonePress(zone),
        ]
        .iter()
        .find_map(|trigger| self.info_source(layer, trigger))
    }
    /// Every source used by any layer, so the reader knows what to poll.
    pub fn info_sources(&self) -> std::collections::BTreeSet<crate::info::InfoSource> {
        self.configurations()
            .flat_map(|layer| layer.displays.values().copied())
            .collect()
    }
    pub fn page_return(&self, layer: &str, trigger: &Trigger) -> Option<PageReturn> {
        self.binding_layer(layer, trigger)?
            .returns
            .get(trigger)
            .cloned()
    }
    /// Swap two keypad bindings in memory without persisting the layer store.
    ///
    /// # Errors
    /// Returns an error for an unknown layer or keys outside 1..=8.
    pub fn swap_keys(&mut self, layer: &str, first: u8, second: u8) -> io::Result<()> {
        validate_trigger(&Trigger::KeyPress(first))?;
        validate_trigger(&Trigger::KeyPress(second))?;
        let data = self
            .layers
            .get_mut(layer)
            .ok_or_else(|| invalid("unknown layer"))?;
        if first == second {
            return Ok(());
        }
        let first = Trigger::KeyPress(first);
        let second = Trigger::KeyPress(second);
        let first_return = data.returns.remove(&first);
        let second_return = data.returns.remove(&second);
        if let Some(value) = first_return {
            data.returns.insert(second.clone(), value);
        }
        if let Some(value) = second_return {
            data.returns.insert(first.clone(), value);
        }
        let first_display = data.displays.remove(&first);
        let second_display = data.displays.remove(&second);
        if let Some(value) = first_display {
            data.displays.insert(second.clone(), value);
        }
        if let Some(value) = second_display {
            data.displays.insert(first.clone(), value);
        }
        let bindings = &mut data.bindings;
        let first_action = bindings.remove(&first);
        let second_action = bindings.remove(&second);
        if let Some(action) = first_action {
            bindings.insert(second, action);
        }
        if let Some(action) = second_action {
            bindings.insert(first, action);
        }
        Ok(())
    }

    pub fn matching_action(&self, event: &Event) -> Option<Action> {
        self.matching_binding(event).map(|(_, action)| action)
    }

    /// Return the winning trigger as well as its action for per-binding state.
    pub fn matching_binding(&self, event: &Event) -> Option<(Trigger, Action)> {
        Trigger::event_triggers(event)
            .into_iter()
            .find_map(|trigger| {
                self.binding(&self.active, &trigger)
                    .map(|action| (trigger, action))
            })
    }
    pub fn apply_switch(&mut self, target: &str) -> bool {
        self.select(target)
    }
    /// Log the removed settings and rewrite the file without them, so the
    /// application keeps starting. Failures here must never block startup.
    fn record_conflicts(&mut self, conflicts: Vec<ConfigConflict>) {
        if let Err(error) = log_conflicts(&self.root, &conflicts) {
            eprintln!("Could not write {CONFLICT_LOG}: {error}");
        }
        for conflict in &conflicts {
            eprintln!(
                "Removed button setting {} in {:?}: {}",
                conflict.control, conflict.layer, conflict.reason
            );
        }
        self.conflicts = conflicts;
        if let Err(error) = self.persist() {
            eprintln!("Could not save the cleaned layer configuration: {error}");
        }
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
            Ok((layers, active, conflicts)) => {
                self.layers = layers;
                self.active = if self.layers.contains_key(&active) {
                    active
                } else {
                    DEFAULT_LAYER.into()
                };
                if !conflicts.is_empty() {
                    self.record_conflicts(conflicts);
                }
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
            for (trigger, value) in &layer.returns {
                text.push_str(&format!(
                    "pagereturn\t{}\t{}\t{}\t{}\n",
                    hex(layer.name.as_bytes()),
                    encode_trigger(trigger),
                    hex(value.layer.as_bytes()),
                    value.seconds
                ));
            }
            for (trigger, source) in &layer.displays {
                text.push_str(&format!(
                    "info\t{}\t{}\t{}\n",
                    hex(layer.name.as_bytes()),
                    encode_trigger(trigger),
                    source.id()
                ));
            }
        }
        self.touchscreens
            .persist_with_layers(&self.root, text.as_bytes())
    }
}

fn validate_name(name: &str) -> io::Result<()> {
    if name.is_empty() || name.len() > 64 || name.chars().any(char::is_control) {
        Err(invalid("layer names must be 1-64 printable characters"))
    } else {
        Ok(())
    }
}
/// Keys draw information on their image; dials and touch sections draw it on
/// their section of the touch strip.
fn shows_information(trigger: &Trigger) -> bool {
    matches!(
        trigger,
        Trigger::KeyPress(_)
            | Trigger::EncoderPress(_)
            | Trigger::EncoderClockwise(_)
            | Trigger::EncoderCounterClockwise(_)
            | Trigger::ZoneTap(_)
            | Trigger::ZonePress(_)
    )
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
        Trigger::ZoneTap(n) | Trigger::ZonePress(n) if !(1..=4).contains(n) => {
            Err(invalid("touch section out of range"))
        }
        Trigger::Rotation(_, 0) => Err(invalid("rotation ticks cannot be zero")),
        _ => Ok(()),
    }
}
fn validate_action(a: &Action) -> io::Result<()> {
    match a {
        Action::Feature(action) => action.encode().map(|_| ()),
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
fn validate_binding_action(trigger: &Trigger, action: &Action) -> io::Result<()> {
    if matches!(
        action,
        Action::Feature(crate::features::FeatureAction::Touchscreen(_))
    ) && !trigger.is_touchscreen()
    {
        return Err(invalid(
            "touchscreen page actions require a touchscreen gesture",
        ));
    }
    Ok(())
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
        Trigger::ZoneTap(zone) => format!("ztap:{zone}"),
        Trigger::ZonePress(zone) => format!("zpress:{zone}"),
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
        ["ztap", zone] => Trigger::ZoneTap(zone.parse().map_err(|_| "invalid touch section")?),
        ["zpress", zone] => Trigger::ZonePress(zone.parse().map_err(|_| "invalid touch section")?),
        _ => return Err("invalid trigger".into()),
    };
    validate_trigger(&t).map_err(|e| e.to_string())?;
    Ok(t)
}
fn encode_action(a: &Action) -> io::Result<String> {
    validate_action(a)?;
    Ok(match a {
        Action::None => "none".into(),
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
        Action::Feature(action) => format!("feature-v1:{}", hex(action.encode()?.as_bytes())),
    })
}
fn decode_action(s: &str) -> Result<Action, String> {
    let action = if s == "none" {
        Action::None
    } else if let Some(value) = s.strip_prefix("feature-v1:") {
        Action::Feature(
            crate::features::FeatureAction::decode(&unhex(value)?)
                .map_err(|error| error.to_string())?,
        )
    } else if let Some(value) = s.strip_prefix("brightness-v1:") {
        Action::Feature(crate::features::FeatureAction::Device(
            crate::device_control::DeviceAction::Brightness(
                value.parse().map_err(|_| "invalid brightness")?,
            ),
        ))
    } else if let Some(value) = s.strip_prefix("scroll-v1:") {
        let (direction, ticks) = value.split_once(':').ok_or("invalid scroll action")?;
        let direction = match direction {
            "up" => crate::desktop_actions::ScrollDirection::Up,
            "down" => crate::desktop_actions::ScrollDirection::Down,
            "left" => crate::desktop_actions::ScrollDirection::Left,
            "right" => crate::desktop_actions::ScrollDirection::Right,
            _ => return Err("invalid scroll direction".into()),
        };
        Action::Feature(crate::features::FeatureAction::Desktop(
            crate::desktop_actions::DesktopAction::MouseScroll {
                direction,
                ticks: ticks.parse().map_err(|_| "invalid scroll ticks")?,
            },
        ))
    } else if let Some(n) = s.strip_prefix("switch:") {
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
/// A button setting that could not be loaded and was removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigConflict {
    pub layer: String,
    pub control: String,
    pub reason: String,
    pub row: String,
}

/// Written in the configuration folder, one line per removed button setting.
pub const CONFLICT_LOG: &str = "button-errors.log";

type Parsed = (BTreeMap<String, Layer>, String, Vec<ConfigConflict>);

/// Only an unsupported header rejects the file; any other broken row removes
/// just the setting it describes and is reported as a conflict.
fn parse(text: &str) -> Result<Parsed, String> {
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
            returns: BTreeMap::new(),
            displays: BTreeMap::new(),
        },
    );
    let mut active = DEFAULT_LAYER.to_string();
    let mut conflicts = Vec::new();
    for row in lines {
        if let Err(reason) = apply_row(&mut layers, &mut active, row) {
            conflicts.push(conflict_for(row, reason));
        }
    }
    let names: Vec<String> = layers.keys().cloned().collect();
    for layer in layers.values_mut() {
        let dangling: Vec<Trigger> = layer
            .bindings
            .iter()
            .filter(|(_, action)| {
                matches!(action, Action::SwitchLayer(target) if !names.contains(target))
            })
            .map(|(trigger, _)| trigger.clone())
            .collect();
        for trigger in dangling {
            layer.bindings.remove(&trigger);
            layer.returns.remove(&trigger);
            conflicts.push(ConfigConflict {
                layer: layer.name.clone(),
                control: encode_trigger(&trigger),
                reason: "binding references a missing target layer".into(),
                row: String::new(),
            });
        }
        let stale: Vec<Trigger> = layer
            .returns
            .iter()
            .filter(|(_, value)| !names.contains(&value.layer))
            .map(|(trigger, _)| trigger.clone())
            .collect();
        for trigger in stale {
            layer.returns.remove(&trigger);
            conflicts.push(ConfigConflict {
                layer: layer.name.clone(),
                control: encode_trigger(&trigger),
                reason: "page return references a missing page".into(),
                row: String::new(),
            });
        }
    }
    if !layers.contains_key(&active) {
        active = DEFAULT_LAYER.into();
    }
    Ok((layers, active, conflicts))
}

fn apply_row(
    layers: &mut BTreeMap<String, Layer>,
    active: &mut String,
    row: &str,
) -> Result<(), String> {
    let cells: Vec<_> = row.split('\t').collect();
    match cells.as_slice() {
        ["active", value] => *active = canonical_layer_name(&unhex(value)?),
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
                    returns: BTreeMap::new(),
                    displays: BTreeMap::new(),
                },
            );
        }
        ["binding", layer, trigger, action] => {
            let layer = canonical_layer_name(&unhex(layer)?);
            let trigger = decode_trigger(trigger)?;
            let mut action = decode_action(action)?;
            validate_binding_action(&trigger, &action).map_err(|error| error.to_string())?;
            if let Action::SwitchLayer(target) = &mut action {
                *target = canonical_layer_name(target);
            }
            let data = layers
                .get_mut(&layer)
                .ok_or("binding references missing layer")?;
            if data.bindings.contains_key(&trigger) {
                return Err("duplicate binding".into());
            }
            data.bindings.insert(trigger, action);
        }
        // Pre-release rows with a numeric page cannot be mapped to a page key.
        ["return", _, _, _, _] => {}
        ["pagereturn", layer, trigger, target, seconds] => {
            let layer = canonical_layer_name(&unhex(layer)?);
            let trigger = decode_trigger(trigger)?;
            let value = PageReturn {
                layer: canonical_layer_name(&unhex(target)?),
                seconds: seconds.parse().map_err(|_| "invalid return delay")?,
            };
            value.validate().map_err(|error| error.to_string())?;
            let data = layers
                .get_mut(&layer)
                .ok_or("return references missing layer")?;
            if !data.bindings.contains_key(&trigger) || data.returns.contains_key(&trigger) {
                return Err("invalid or duplicate page return".into());
            }
            data.returns.insert(trigger, value);
        }
        ["info", layer, trigger, source] => {
            let layer = canonical_layer_name(&unhex(layer)?);
            let trigger = decode_trigger(trigger)?;
            let source =
                crate::info::InfoSource::from_id(source).ok_or("unknown information source")?;
            let data = layers
                .get_mut(&layer)
                .ok_or("information references missing layer")?;
            if !shows_information(&trigger)
                || !data.bindings.contains_key(&trigger)
                || data.displays.contains_key(&trigger)
            {
                return Err("invalid or duplicate live information".into());
            }
            data.displays.insert(trigger, source);
        }
        _ => return Err("malformed layer configuration".into()),
    }
    Ok(())
}

fn conflict_for(row: &str, reason: String) -> ConfigConflict {
    let cells: Vec<_> = row.split('\t').collect();
    let layer = cells
        .get(1)
        .and_then(|cell| unhex(cell).ok())
        .filter(|name| !name.chars().any(char::is_control))
        .map_or_else(|| "?".to_owned(), |name| canonical_layer_name(&name));
    let control = match cells.first() {
        Some(&("binding" | "pagereturn" | "info")) => cells.get(2).map_or("?", |cell| *cell),
        _ => "-",
    };
    ConfigConflict {
        layer,
        control: control.chars().take(40).collect(),
        reason,
        row: row.chars().take(300).collect(),
    }
}

fn utc_timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = i64::try_from(secs / 86_400).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let rem = secs % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

fn log_conflicts(root: &Path, conflicts: &[ConfigConflict]) -> io::Result<()> {
    use std::io::Write;
    let stamp = utc_timestamp();
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(CONFLICT_LOG))?;
    for conflict in conflicts {
        writeln!(
            file,
            "{stamp}\tERROR\tprofile={:?}\tcontrol={}\treason={}\tremoved={}",
            conflict.layer, conflict.control, conflict.reason, conflict.row
        )?;
    }
    Ok(())
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
    /// Launch independently; test adapters can acknowledge without a real child.
    fn launch(&mut self, argv: &[String]) -> io::Result<Option<Child>> {
        self.run(argv)?;
        Ok(None)
    }
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

    fn launch(&mut self, argv: &[String]) -> io::Result<Option<Child>> {
        let Some((program, args)) = argv.split_first() else {
            return Err(invalid("empty process argv"));
        };
        Command::new(program).args(args).spawn().map(Some)
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
    desktop: crate::desktop_actions::DesktopRuntime,
    applications: Vec<(PathBuf, Child)>,
}

impl<P: ProcessRunner, K: KeyboardSender> ActionExecutor<P, K> {
    pub fn new(process: P, keyboard: K) -> Self {
        Self {
            process,
            keyboard,
            desktop: crate::desktop_actions::DesktopRuntime::new(),
            applications: Vec::new(),
        }
    }

    pub fn execute(&mut self, action: &Action) -> io::Result<()> {
        self.execute_for("legacy", action)
    }

    /// Keep stateful desktop actions independent for each physical binding.
    pub fn execute_for(&mut self, binding_id: &str, action: &Action) -> io::Result<()> {
        validate_action(action)?;
        match action {
            Action::None => Ok(()),
            Action::Launch { executable, .. } => {
                if let Some(child) = self.process.launch(&action_argv(action)?)? {
                    self.applications.push((executable.clone(), child));
                }
                Ok(())
            }
            Action::Feature(crate::features::FeatureAction::Desktop(action)) => self
                .desktop
                .execute_for(binding_id, action, &mut self.keyboard),
            Action::Feature(_) => Err(invalid("action requires the application runtime")),
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

    /// Report delayed adapter failures without waiting for an application exit.
    pub fn poll_errors(&mut self) -> Vec<String> {
        let mut errors = self.desktop.poll_errors();
        let mut index = 0;
        while index < self.applications.len() {
            match self.applications[index].1.try_wait() {
                Ok(None) => index += 1,
                outcome => {
                    let (executable, _) = self.applications.swap_remove(index);
                    match outcome {
                        Ok(Some(status)) if !status.success() => {
                            errors.push(format!("{} exited with {status}", executable.display()))
                        }
                        Err(error) => errors.push(format!("{}: {error}", executable.display())),
                        Ok(Some(_)) => {}
                        Ok(None) => unreachable!("running children are retained"),
                    }
                }
            }
        }
        errors
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
        Action::None
        | Action::KeyPress { .. }
        | Action::Shortcut { .. }
        | Action::SwitchLayer(_)
        | Action::Emit(_)
        | Action::Feature(_) => {
            return Err(invalid("action does not execute an external process"));
        }
    })
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod launch_tests;

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
    fn swap_keys_exchanges_actions_only_in_memory_and_selected_layer() {
        let root = temp();
        let mut store = LayerStore::open_at(&root).unwrap();
        store.create("Other").unwrap();
        for (layer, trigger, action) in [
            ("Base", Trigger::KeyPress(1), Action::Emit("first".into())),
            (
                "Base",
                Trigger::KeyPress(8),
                Action::Command("echo second".into()),
            ),
            ("Base", Trigger::KeyPress(3), Action::Emit("keep".into())),
            (
                "Base",
                Trigger::EncoderPress(1),
                Action::Emit("encoder".into()),
            ),
            ("Other", Trigger::KeyPress(1), Action::Emit("other".into())),
        ] {
            store.bind(layer, trigger, action).unwrap();
        }
        store.persist().unwrap();
        let original = store.layers()["Base"].bindings.clone();
        let other = store.layers()["Other"].bindings.clone();
        let persisted = fs::read(root.join("layers.conf")).unwrap();
        store.select("Other");

        store.swap_keys("Base", 1, 8).unwrap();

        let mut expected = original.clone();
        expected.insert(
            Trigger::KeyPress(1),
            original[&Trigger::KeyPress(8)].clone(),
        );
        expected.insert(
            Trigger::KeyPress(8),
            original[&Trigger::KeyPress(1)].clone(),
        );
        assert_eq!(store.layers()["Base"].bindings, expected);
        assert_eq!(store.layers()["Other"].bindings, other);
        assert_eq!(store.active(), "Other");
        assert_eq!(fs::read(root.join("layers.conf")).unwrap(), persisted);
        assert_eq!(
            LayerStore::open_at(&root).unwrap().layers()["Base"].bindings,
            original
        );
        store.persist().unwrap();
        assert_eq!(
            LayerStore::open_at(&root).unwrap().layers()["Base"].bindings,
            expected
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn swap_keys_moves_empty_bindings_and_validates_before_self_swap() {
        let root = temp();
        let mut store = LayerStore::open_at(&root).unwrap();
        let action = Action::Emit("move".into());
        store
            .bind("Base", Trigger::KeyPress(1), action.clone())
            .unwrap();
        store.swap_keys("Base", 1, 2).unwrap();
        assert!(!store.layers()["Base"]
            .bindings
            .contains_key(&Trigger::KeyPress(1)));
        assert_eq!(
            store.layers()["Base"].bindings.get(&Trigger::KeyPress(2)),
            Some(&action)
        );
        store.swap_keys("Base", 1, 2).unwrap();
        assert_eq!(
            store.layers()["Base"].bindings.get(&Trigger::KeyPress(1)),
            Some(&action)
        );
        let before = store.layers()["Base"].bindings.clone();
        for (first, second) in [(1, 1), (8, 8), (3, 4)] {
            store.swap_keys("Base", first, second).unwrap();
            assert_eq!(store.layers()["Base"].bindings, before);
        }
        for (layer, first, second) in [
            ("Base", 0, 1),
            ("Base", 1, 9),
            ("Base", 9, 1),
            ("Base", 1, 0),
            ("Base", 0, 0),
            ("Base", 9, 9),
            ("missing", 1, 2),
            ("missing", 1, 1),
        ] {
            assert_eq!(
                store.swap_keys(layer, first, second).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            assert_eq!(store.layers()["Base"].bindings, before);
        }
        assert!(!root.join("layers.conf").exists());
        fs::remove_dir_all(root).unwrap();
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
            match trigger {
                // A tap or hold names its touch section first, then the strip.
                Trigger::TouchTap | Trigger::TouchPress => {
                    assert!(Trigger::event_triggers(event).contains(trigger));
                }
                Trigger::TouchFlick => {}
                _ => assert_eq!(Trigger::from_event(event), Some(trigger.clone())),
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
    fn none_does_not_send_keys_or_start_a_process() {
        let mut executor = ActionExecutor::new(
            MockRunner {
                next_error: Some("must not run".into()),
                ..Default::default()
            },
            MockKeyboard {
                fail: true,
                ..Default::default()
            },
        );
        executor.execute(&Action::None).unwrap();
        assert!(executor.process.argv.is_empty());
        assert!(executor.keyboard.events.is_empty());
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
