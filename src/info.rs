//! Live information drawn over a key: sources, parsers and the reader thread.

use crate::{
    desktop_actions::{self, PlaybackStatus, PlayerState},
    event_api::MonitorEvent,
    obs,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Sender},
    thread,
    time::{Duration, Instant},
};

mod app_icon;
pub use app_icon::{resolve as resolve_app_icon, AppIcon, AppIcons, IconDirs};

const SETTINGS: &str = "info.json";
const ELLIPSIS: &str = "\u{2026}";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InfoSource {
    Volume,
    AudioDevice,
    Brightness,
    Clock,
    Weather,
    Cpu,
    Memory,
    Temperature,
    Obs,
    NowPlaying,
}

impl InfoSource {
    pub const ALL: [Self; 10] = [
        Self::Volume,
        Self::AudioDevice,
        Self::Brightness,
        Self::Clock,
        Self::Weather,
        Self::Cpu,
        Self::Memory,
        Self::Temperature,
        Self::Obs,
        Self::NowPlaying,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Volume => "volume",
            Self::AudioDevice => "audio-device",
            Self::Brightness => "brightness",
            Self::Clock => "clock",
            Self::Weather => "weather",
            Self::Cpu => "cpu",
            Self::Memory => "memory",
            Self::Temperature => "temperature",
            Self::Obs => "obs",
            Self::NowPlaying => "now-playing",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|source| source.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Volume => "Volume and mute",
            Self::AudioDevice => "Audio output device",
            Self::Brightness => "Stream Deck brightness",
            Self::Clock => "Time and date",
            Self::Weather => "Weather",
            Self::Cpu => "CPU usage",
            Self::Memory => "Memory usage",
            Self::Temperature => "System temperature",
            Self::Obs => "OBS status",
            Self::NowPlaying => "Now playing",
        }
    }

    /// Icon used before the first reading and when a reading fails.
    pub fn glyph(self) -> Glyph {
        match self {
            Self::Volume | Self::AudioDevice => Glyph::Speaker,
            Self::Brightness => Glyph::Sun,
            Self::Clock => Glyph::None,
            Self::Weather => Glyph::Cloud,
            Self::Cpu => Glyph::Chip,
            Self::Memory => Glyph::Memory,
            Self::Temperature => Glyph::Thermometer,
            Self::Obs => Glyph::ObsIdle,
            Self::NowPlaying => Glyph::MediaIdle,
        }
    }

    /// Shown until the first reading arrives.
    pub fn placeholder(self) -> InfoValue {
        InfoValue::text(self.glyph(), ELLIPSIS)
    }

    /// Shown when a reading fails.
    pub fn unavailable(self) -> InfoValue {
        InfoValue {
            inactive: true,
            ..InfoValue::text(self.glyph(), "--")
        }
    }

    fn interval(self) -> Duration {
        Duration::from_secs(match self {
            Self::Volume | Self::NowPlaying => 1,
            Self::Cpu => 2,
            Self::AudioDevice => 3,
            Self::Clock | Self::Memory | Self::Temperature | Self::Obs => 5,
            Self::Weather => 600,
            Self::Brightness => 3_600,
        })
    }

    /// Brightness is known to the application itself; the rest are read by the worker.
    pub fn is_polled(self) -> bool {
        self != Self::Brightness
    }

    /// Cheap local readings that a button press can change right away.
    fn follows_actions(self) -> bool {
        matches!(self, Self::Volume | Self::AudioDevice)
    }
}

/// Picture drawn on a key for an information source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    None,
    Speaker,
    SpeakerMuted,
    Headphones,
    Sun,
    Cloud,
    PartlyCloudy,
    Rain,
    Snow,
    Storm,
    Fog,
    Chip,
    Memory,
    Thermometer,
    Record,
    Live,
    ObsIdle,
    /// Media playing.
    Play,
    /// Media paused.
    Pause,
    /// No media playing.
    MediaIdle,
}

