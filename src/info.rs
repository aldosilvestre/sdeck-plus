//! Live information drawn over a key: sources, parsers and the reader thread.

use crate::{event_api::MonitorEvent, obs};
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
}

impl InfoSource {
    pub const ALL: [Self; 9] = [
        Self::Volume,
        Self::AudioDevice,
        Self::Brightness,
        Self::Clock,
        Self::Weather,
        Self::Cpu,
        Self::Memory,
        Self::Temperature,
        Self::Obs,
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
            Self::Volume => 1,
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
}

impl InfoValue {
    pub fn text(glyph: Glyph, headline: &str) -> Self {
        Self {
            glyph,
            level: None,
            headline: headline.to_owned(),
            detail: String::new(),
            inactive: false,
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
}

impl InfoReader {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            cpu: None,
            geocoded: None,
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

    #[test]
    fn source_ids_round_trip_and_location_is_validated() {
        for source in InfoSource::ALL {
            assert_eq!(InfoSource::from_id(source.id()), Some(source));
            assert!(source.unavailable().inactive);
        }
        assert_eq!(InfoSource::from_id("nope"), None);
        let root = std::env::temp_dir().join(format!("sdeck-info-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        assert_eq!(load_location(&root), None);
        assert!(save_location(&root, "  ").is_err());
        save_location(&root, " Madrid ").unwrap();
        assert_eq!(load_location(&root).as_deref(), Some("Madrid"));
        fs::remove_dir_all(root).unwrap();
    }
}
