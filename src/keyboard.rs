//! Physical evdev chords, not text entry or compositor commands.
use std::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Modifier {
    Control,
    Shift,
    Alt,
    Super,
}
impl Modifier {
    fn code(self) -> u16 {
        match self {
            Self::Control => 29,
            Self::Shift => 42,
            Self::Alt => 56,
            Self::Super => 125,
        }
    }
    fn parse(value: &str) -> Option<Self> {
        match value {
            "CTRL" | "CONTROL" => Some(Self::Control),
            "SHIFT" => Some(Self::Shift),
            "ALT" => Some(Self::Alt),
            "SUPER" | "WIN" | "WINDOWS" | "META" => Some(Self::Super),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalKey(u16);
impl PhysicalKey {
    pub fn code(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chord {
    modifiers: Vec<Modifier>,
    key: PhysicalKey,
}
impl Chord {
    pub fn modifiers(&self) -> &[Modifier] {
        &self.modifiers
    }
    pub fn key(&self) -> PhysicalKey {
        self.key
    }
}

// Linux evdev positions, intentionally independent of the active symbol layout.
// Device capabilities come from this same complete supported-name table.
const KEYS: &[(&str, u16)] = &[
    ("ESCAPE", 1),
    ("1", 2),
    ("2", 3),
    ("3", 4),
    ("4", 5),
    ("5", 6),
    ("6", 7),
    ("7", 8),
    ("8", 9),
    ("9", 10),
    ("0", 11),
    ("MINUS", 12),
    ("EQUAL", 13),
    ("BACKSPACE", 14),
    ("TAB", 15),
    ("Q", 16),
    ("W", 17),
    ("E", 18),
    ("R", 19),
    ("T", 20),
    ("Y", 21),
    ("U", 22),
    ("I", 23),
    ("O", 24),
    ("P", 25),
    ("LEFTBRACE", 26),
    ("RIGHTBRACE", 27),
    ("ENTER", 28),
    ("A", 30),
    ("S", 31),
    ("D", 32),
    ("F", 33),
    ("G", 34),
    ("H", 35),
    ("J", 36),
    ("K", 37),
    ("L", 38),
    ("SEMICOLON", 39),
    ("APOSTROPHE", 40),
    ("GRAVE", 41),
    ("BACKSLASH", 43),
    ("Z", 44),
    ("X", 45),
    ("C", 46),
    ("V", 47),
    ("B", 48),
    ("N", 49),
    ("M", 50),
    ("COMMA", 51),
    ("DOT", 52),
    ("SLASH", 53),
    ("KPASTERISK", 55),
    ("SPACE", 57),
    ("CAPSLOCK", 58),
    ("F1", 59),
    ("F2", 60),
    ("F3", 61),
    ("F4", 62),
    ("F5", 63),
    ("F6", 64),
    ("F7", 65),
    ("F8", 66),
    ("F9", 67),
    ("F10", 68),
    ("NUMLOCK", 69),
    ("SCROLLLOCK", 70),
    ("KP7", 71),
    ("KP8", 72),
    ("KP9", 73),
    ("KPMINUS", 74),
    ("KP4", 75),
    ("KP5", 76),
    ("KP6", 77),
    ("KPPLUS", 78),
    ("KP1", 79),
    ("KP2", 80),
    ("KP3", 81),
    ("KP0", 82),
    ("KPDOT", 83),
    ("102ND", 86),
    ("F11", 87),
    ("F12", 88),
    ("KPENTER", 96),
    ("KPSLASH", 98),
    ("SYSRQ", 99),
    ("HOME", 102),
    ("UP", 103),
    ("PAGEUP", 104),
    ("LEFT", 105),
    ("RIGHT", 106),
    ("END", 107),
    ("DOWN", 108),
    ("PAGEDOWN", 109),
    ("INSERT", 110),
    ("DELETE", 111),
    ("MUTE", 113),
    ("VOLUMEDOWN", 114),
    ("VOLUMEUP", 115),
    ("PAUSE", 119),
    ("COMPOSE", 127),
    ("STOP", 128),
    ("MENU", 139),
    ("NEXTSONG", 163),
    ("PLAYPAUSE", 164),
    ("PREVIOUSSONG", 165),
    ("STOPCD", 166),
    ("F13", 183),
    ("F14", 184),
    ("F15", 185),
    ("F16", 186),
    ("F17", 187),
    ("F18", 188),
    ("F19", 189),
    ("F20", 190),
    ("F21", 191),
    ("F22", 192),
    ("F23", 193),
    ("F24", 194),
];

/// Canonical physical key names accepted by the parser (aliases excluded).
pub fn supported_key_names() -> impl Iterator<Item = &'static str> {
    KEYS.iter().map(|(name, _)| *name)
}

/// Merge checkbox modifiers with a `+`-separated chord. Uppercase letters do
/// not imply Shift. Names select physical positions; Unicode text is unsupported.
pub fn parse_chord(modifiers: &[String], text: &str) -> Result<Chord, String> {
    let mut normalized = Vec::new();
    for value in modifiers {
        let name = value.trim().to_ascii_uppercase();
        normalized.push(Modifier::parse(&name).ok_or_else(|| {
            format!("Unsupported modifier {value:?}; use CTRL, SHIFT, ALT or SUPER")
        })?);
    }
    let mut key = None;
    for segment in text.split('+') {
        let name = segment.trim().to_ascii_uppercase();
        if name.is_empty() {
            return Err("Empty chord segment; use e.g. CTRL + D".into());
        }
        if let Some(modifier) = Modifier::parse(&name) {
            normalized.push(modifier);
            continue;
        }
        let name = match name.as_str() {
            "RETURN" => "ENTER",
            "ESC" => "ESCAPE",
            "SPACEBAR" => "SPACE",
            "BACKSPACE" | "BACK_SPACE" => "BACKSPACE",
            "PAGE_UP" | "PRIOR" => "PAGEUP",
            "PAGE_DOWN" | "NEXT" => "PAGEDOWN",
            "PRINTSCREEN" | "PRINT" => "SYSRQ",
            "PERIOD" => "DOT",
            "BRACKETLEFT" => "LEFTBRACE",
            "BRACKETRIGHT" => "RIGHTBRACE",
            "KP_ENTER" => "KPENTER",
            other => other,
        };
        let code = KEYS.iter().find(|(candidate, _)| *candidate == name).map(|(_, code)| *code)
            .ok_or_else(|| format!("Unsupported physical key {segment:?}; use ASCII A-Z/0-9, F1-F24, Enter/Return, Tab, Escape, Space, navigation, named punctuation (e.g. Minus/Slash), KP0-KP9/KPEnter or media key names; not Unicode text"))?;
        if key.replace(PhysicalKey(code)).is_some() {
            return Err("A chord must contain exactly one main key".into());
        }
    }
    normalized.sort_unstable();
    normalized.dedup();
    Ok(Chord {
        modifiers: normalized,
        key: key.ok_or("A chord needs a main physical key, not only modifiers")?,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Key { code: u16, pressed: bool },
    Sync,
}

/// Implementations must invalidate the device on request; tests need no device.
pub trait EventSink {
    fn emit(&mut self, event: Event) -> io::Result<()>;
    fn invalidate(&mut self);
    fn is_valid(&self) -> bool {
        true
    }
}

/// Each phase ends with SYN_REPORT. Track before attempting down, because an
/// error cannot prove the kernel did not receive it. Cleanup keeps the first
/// error, attempts every possible release, then syncs even if releases fail.
pub fn emit_chord(sink: &mut impl EventSink, chord: &Chord) -> io::Result<()> {
    let mut potential = Vec::new();
    let result = (|| {
        for modifier in &chord.modifiers {
            potential.push(modifier.code());
            sink.emit(Event::Key {
                code: modifier.code(),
                pressed: true,
            })?;
        }
        sink.emit(Event::Sync)?;
        potential.push(chord.key.code());
        sink.emit(Event::Key {
            code: chord.key.code(),
            pressed: true,
        })?;
        sink.emit(Event::Sync)?;
        sink.emit(Event::Key {
            code: chord.key.code(),
            pressed: false,
        })?;
        sink.emit(Event::Sync)?;
        for modifier in chord.modifiers.iter().rev() {
            sink.emit(Event::Key {
                code: modifier.code(),
                pressed: false,
            })?;
        }
        sink.emit(Event::Sync)
    })();
    if let Err(original) = result {
        let mut clean = true;
        for code in potential.into_iter().rev() {
            if sink
                .emit(Event::Key {
                    code,
                    pressed: false,
                })
                .is_err()
            {
                clean = false;
            }
        }
        if sink.emit(Event::Sync).is_err() {
            clean = false;
        }
        if !clean {
            sink.invalidate();
        }
        return Err(original);
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
struct Owner<S> {
    device: Option<S>,
}
#[cfg(any(target_os = "linux", test))]
impl<S: EventSink> Owner<S> {
    fn send(&mut self, chord: &Chord, create: impl FnOnce() -> io::Result<S>) -> io::Result<()> {
        if self.device.is_none() {
            self.device = Some(create()?);
        }
        let device = self.device.as_mut().expect("initialized above");
        let result = emit_chord(device, chord);
        if !device.is_valid() {
            self.device = None;
        }
        result
    }
}

/// Keep one owner in the serial action worker. Construction is inert; the
/// first send opens uinput, subsequent sends reuse it. Drop destroys it.
#[derive(Default)]
pub struct Keyboard {
    #[cfg(target_os = "linux")]
    owner: Owner<linux::Device<linux::Native>>,
}
impl Keyboard {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn send(&mut self, chord: &Chord) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        {
            self.owner
                .send(chord, || linux::Device::new(linux::Native::open()?))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = chord;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Physical keyboard injection requires Linux uinput",
            ))
        }
    }
}
#[cfg(target_os = "linux")]
impl<S> Default for Owner<S> {
    fn default() -> Self {
        Self { device: None }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{
        fs::{File, OpenOptions},
        io::Write,
        mem,
        os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
        thread,
        time::Duration,
    };

    // linux/uinput.h numbers; libc builders encode the target architecture's
    // _IOC direction/size fields rather than assuming asm-generic bit shifts.
    const CREATE: libc::Ioctl = libc::_IO(b'U' as u32, 1);
    const DESTROY: libc::Ioctl = libc::_IO(b'U' as u32, 2);
    const SETUP: libc::Ioctl = libc::_IOW::<libc::uinput_setup>(b'U' as u32, 3);
    const EVBIT: libc::Ioctl = libc::_IOW::<libc::c_int>(b'U' as u32, 100);
    const KEYBIT: libc::Ioctl = libc::_IOW::<libc::c_int>(b'U' as u32, 101);

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum Command {
        Event(u16),
        Key(u16),
        Setup,
        Create,
        Ready,
        Destroy,
    }
    pub(super) trait Backend: Write {
        fn command(&mut self, command: Command) -> io::Result<()>;
    }
    pub(super) struct Device<B: Backend> {
        backend: Option<B>,
    }
    impl<B: Backend> Device<B> {
        pub(super) fn new(backend: B) -> io::Result<Self> {
            // Install the teardown guard before any ioctl, including setup and
            // create failures. Closing the owned descriptor is the final fallback.
            let mut device = Self {
                backend: Some(backend),
            };
            let backend = device.backend.as_mut().expect("just installed");
            backend.command(Command::Event(1))?; // EV_KEY; EV_SYN is implicit.
            let mut codes: Vec<_> = KEYS.iter().map(|(_, code)| *code).collect();
            codes.extend([29, 42, 56, 125]);
            codes.sort_unstable();
            codes.dedup();
            for code in codes {
                backend.command(Command::Key(code))?;
            }
            backend.command(Command::Setup)?;
            backend.command(Command::Create)?;
            backend.command(Command::Ready)?;
            Ok(device)
        }
        fn teardown(&mut self) {
            if let Some(mut backend) = self.backend.take() {
                let _ = backend.command(Command::Destroy);
                // The File closes even when UI_DEV_DESTROY fails.
            }
        }
    }
    impl<B: Backend> Drop for Device<B> {
        fn drop(&mut self) {
            self.teardown();
        }
    }
    impl<B: Backend> EventSink for Device<B> {
        fn emit(&mut self, event: Event) -> io::Result<()> {
            let backend = self
                .backend
                .as_mut()
                .ok_or_else(|| io::Error::other("uinput device invalidated"))?;
            // All-zero native input_event is valid (timestamp is ignored by
            // uinput). Initialize in place so padding remains zero; no ABI sizes.
            let mut storage = mem::MaybeUninit::<libc::input_event>::zeroed();
            let pointer = storage.as_mut_ptr();
            // SAFETY: storage is aligned and fully initialized to valid zero
            // values; only integer fields are modified. The byte view borrows
            // this live storage until the synchronous write completes.
            unsafe {
                match event {
                    Event::Key { code, pressed } => {
                        (*pointer).type_ = 1;
                        (*pointer).code = code;
                        (*pointer).value = i32::from(pressed);
                    }
                    Event::Sync => {} // EV_SYN/SYN_REPORT, value 0.
                }
                let bytes = std::slice::from_raw_parts(
                    pointer.cast::<u8>(),
                    mem::size_of::<libc::input_event>(),
                );
                write_records(backend, bytes, mem::size_of::<libc::input_event>())
            }
        }
        fn invalidate(&mut self) {
            self.teardown();
        }
        fn is_valid(&self) -> bool {
            self.backend.is_some()
        }
    }

    fn write_records(writer: &mut impl Write, mut bytes: &[u8], stride: usize) -> io::Result<()> {
        while !bytes.is_empty() {
            match writer.write(bytes) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(count) if count > bytes.len() || count % stride != 0 => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "uinput write split an input_event",
                    ));
                }
                Ok(count) => bytes = &bytes[count..],
            }
        }
        Ok(())
    }

    pub(super) struct Native(File);
    impl Native {
        pub(super) fn open() -> io::Result<Self> {
            OpenOptions::new().write(true).custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK).open("/dev/uinput")
                .map(Self).map_err(|error| io::Error::new(error.kind(), format!("Cannot open /dev/uinput: {error}; Linux uinput access must be granted by the host administrator")))
        }
        fn ioctl(&self, request: libc::Ioctl, argument: usize) -> io::Result<()> {
            loop {
                // SAFETY: fd is owned/live; requests either take an integer,
                // no argument, or a pointer to live native uinput_setup below.
                // ioctl is synchronous and does not retain that pointer.
                let result = unsafe { libc::ioctl(self.0.as_raw_fd(), request, argument) };
                if result >= 0 {
                    return Ok(());
                }
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
    }
    impl Write for Native {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.write(bytes)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Backend for Native {
        fn command(&mut self, command: Command) -> io::Result<()> {
            let result = match command {
                Command::Event(code) => self.ioctl(EVBIT, code as usize),
                Command::Key(code) => self.ioctl(KEYBIT, code as usize),
                Command::Create => self.ioctl(CREATE, 0),
                Command::Destroy => self.ioctl(DESTROY, 0),
                Command::Setup => {
                    let mut storage = mem::MaybeUninit::<libc::uinput_setup>::zeroed();
                    let pointer = storage.as_mut_ptr();
                    // SAFETY: zero is valid for all native integer fields; the
                    // in-place writes preserve zero padding and NUL termination.
                    unsafe {
                        (*pointer).id.bustype = 0x03; // BUS_USB
                        (*pointer).id.version = 1;
                        for (slot, byte) in (*pointer)
                            .name
                            .iter_mut()
                            .zip(b"Stream Deck physical keyboard")
                        {
                            *slot = *byte as libc::c_char;
                        }
                    }
                    self.ioctl(SETUP, pointer as usize)
                }
                Command::Ready => {
                    // Bounded hotplug grace only, not proof a compositor has
                    // recognized or accepted the virtual keyboard.
                    thread::sleep(Duration::from_millis(200));
                    Ok(())
                }
            };
            result.map_err(|error| {
                io::Error::new(error.kind(), format!("uinput {command:?} failed: {error}"))
            })
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{cell::RefCell, collections::VecDeque, rc::Rc};
        #[derive(Default)]
        struct State {
            commands: Vec<Command>,
            fail: Option<usize>,
            writes: Vec<Vec<u8>>,
        }
        struct Fake(Rc<RefCell<State>>);
        impl Write for Fake {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0.borrow_mut().writes.push(bytes.to_vec());
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        impl Backend for Fake {
            fn command(&mut self, command: Command) -> io::Result<()> {
                let mut state = self.0.borrow_mut();
                let index = state.commands.len();
                state.commands.push(command);
                if state.fail == Some(index) {
                    Err(io::Error::other("setup failure"))
                } else {
                    Ok(())
                }
            }
        }
        #[test]
        fn capability_lifecycle_and_failure_teardown() {
            let state = Rc::new(RefCell::new(State::default()));
            let device = Device::new(Fake(state.clone())).unwrap();
            let commands = state.borrow().commands.clone();
            assert_eq!(commands[0], Command::Event(1));
            for (_, code) in KEYS {
                assert!(commands.contains(&Command::Key(*code)));
            }
            for modifier in [
                Modifier::Control,
                Modifier::Shift,
                Modifier::Alt,
                Modifier::Super,
            ] {
                assert!(commands.contains(&Command::Key(modifier.code())));
            }
            assert_eq!(
                &commands[commands.len() - 3..],
                &[Command::Setup, Command::Create, Command::Ready]
            );
            drop(device);
            assert_eq!(state.borrow().commands.last(), Some(&Command::Destroy));
            for phase in 0..commands.len() {
                let state = Rc::new(RefCell::new(State {
                    fail: Some(phase),
                    ..State::default()
                }));
                assert!(Device::new(Fake(state.clone())).is_err());
                assert_eq!(
                    state.borrow().commands.last(),
                    Some(&Command::Destroy),
                    "phase {phase}"
                );
            }
        }
        #[test]
        fn native_records_are_zeroed_and_invalidation_tears_down_once() {
            let state = Rc::new(RefCell::new(State::default()));
            let mut device = Device::new(Fake(state.clone())).unwrap();
            for event in [
                Event::Key {
                    code: 28,
                    pressed: true,
                },
                Event::Sync,
                Event::Key {
                    code: 28,
                    pressed: false,
                },
            ] {
                device.emit(event).unwrap();
            }
            let mut expected = vec![0; mem::size_of::<libc::input_event>()];
            let type_offset = mem::offset_of!(libc::input_event, type_);
            let code_offset = mem::offset_of!(libc::input_event, code);
            let value_offset = mem::offset_of!(libc::input_event, value);
            expected[type_offset..type_offset + 2].copy_from_slice(&1u16.to_ne_bytes());
            expected[code_offset..code_offset + 2].copy_from_slice(&28u16.to_ne_bytes());
            expected[value_offset..value_offset + 4].copy_from_slice(&1i32.to_ne_bytes());
            assert_eq!(state.borrow().writes[0], expected);
            assert_eq!(
                state.borrow().writes[1],
                vec![0; mem::size_of::<libc::input_event>()]
            );
            expected[value_offset..value_offset + 4].fill(0);
            assert_eq!(state.borrow().writes[2], expected);
            let destroy_index = state.borrow().commands.len();
            state.borrow_mut().fail = Some(destroy_index);
            device.invalidate();
            assert!(!device.is_valid());
            assert!(device.emit(Event::Sync).is_err());
            device.invalidate();
            drop(device);
            assert_eq!(
                state
                    .borrow()
                    .commands
                    .iter()
                    .filter(|command| **command == Command::Destroy)
                    .count(),
                1
            );
        }
        struct Writer {
            outcomes: VecDeque<io::Result<usize>>,
            lengths: Vec<usize>,
        }
        impl Write for Writer {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.lengths.push(bytes.len());
                self.outcomes.pop_front().unwrap_or(Ok(bytes.len()))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        #[test]
        fn interrupted_and_aligned_short_writes() {
            let stride = std::mem::size_of::<libc::input_event>();
            let bytes = vec![0; stride * 3];
            let mut writer = Writer {
                outcomes: VecDeque::from([
                    Err(io::ErrorKind::Interrupted.into()),
                    Ok(stride),
                    Ok(stride * 2),
                ]),
                lengths: vec![],
            };
            write_records(&mut writer, &bytes, stride).unwrap();
            assert_eq!(writer.lengths, vec![stride * 3, stride * 3, stride * 2]);
            for outcome in [
                Ok(0),
                Ok(stride - 1),
                Ok(bytes.len() + stride),
                Err(io::ErrorKind::PermissionDenied.into()),
            ] {
                let mut writer = Writer {
                    outcomes: VecDeque::from([outcome]),
                    lengths: vec![],
                };
                assert!(write_records(&mut writer, &bytes, stride).is_err());
                assert_eq!(writer.lengths.len(), 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).into()).collect()
    }
    fn down(code: u16) -> Event {
        Event::Key {
            code,
            pressed: true,
        }
    }
    fn up(code: u16) -> Event {
        Event::Key {
            code,
            pressed: false,
        }
    }
    #[derive(Default)]
    struct Mock {
        events: Vec<Event>,
        failures: Vec<usize>,
        invalid: bool,
    }
    impl EventSink for Mock {
        fn emit(&mut self, event: Event) -> io::Result<()> {
            let index = self.events.len();
            self.events.push(event);
            if self.failures.contains(&index) {
                Err(io::Error::other(format!("failure {index}")))
            } else {
                Ok(())
            }
        }
        fn invalidate(&mut self) {
            self.invalid = true;
        }
        fn is_valid(&self) -> bool {
            !self.invalid
        }
    }
    #[test]
    fn owner_is_lazy_persistent_and_replaces_invalid_devices() {
        let mut owner = Owner { device: None };
        let chord = parse_chord(&[], "D").unwrap();
        owner.send(&chord, || Ok(Mock::default())).unwrap();
        owner.send(&chord, || panic!("must reuse device")).unwrap();
        assert_eq!(owner.device.as_ref().unwrap().events.len(), 12);
        let mut owner = Owner {
            device: Some(Mock {
                failures: vec![1, 2],
                ..Mock::default()
            }),
        };
        assert!(owner
            .send(&chord, || panic!("already initialized"))
            .is_err());
        assert!(owner.device.is_none());
        owner.send(&chord, || Ok(Mock::default())).unwrap();
        let mut owner = Owner::<Mock> { device: None };
        assert!(owner
            .send(&chord, || Err(io::Error::other("unavailable")))
            .is_err());
        assert!(owner.device.is_none());
    }
    #[test]
    fn examples_aliases_and_physical_case() {
        let ctrl_d = parse_chord(&[], "CTRL + D").unwrap();
        assert_eq!(ctrl_d.modifiers(), &[Modifier::Control]);
        assert_eq!(ctrl_d.key().code(), 32);
        for alias in ["Super", "win", "WINDOWS", "meta"] {
            let chord = parse_chord(
                &strings(&["CONTROL", "CTRL", "SHIFT"]),
                &format!(" {alias} + return "),
            )
            .unwrap();
            assert_eq!(
                chord.modifiers(),
                &[Modifier::Control, Modifier::Shift, Modifier::Super]
            );
            assert_eq!(chord.key().code(), 28);
        }
        assert_eq!(
            parse_chord(&[], "SUPER + RETURN").unwrap(),
            parse_chord(&[], "Win + Enter").unwrap()
        );
        assert_eq!(
            parse_chord(&[], "D").unwrap(),
            parse_chord(&[], "d").unwrap()
        );
        assert!(parse_chord(&[], "D").unwrap().modifiers().is_empty());
        for (name, code) in KEYS {
            assert_eq!(parse_chord(&[], name).unwrap().key().code(), *code);
        }
    }
    #[test]
    fn invalid_chords_are_not_commands_or_text() {
        for text in [
            "",
            "+D",
            "CTRL+",
            "CTRL++D",
            "CTRL",
            "D+E",
            "é",
            "D;exec",
            "D,activewindow",
            "$(id)",
            "hello",
            "CTRL D",
        ] {
            assert!(parse_chord(&[], text).is_err(), "{text:?}");
        }
        assert!(parse_chord(&strings(&["Hyper"]), "D").is_err());
        for text in [
            "Tab", "Escape", "Space", "Left", "PageDown", "F12", "KPEnter", "VolumeUp", "Minus",
            "Slash",
        ] {
            assert!(parse_chord(&[], text).is_ok(), "{text}");
        }
    }
    #[test]
    fn synchronized_sequence() {
        let chord = parse_chord(&strings(&["SUPER", "CTRL"]), "D").unwrap();
        let mut sink = Mock::default();
        emit_chord(&mut sink, &chord).unwrap();
        assert_eq!(
            sink.events,
            vec![
                down(29),
                down(125),
                Event::Sync,
                down(32),
                Event::Sync,
                up(32),
                Event::Sync,
                up(125),
                up(29),
                Event::Sync
            ]
        );
        assert!(!sink.invalid);
    }
    #[test]
    fn every_phase_failure_releases_potential_keys() {
        let chord = parse_chord(&strings(&["CTRL", "SUPER"]), "D").unwrap();
        for failed in 0..10 {
            let mut sink = Mock {
                failures: vec![failed],
                ..Mock::default()
            };
            let error = emit_chord(&mut sink, &chord).unwrap_err();
            assert_eq!(error.to_string(), format!("failure {failed}"));
            assert_eq!(sink.events.last(), Some(&Event::Sync));
            let cleanup = &sink.events[failed + 1..];
            assert!(cleanup.contains(&up(29)), "phase {failed}: {cleanup:?}");
            if failed >= 1 {
                assert!(cleanup.contains(&up(125)));
            }
            if failed >= 3 {
                assert!(cleanup.contains(&up(32)));
            }
            assert!(!sink.invalid);
        }
    }
    #[test]
    fn cleanup_attempts_all_releases_and_preserves_original_error() {
        let chord = parse_chord(&strings(&["CTRL", "SUPER"]), "D").unwrap();
        let mut sink = Mock {
            failures: vec![4, 5, 6, 7, 8],
            ..Mock::default()
        };
        assert_eq!(
            emit_chord(&mut sink, &chord).unwrap_err().to_string(),
            "failure 4"
        );
        assert_eq!(&sink.events[5..], &[up(32), up(125), up(29), Event::Sync]);
        assert!(sink.invalid);
    }
}