/// One reading: an icon, an optional 0-100 bar and up to two text parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InfoValue {
    pub glyph: Glyph,
    /// Filled share of the bar; `None` draws no bar.
    pub level: Option<u8>,
    /// Large text, e.g. `45%` or `21:41`.
    pub headline: String,
    /// Small text below the headline; may span several lines.
    pub detail: String,
    /// Greys the picture out, e.g. while muted or unavailable.
    pub inactive: bool,
    /// Now playing: the playing application's icon, drawn instead of the
    /// glyph with a play or pause badge. Other sources leave it `None`.
    pub app_icon: Option<AppIcon>,
}

impl InfoValue {
    pub fn text(glyph: Glyph, headline: &str) -> Self {
        Self {
            glyph,
            level: None,
            headline: headline.to_owned(),
            detail: String::new(),
            inactive: false,
            app_icon: None,
        }
    }

    pub fn gauge(glyph: Glyph, level: u8, headline: String) -> Self {
        Self {
            level: Some(level.min(100)),
            headline,
            ..Self::text(glyph, "")
        }
    }
}

fn percent(fraction: f64) -> Option<u32> {
    format!("{:.0}", (fraction * 100.0).clamp(0.0, 999.0))
        .parse()
        .ok()
}

/// Greedy word wrap for narrow keys; overflow ends in an ellipsis.
pub fn wrap_words(text: &str, width: usize, max_lines: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        let word: String = word.chars().take(width).collect();
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= width => {
                line.push(' ');
                line.push_str(&word);
            }
            _ => lines.push(word),
        }
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            last.push_str(ELLIPSIS);
        }
    }
    lines.join("\n")
}

/// Parses `wpctl get-volume` output such as `Volume: 0.45 [MUTED]`.
pub fn parse_volume(output: &str) -> Option<InfoValue> {
    let rest = output.trim().strip_prefix("Volume:")?.trim();
    let (value, flags) = rest.split_once(' ').unwrap_or((rest, ""));
    let shown = percent(value.parse().ok()?)?;
    let muted = flags.contains("[MUTED]");
    Some(InfoValue {
        inactive: muted,
        glyph: if muted {
            Glyph::SpeakerMuted
        } else {
            Glyph::Speaker
        },
        ..InfoValue::gauge(
            Glyph::Speaker,
            u8::try_from(shown.min(100)).ok()?,
            format!("{shown}%"),
        )
    })
}

/// Parses `wpctl inspect` output for the device description.
pub fn parse_audio_device(output: &str) -> Option<InfoValue> {
    output.lines().find_map(|line| {
        let line = line.trim().trim_start_matches('*').trim();
        let name = line
            .strip_prefix("node.description = \"")?
            .strip_suffix('"')?;
        let lower = name.to_lowercase();
        let glyph = if ["headphone", "headset", "buds", "airpods"]
            .iter()
            .any(|word| lower.contains(word))
        {
            Glyph::Headphones
        } else {
            Glyph::Speaker
        };
        Some(InfoValue {
            detail: wrap_words(name, 14, 3),
            ..InfoValue::text(glyph, "")
        })
    })
}

/// Returns (total, idle) jiffies from the aggregate line of /proc/stat.
pub fn parse_cpu_times(stat: &str) -> Option<(u64, u64)> {
    let fields: Vec<u64> = stat
        .lines()
        .next()?
        .strip_prefix("cpu ")?
        .split_whitespace()
        .take(8)
        .map(|field| field.parse().ok())
        .collect::<Option<_>>()?;
    if fields.len() < 5 {
        return None;
    }
    Some((fields.iter().sum(), fields[3] + fields[4]))
}

pub fn format_cpu(previous: (u64, u64), current: (u64, u64)) -> Option<InfoValue> {
    let total = current
        .0
        .checked_sub(previous.0)
        .filter(|total| *total > 0)?;
    let idle = current.1.saturating_sub(previous.1);
    let busy = u8::try_from(total.saturating_sub(idle) * 100 / total).ok()?;
    Some(InfoValue::gauge(Glyph::Chip, busy, format!("{busy}%")))
}

pub fn parse_memory(meminfo: &str) -> Option<InfoValue> {
    let kilobytes = |name: &str| -> Option<u64> {
        meminfo
            .lines()
            .find_map(|line| line.strip_prefix(name))?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    };
    let total = kilobytes("MemTotal:").filter(|total| *total > 0)?;
    let used = total.checked_sub(kilobytes("MemAvailable:")?)?;
    let used = u8::try_from(used * 100 / total).ok()?;
    Some(InfoValue::gauge(Glyph::Memory, used, format!("{used}%")))
}

