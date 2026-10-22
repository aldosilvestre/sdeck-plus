#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    ShortcutToggle,
    Text,
    Open,
    Website,
    Close,
    Scroll,
    PlayPause,
    Stop,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    Mute,
    SetAudioOutput,
    PlayAudio,
    StopAudio,
    NextPage,
    PreviousPage,
    NextTouchscreenPage,
    PreviousTouchscreenPage,
    GoToPage,
    OpenFolder,
    ParentFolder,
    Timer,
    CancelTimer,
    Brightness,
    AdjustBrightness,
    Sleep,
    Wake,
    Scene,
    Recording,
    Streaming,
    ObsMute,
    Visibility,
    StartRecording,
    StopRecording,
    PauseRecording,
    ResumeRecording,
    StartStreaming,
    StopStreaming,
}

impl Choice {
    pub(super) fn icon(self) -> &'static str {
        match self.category() {
            0 => "preferences-system-symbolic",
            1 => "audio-x-generic-symbolic",
            2 => "folder-symbolic",
            3 => "preferences-desktop-display-symbolic",
            _ => "camera-video-symbolic",
        }
    }

    pub(super) fn description(self) -> &'static str {
        match self {
            Self::ShortcutToggle => "Alternate between these shortcuts on successive presses. Combine modifiers and a key with +.",
            Self::Text => "Type this text into the focused application. Line breaks and spaces are preserved.",
            Self::Open => "Open a local file or folder with its default application. Browse or enter an absolute path.",
            Self::Website => "Open this address in your default browser. Include https:// or http://.",
            Self::Close => "Close processes matching this executable name exactly. Use the basename, not a path or window title.",
            Self::Scroll => "Scroll the pointer in the selected direction by this many ticks.",
            Self::PlayPause => "Pause or resume playback in the active media player.",
            Self::Stop => "Stop playback in the active media player.",
            Self::Next => "Skip to the next track in the active media player.",
            Self::Previous => "Return to the previous track in the active media player.",
            Self::VolumeUp => "Increase the system output volume.",
            Self::VolumeDown => "Decrease the system output volume.",
            Self::Mute => "Toggle the system output mute state.",
            Self::SetAudioOutput => "Choose a fixed output or Rotate to advance through connected devices on each press. Choosing or saving does not change audio.",
            Self::PlayAudio => "Play WAV, MP3, FLAC, OGG, M4A or another audio format supported by mpv. Choose a local file or enter its absolute path. Choosing or saving does not play it.",
            Self::StopAudio => "Stop audio started by a Play audio file action.",
            Self::NextPage => "Show the next page in the current profile folder.",
            Self::PreviousPage => "Show the previous page in the current profile folder.",
            Self::NextTouchscreenPage => "Show the next touchscreen page in this profile without changing keypad pages or folders. Assign to Swipe left.",
            Self::PreviousTouchscreenPage => "Show the previous touchscreen page in this profile without changing keypad pages or folders. Assign to Swipe right.",
            Self::GoToPage => "Show this page in the current profile folder. The first page is 1.",
            Self::OpenFolder => "Open an existing profile folder by name. This is not a filesystem path.",
            Self::ParentFolder => "Return to the parent profile folder.",
            Self::Timer => "Start a countdown in seconds, from 1 to 86400.",
            Self::CancelTimer => "Cancel the running countdown.",
            Self::Brightness => "Set the device brightness from 0% to 100%.",
            Self::AdjustBrightness => "Change brightness by a signed percentage, from -100 to 100. Negative values dim the device.",
            Self::Sleep => "Put the device display to sleep.",
            Self::Wake => "Wake the device display.",
            Self::Scene => "Switch OBS to an existing scene. Match its name exactly.",
            Self::Recording => "Start or stop OBS recording when this control is activated.",
            Self::Streaming => "Start or stop OBS streaming when this control is activated.",
            Self::ObsMute => "Toggle mute for an OBS input. Match the input name exactly.",
            Self::Visibility => "Set a source's visibility in an OBS scene. Match both names exactly; clear Visible to hide it.",
            Self::StartRecording => "Start OBS recording. OBS must have a recording output configured.",
            Self::StopRecording => "Stop OBS recording and save the recording file.",
            Self::PauseRecording => "Pause the current OBS recording without ending it. The recording output must support pausing.",
            Self::ResumeRecording => "Resume the paused OBS recording.",
            Self::StartStreaming => "Start OBS streaming using the service and output settings configured in OBS.",
            Self::StopStreaming => "Stop the current OBS stream.",
        }
    }

    pub(super) const ALL: [Self; 40] = [
        Self::ShortcutToggle,
        Self::Text,
        Self::Open,
        Self::Website,
        Self::Close,
        Self::Scroll,
        Self::PlayPause,
        Self::Stop,
        Self::Next,
        Self::Previous,
        Self::VolumeUp,
        Self::VolumeDown,
        Self::Mute,
        Self::SetAudioOutput,
        Self::PlayAudio,
        Self::StopAudio,
        Self::NextPage,
        Self::PreviousPage,
        Self::NextTouchscreenPage,
        Self::PreviousTouchscreenPage,
        Self::GoToPage,
        Self::OpenFolder,
        Self::ParentFolder,
        Self::Timer,
        Self::CancelTimer,
        Self::Brightness,
        Self::AdjustBrightness,
        Self::Sleep,
        Self::Wake,
        Self::Scene,
        Self::Recording,
        Self::Streaming,
        Self::ObsMute,
        Self::Visibility,
        Self::StartRecording,
        Self::StopRecording,
        Self::PauseRecording,
        Self::ResumeRecording,
        Self::StartStreaming,
        Self::StopStreaming,
    ];

    pub(super) fn category(self) -> u32 {
        match self {
            Self::ShortcutToggle
            | Self::Text
            | Self::Open
            | Self::Website
            | Self::Close
            | Self::Scroll => 0,
            Self::PlayPause
            | Self::Stop
            | Self::Next
            | Self::Previous
            | Self::VolumeUp
            | Self::VolumeDown
            | Self::Mute
            | Self::SetAudioOutput
            | Self::PlayAudio
            | Self::StopAudio => 1,
            Self::NextPage
            | Self::NextTouchscreenPage
            | Self::PreviousTouchscreenPage
            | Self::PreviousPage
            | Self::GoToPage
            | Self::OpenFolder
            | Self::ParentFolder => 2,
            Self::Brightness
            | Self::AdjustBrightness
            | Self::Sleep
            | Self::Wake
            | Self::Timer
            | Self::CancelTimer => 3,
            Self::Scene
            | Self::Recording
            | Self::Streaming
            | Self::ObsMute
            | Self::Visibility
            | Self::StartRecording
            | Self::StopRecording
            | Self::PauseRecording
            | Self::ResumeRecording
            | Self::StartStreaming
            | Self::StopStreaming => 4,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::ShortcutToggle => "Alternate shortcuts",
            Self::Text => "Type text",
            Self::Open => "Open file or folder",
            Self::Website => "Open website",
            Self::Close => "Close application",
            Self::Scroll => "Scroll mouse",
            Self::PlayPause => "Play / pause",
            Self::Stop => "Stop playback",
            Self::Next => "Next track",
            Self::Previous => "Previous track",
            Self::VolumeUp => "Volume up",
            Self::VolumeDown => "Volume down",
            Self::Mute => "Mute",
            Self::SetAudioOutput => "Change audio output",
            Self::PlayAudio => "Play audio file",
            Self::StopAudio => "Stop audio file",
            Self::NextPage => "Next page",
            Self::PreviousPage => "Previous page",
            Self::NextTouchscreenPage => "Next touchscreen page",
            Self::PreviousTouchscreenPage => "Previous touchscreen page",
            Self::GoToPage => "Go to page",
            Self::OpenFolder => "Open folder",
            Self::ParentFolder => "Parent folder",
            Self::Timer => "Start timer",
            Self::CancelTimer => "Cancel timer",
            Self::Brightness => "Set brightness",
            Self::AdjustBrightness => "Adjust brightness",
            Self::Sleep => "Sleep device",
            Self::Wake => "Wake device",
            Self::Scene => "Switch scene",
            Self::Recording => "Toggle recording",
            Self::Streaming => "Toggle streaming",
            Self::ObsMute => "Toggle input mute",
            Self::Visibility => "Set source visibility",
            Self::StartRecording => "Start recording",
            Self::StopRecording => "Stop recording",
            Self::PauseRecording => "Pause recording",
            Self::ResumeRecording => "Resume recording",
            Self::StartStreaming => "Start streaming",
            Self::StopStreaming => "Stop streaming",
        }
    }
}