/// Hottest plausible sensor, from readings in millidegrees Celsius.
pub fn format_temperature(readings: &[i64]) -> Option<InfoValue> {
    let hottest = readings
        .iter()
        .copied()
        .filter(|value| (1_000..150_000).contains(value))
        .max()?;
    let degrees = hottest / 1_000;
    Some(InfoValue::gauge(
        Glyph::Thermometer,
        u8::try_from(degrees.clamp(0, 100)).ok()?,
        format!("{degrees}\u{b0}C"),
    ))
}

fn read_temperatures() -> Vec<i64> {
    let mut readings = Vec::new();
    for (directory, file) in [
        ("/sys/class/thermal", "temp"),
        ("/sys/class/hwmon", "temp1_input"),
    ] {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            if let Ok(text) = fs::read_to_string(entry.path().join(file)) {
                readings.extend(text.trim().parse::<i64>());
            }
        }
    }
    readings
}

pub fn weather_label(code: u32) -> &'static str {
    match code {
        0 => "Clear",
        1 | 2 => "Partly cloudy",
        3 => "Cloudy",
        45 | 48 => "Fog",
        51..=57 => "Drizzle",
        61..=67 => "Rain",
        71..=77 => "Snow",
        80..=82 => "Showers",
        85 | 86 => "Snow showers",
        95..=99 => "Storm",
        _ => "Weather",
    }
}

fn weather_glyph(code: u32) -> Glyph {
    match code {
        0 => Glyph::Sun,
        1 | 2 => Glyph::PartlyCloudy,
        45 | 48 => Glyph::Fog,
        51..=67 | 80..=82 => Glyph::Rain,
        71..=77 | 85 | 86 => Glyph::Snow,
        95..=99 => Glyph::Storm,
        _ => Glyph::Cloud,
    }
}

/// Parses an Open-Meteo `current` forecast response.
pub fn parse_weather(json: &str) -> Option<InfoValue> {
    let value: Value = serde_json::from_str(json).ok()?;
    let current = &value["current"];
    let temperature = current["temperature_2m"].as_f64()?;
    let code = u32::try_from(current["weather_code"].as_u64()?).ok()?;
    Some(InfoValue {
        detail: wrap_words(weather_label(code), 14, 2),
        ..InfoValue::text(weather_glyph(code), &format!("{temperature:.0}\u{b0}C"))
    })
}

pub fn parse_geocode(json: &str) -> Option<(f64, f64)> {
    let value: Value = serde_json::from_str(json).ok()?;
    let first = &value["results"][0];
    Some((first["latitude"].as_f64()?, first["longitude"].as_f64()?))
}

/// Accepts `latitude,longitude`; anything else is a place name to geocode.
pub fn parse_coordinates(text: &str) -> Option<(f64, f64)> {
    let (latitude, longitude) = text.split_once(',')?;
    let latitude: f64 = latitude.trim().parse().ok()?;
    let longitude: f64 = longitude.trim().parse().ok()?;
    ((-90.0..=90.0).contains(&latitude) && (-180.0..=180.0).contains(&longitude))
        .then_some((latitude, longitude))
}

pub fn format_obs(status: &obs::ObsStatus) -> InfoValue {
    let (glyph, headline) = match (status.recording, status.streaming) {
        (true, true) => (Glyph::Record, "REC LIVE"),
        (true, false) => (Glyph::Record, "REC"),
        (false, true) => (Glyph::Live, "LIVE"),
        (false, false) => (Glyph::ObsIdle, "OBS"),
    };
    InfoValue {
        detail: wrap_words(&status.current_scene, 14, 2),
        ..InfoValue::text(glyph, headline)
    }
}

/// Chooses the MPRIS player the Now playing source shows: a Playing player
/// wins (the first by bus name when several play); otherwise the Paused player
/// that was seen playing most recently (the first by bus name among players
/// never seen playing); otherwise none. Stopped players count as nothing
/// playing. It remembers only players still on the bus.
#[derive(Debug, Default)]
pub struct NowPlaying {
    /// Bus name -> the poll in which the player was last seen Playing.
    last_playing: BTreeMap<String, u64>,
    poll: u64,
    icons: AppIcons,
}

impl NowPlaying {
    pub fn choose<'a>(&mut self, players: &'a [PlayerState]) -> Option<&'a PlayerState> {
        self.poll += 1;
        self.last_playing
            .retain(|name, _| players.iter().any(|player| player.bus_name == *name));
        for player in players {
            if player.status == PlaybackStatus::Playing {
                self.last_playing.insert(player.bus_name.clone(), self.poll);
            }
        }
        let with = |status| players.iter().filter(move |player| player.status == status);
        with(PlaybackStatus::Playing)
            .min_by(|a, b| a.bus_name.cmp(&b.bus_name))
            .or_else(|| {
                with(PlaybackStatus::Paused).max_by_key(|player| {
                    (
                        self.last_playing
                            .get(&player.bus_name)
                            .copied()
                            .unwrap_or(0),
                        std::cmp::Reverse(&player.bus_name),
                    )
                })
            })
    }

    /// Resolve application icons in `dirs` instead of the user's.
    pub fn with_icon_dirs(dirs: IconDirs) -> Self {
        Self {
            icons: AppIcons::with_dirs(dirs),
            ..Self::default()
        }
    }

    /// The reading for one poll of the bus; `None` when the bus failed. The
    /// chosen player's icon comes from its `DesktopEntry`, else its
    /// `Identity` as an entry name; nothing playing has none.
    pub fn value(&mut self, players: io::Result<Vec<PlayerState>>) -> Option<InfoValue> {
        let players = players.ok()?;
        let player = self.choose(&players);
        let mut value = format_now_playing(player);
        if let Some(player) = player.filter(|_| value.glyph != Glyph::MediaIdle) {
            // Entry lookups also try lower case ("Firefox" -> firefox).
            let hint = player
                .identity
                .as_deref()
                .map(|identity| identity.replace(' ', "-"));
            value.app_icon = self.icons.lookup(
                player
                    .desktop_entry
                    .as_deref()
                    .into_iter()
                    .chain(hint.as_deref()),
            );
        }
        Some(value)
    }
}

/// Playing: play glyph, title and artist. Paused: the same with the pause
/// glyph, greyed out. No player: a grey idle glyph over a plain "Nothing
/// playing" caption (not greyed, so it stays legible), like OBS idle.
pub fn format_now_playing(player: Option<&PlayerState>) -> InfoValue {
    let Some(player) = player.filter(|player| player.status != PlaybackStatus::Stopped) else {
        return InfoValue {
            detail: wrap_words("Nothing playing", 14, 2),
            ..InfoValue::text(Glyph::MediaIdle, "")
        };
    };
    let paused = player.status == PlaybackStatus::Paused;
    let headline = player
        .title
        .as_deref()
        .or(player.identity.as_deref())
        .unwrap_or("Unknown title");
    InfoValue {
        detail: wrap_words(player.artist.as_deref().unwrap_or(""), 14, 2),
        inactive: paused,
        ..InfoValue::text(if paused { Glyph::Pause } else { Glyph::Play }, headline)
    }
}

pub fn load_location(root: &Path) -> Option<String> {
    let text = fs::read_to_string(root.join(SETTINGS)).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value["location"]
        .as_str()
        .map(|location| location.trim().to_owned())
        .filter(|location| !location.is_empty())
}

/// # Errors
/// Rejects empty, oversized or control-character locations and write failures.
pub fn save_location(root: &Path, location: &str) -> io::Result<()> {
    let location = location.trim();
    if location.is_empty() || location.len() > 100 || location.chars().any(char::is_control) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "weather location must be 1-100 printable characters",
        ));
    }
    let temporary = root.join("info.json.tmp");
    fs::write(
        &temporary,
        serde_json::json!({ "location": location }).to_string(),
    )?;
    fs::rename(temporary, root.join(SETTINGS))
}

fn run(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())?
}

fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// Blocking reader for every polled source; keep it off the GTK thread.
pub struct InfoReader {
    root: PathBuf,
    cpu: Option<(u64, u64)>,
    geocoded: Option<(String, (f64, f64))>,
    now_playing: NowPlaying,
}

impl InfoReader {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            cpu: None,
            geocoded: None,
            now_playing: NowPlaying::default(),
        }
    }

    /// A failed reading shows `--` instead of an error.
    pub fn read(&mut self, source: InfoSource) -> InfoValue {
        self.try_read(source)
            .unwrap_or_else(|| source.unavailable())
    }

    fn try_read(&mut self, source: InfoSource) -> Option<InfoValue> {
        match source {
            InfoSource::Volume => {
                parse_volume(&run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])?)
            }
            InfoSource::AudioDevice => {
                parse_audio_device(&run("wpctl", &["inspect", "@DEFAULT_AUDIO_SINK@"])?)
            }
            InfoSource::Brightness => None,
            InfoSource::Clock => {
                let output = run("date", &["+%H:%M%n%a %d"])?;
                let (time, date) = output.trim().split_once('\n')?;
                Some(InfoValue {
                    detail: date.to_owned(),
                    ..InfoValue::text(Glyph::None, time)
                })
            }
            InfoSource::Weather => self.read_weather(),
            InfoSource::Cpu => {
                let current = parse_cpu_times(&fs::read_to_string("/proc/stat").ok()?)?;
                let previous = self.cpu.replace(current).unwrap_or((0, 0));
                format_cpu(previous, current)
            }
            InfoSource::Memory => parse_memory(&fs::read_to_string("/proc/meminfo").ok()?),
            InfoSource::Temperature => format_temperature(&read_temperatures()),
            InfoSource::Obs => {
                let config = obs::ObsConfig::load().ok()?;
                obs::Client::new(config)
                    .probe()
                    .ok()
                    .map(|status| format_obs(&status))
            }
            InfoSource::NowPlaying => self.now_playing.value(desktop_actions::read_players()),
        }
    }

    fn read_weather(&mut self) -> Option<InfoValue> {
        let Some(location) = load_location(&self.root) else {
            return Some(InfoValue {
                detail: "set a location".into(),
                ..InfoValue::text(Glyph::Cloud, "--")
            });
        };
        let (latitude, longitude) = match parse_coordinates(&location) {
            Some(found) => found,
            None => match &self.geocoded {
                Some((name, found)) if *name == location => *found,
                _ => {
                    let url = format!(
                        "https://geocoding-api.open-meteo.com/v1/search?count=1&name={}",
                        percent_encode(&location)
                    );
                    let found = parse_geocode(&run("curl", &["-fsS", "--max-time", "8", &url])?)?;
                    self.geocoded = Some((location, found));
                    found
                }
            },
        };
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={latitude}&longitude={longitude}&current=temperature_2m,weather_code"
        );
        parse_weather(&run("curl", &["-fsS", "--max-time", "8", &url])?)
    }
}

/// Starts the reader thread. It reports through `output` and stops when the
/// returned sender (the set of sources to watch) is dropped. Sending the set
/// that is already watched re-reads the sources an action can change at once,
/// instead of waiting for their polling interval.
///
/// # Errors
/// Returns the thread spawn error.
pub fn spawn_worker(
    root: PathBuf,
    output: Sender<MonitorEvent>,
) -> io::Result<Sender<BTreeSet<InfoSource>>> {
    let (commands, receiver) = mpsc::channel::<BTreeSet<InfoSource>>();
    thread::Builder::new()
        .name("info-reader".into())
        .spawn(move || {
            let mut reader = InfoReader::new(root);
            let mut watched = BTreeSet::new();
            let mut last: BTreeMap<InfoSource, Instant> = BTreeMap::new();
            loop {
                match receiver.recv_timeout(Duration::from_millis(500)) {
                    Ok(set) => {
                        last.retain(|source, _| set.contains(source));
                        if set == watched {
                            last.retain(|source, _| !source.follows_actions());
                        }
                        watched = set;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
                for source in &watched {
                    if last
                        .get(source)
                        .is_some_and(|at| at.elapsed() < source.interval())
                    {
                        continue;
                    }
                    last.insert(*source, Instant::now());
                    let value = reader.read(*source);
                    if output
                        .send(MonitorEvent::Info {
                            source: *source,
                            value,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            }
        })?;
    Ok(commands)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(value: &InfoValue) -> (Glyph, Option<u8>, &str, &str, bool) {
        (
            value.glyph,
            value.level,
            &value.headline,
            &value.detail,
            value.inactive,
        )
    }

    #[test]
    fn volume_reports_level_and_swaps_the_speaker_when_muted() {
        let on = parse_volume("Volume: 0.45\n").unwrap();
        assert_eq!(shown(&on), (Glyph::Speaker, Some(45), "45%", "", false));
        let muted = parse_volume("Volume: 1.20 [MUTED]").unwrap();
        assert_eq!(
            shown(&muted),
            (Glyph::SpeakerMuted, Some(100), "120%", "", true)
        );
        assert_eq!(parse_volume("not a volume"), None);
    }

    #[test]
    fn audio_device_uses_the_description_and_picks_headphones() {
        let output =
            "  * node.description = \"Family 17h/19h HD Audio Controller Analog Stereo\"\n";
        let value = parse_audio_device(output).unwrap();
        assert_eq!(value.glyph, Glyph::Speaker);
        assert_eq!(value.detail, "Family 17h/19h\nHD Audio\nController\u{2026}");
        let headset = parse_audio_device("node.description = \"WH-1000 Headset\"").unwrap();
        assert_eq!(headset.glyph, Glyph::Headphones);
        assert_eq!(parse_audio_device("node.name = \"x\""), None);
    }

    #[test]
    fn cpu_usage_is_computed_from_two_samples() {
        let first = parse_cpu_times("cpu  100 0 100 700 100 0 0 0 0 0\ncpu0 1 1 1 1").unwrap();
        let second = parse_cpu_times("cpu  200 0 200 750 150 0 0 0 0 0").unwrap();
        assert_eq!(first, (1000, 800));
        let value = format_cpu(first, second).unwrap();
        assert_eq!(shown(&value), (Glyph::Chip, Some(66), "66%", "", false));
        assert_eq!(format_cpu(second, second), None);
        assert_eq!(parse_cpu_times("intr 1 2 3"), None);
    }

    #[test]
    fn memory_and_temperature_are_gauges() {
        let meminfo = "MemTotal:       16000000 kB\nMemFree: 1 kB\nMemAvailable:    4000000 kB\n";
        let memory = parse_memory(meminfo).unwrap();
        assert_eq!(shown(&memory), (Glyph::Memory, Some(75), "75%", "", false));
        assert_eq!(parse_memory("MemTotal: 0 kB"), None);
        let hot = format_temperature(&[0, 41_000, 54_700, 900_000]).unwrap();
        assert_eq!(
            shown(&hot),
            (Glyph::Thermometer, Some(54), "54\u{b0}C", "", false)
        );
        assert_eq!(format_temperature(&[]), None);
    }

    #[test]
    fn weather_and_locations_are_parsed_at_the_boundary() {
        let forecast = r#"{"current":{"temperature_2m":21.6,"weather_code":2}}"#;
        let value = parse_weather(forecast).unwrap();
        assert_eq!(
            shown(&value),
            (
                Glyph::PartlyCloudy,
                None,
                "22\u{b0}C",
                "Partly cloudy",
                false
            )
        );
        assert_eq!(
            parse_weather(r#"{"current":{"temperature_2m":3,"weather_code":95}}"#)
                .unwrap()
                .glyph,
            Glyph::Storm
        );
        assert_eq!(parse_weather("{}"), None);
        let found = r#"{"results":[{"latitude":40.4,"longitude":-3.7}]}"#;
        assert_eq!(parse_geocode(found), Some((40.4, -3.7)));
        assert_eq!(parse_geocode(r#"{"results":[]}"#), None);
        assert_eq!(parse_coordinates("40.4, -3.7"), Some((40.4, -3.7)));
        assert_eq!(parse_coordinates("91,0"), None);
        assert_eq!(parse_coordinates("Madrid"), None);
        assert_eq!(percent_encode("S\u{e3}o Paulo"), "S%C3%A3o%20Paulo");
    }

    #[test]
    fn obs_status_shows_recording_streaming_and_scene() {
        let mut status = obs::ObsStatus {
            scenes: vec![],
            current_scene: "Main scene".into(),
            recording: false,
            streaming: false,
        };
        let idle = format_obs(&status);
        assert_eq!(
            (idle.glyph, idle.headline.as_str()),
            (Glyph::ObsIdle, "OBS")
        );
        status.recording = true;
        status.streaming = true;
        let live = format_obs(&status);
        assert_eq!(
            (live.glyph, live.headline.as_str()),
            (Glyph::Record, "REC LIVE")
        );
        assert_eq!(live.detail, "Main scene");
    }

    /// A selector that never finds icons on the host.
    fn quiet() -> NowPlaying {
        NowPlaying::with_icon_dirs(IconDirs {
            data: vec![],
            themes: vec![],
        })
    }

    #[test]
    fn now_playing_carries_the_chosen_players_application_icon() {
        let root = std::env::temp_dir().join(format!("sdeck-now-icon-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let icons = root.join("icons/hicolor/48x48/apps");
        fs::create_dir_all(&icons).unwrap();
        fs::create_dir_all(root.join("applications")).unwrap();
        let image = gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, true, 8, 48, 48)
            .unwrap();
        image.fill(0xff0000ff);
        image.savev(icons.join("tune.png"), "png", &[]).unwrap();
        fs::write(
            root.join("applications/org.example.Tune.desktop"),
            "[Desktop Entry]\nType=Application\nName=Tune\nExec=true\nIcon=tune\n",
        )
        .unwrap();
        let mut now = NowPlaying::with_icon_dirs(IconDirs {
            data: vec![root.clone()],
            themes: vec!["hicolor".into()],
        });
        let tune = PlayerState {
            desktop_entry: Some("org.example.Tune".into()),
            ..player("tune", PlaybackStatus::Playing, Some("A"))
        };
        let playing = now.value(Ok(vec![tune.clone()])).unwrap();
        let icon = playing.app_icon.expect("the playing application's icon");
        assert_eq!(icon.path(), icons.join("tune.png"));
        let paused = PlayerState {
            status: PlaybackStatus::Paused,
            ..tune.clone()
        };
        assert!(now.value(Ok(vec![paused])).unwrap().app_icon.is_some());
        // Identity is a hint when the player names no desktop entry.
        let hinted = PlayerState {
            desktop_entry: None,
            identity: Some("org.example.Tune".into()),
            ..tune.clone()
        };
        assert!(now.value(Ok(vec![hinted])).unwrap().app_icon.is_some());
        // Nothing playing and unknown applications keep the glyph.
        let stopped = PlayerState {
            status: PlaybackStatus::Stopped,
            ..tune.clone()
        };
        assert_eq!(now.value(Ok(vec![stopped])).unwrap().app_icon, None);
        let unknown = PlayerState {
            desktop_entry: Some("unknown".into()),
            ..tune
        };
        assert_eq!(now.value(Ok(vec![unknown])).unwrap().app_icon, None);
        fs::remove_dir_all(root).unwrap();
    }

    fn player(name: &str, status: PlaybackStatus, title: Option<&str>) -> PlayerState {
        PlayerState {
            bus_name: format!("org.mpris.MediaPlayer2.{name}"),
            status,
            title: title.map(str::to_owned),
            artist: Some(format!("{name} band")),
            identity: None,
            desktop_entry: None,
        }
    }

    #[test]
    fn now_playing_shows_the_playing_title_and_artist() {
        let mut now = quiet();
        let players = [player(
            "mpv",
            PlaybackStatus::Playing,
            Some("Blue in Green"),
        )];
        let value = now.value(Ok(players.to_vec())).unwrap();
        assert_eq!(
            shown(&value),
            (Glyph::Play, None, "Blue in Green", "mpv band", false)
        );
    }

    #[test]
    fn now_playing_paused_is_inactive_with_the_pause_glyph() {
        let mut now = quiet();
        let players = vec![player("mpv", PlaybackStatus::Paused, Some("So What"))];
        let value = now.value(Ok(players)).unwrap();
        assert_eq!(
            shown(&value),
            (Glyph::Pause, None, "So What", "mpv band", true)
        );
    }

    #[test]
    fn now_playing_stopped_or_no_player_is_nothing_playing() {
        let mut now = quiet();
        let nothing = now.value(Ok(vec![])).unwrap();
        assert_eq!(
            shown(&nothing),
            (Glyph::MediaIdle, None, "", "Nothing\nplaying", false)
        );
        let stopped = vec![player("mpv", PlaybackStatus::Stopped, Some("Gone"))];
        assert_eq!(now.value(Ok(stopped)).unwrap(), nothing);
        assert_eq!(InfoSource::NowPlaying.glyph(), Glyph::MediaIdle);
    }

    #[test]
    fn now_playing_bus_errors_are_the_normal_unavailable_value() {
        let mut now = quiet();
        assert_eq!(now.value(Err(io::Error::other("no session bus"))), None);
        let shown_value = now
            .value(Err(io::Error::other("no session bus")))
            .unwrap_or_else(|| InfoSource::NowPlaying.unavailable());
        assert_eq!(
            shown(&shown_value),
            (Glyph::MediaIdle, None, "--", "", true)
        );
    }

    #[test]
    fn now_playing_prefers_playing_then_the_most_recently_playing_paused_one() {
        let mut now = quiet();
        let (a, b, c) = ("alpha", "beta", "gamma");
        // Several Playing players: the first by bus name, whatever the order.
        let both = vec![
            player(c, PlaybackStatus::Playing, Some("C")),
            player(a, PlaybackStatus::Paused, Some("A")),
            player(b, PlaybackStatus::Playing, Some("B")),
        ];
        assert_eq!(now.value(Ok(both)).unwrap().headline, "B");
        // Then gamma plays alone; beta pauses.
        let gamma = vec![
            player(a, PlaybackStatus::Paused, Some("A")),
            player(b, PlaybackStatus::Paused, Some("B")),
            player(c, PlaybackStatus::Playing, Some("C")),
        ];
        assert_eq!(now.value(Ok(gamma)).unwrap().headline, "C");
        // Everyone paused: the one seen playing most recently (gamma) wins,
        // not the first by name.
        let paused = vec![
            player(a, PlaybackStatus::Paused, Some("A")),
            player(b, PlaybackStatus::Paused, Some("B")),
            player(c, PlaybackStatus::Paused, Some("C")),
        ];
        let value = now.value(Ok(paused.clone())).unwrap();
        assert_eq!((value.glyph, value.headline.as_str()), (Glyph::Pause, "C"));
        // Without history, paused players that never played tie by bus name.
        let mut fresh = quiet();
        assert_eq!(fresh.value(Ok(paused)).unwrap().headline, "A");
    }

    #[test]
    fn now_playing_missing_metadata_falls_back_to_identity_then_unknown() {
        let mut now = quiet();
        let mut browser = player("firefox", PlaybackStatus::Playing, None);
        browser.artist = None;
        browser.identity = Some("Firefox".into());
        let value = now.value(Ok(vec![browser.clone()])).unwrap();
        assert_eq!(shown(&value), (Glyph::Play, None, "Firefox", "", false));
        browser.identity = None;
        let value = now.value(Ok(vec![browser])).unwrap();
        assert_eq!(value.headline, "Unknown title");
        let long = PlayerState {
            artist: Some("The Very Long Named Orchestra of Somewhere".into()),
            ..player("mpv", PlaybackStatus::Playing, Some("A"))
        };
        let value = now.value(Ok(vec![long])).unwrap();
        assert_eq!(value.detail, "The Very Long\nNamed\u{2026}");
    }

    #[test]
    fn source_ids_round_trip_and_location_is_validated() {
        for source in InfoSource::ALL {
            assert_eq!(InfoSource::from_id(source.id()), Some(source));
            assert!(source.unavailable().inactive);
        }
        assert_eq!(InfoSource::from_id("nope"), None);
        assert_eq!(
            InfoSource::from_id("now-playing"),
            Some(InfoSource::NowPlaying)
        );
        assert_eq!(InfoSource::NowPlaying.label(), "Now playing");
        assert!(InfoSource::NowPlaying.is_polled());
        let root = std::env::temp_dir().join(format!("sdeck-info-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        assert_eq!(load_location(&root), None);
        assert!(save_location(&root, "  ").is_err());
        save_location(&root, " Madrid ").unwrap();
        assert_eq!(load_location(&root).as_deref(), Some("Madrid"));
        fs::remove_dir_all(root).unwrap();
    }
}
