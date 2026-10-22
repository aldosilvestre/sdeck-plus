//! Native, typed fields for the extended action family.
use gtk::{prelude::*, Box as GtkBox, CheckButton, Entry, Label, Orientation, TextView};
use sdeck_plus::{
    desktop_actions::{DesktopAction, MediaCommand, ScrollDirection},
    device_control::DeviceAction,
    features::{FeatureAction, TouchscreenAction},
    obs::ObsAction,
    organization::NavigationAction,
};
use std::{
    cell::{Cell, RefCell},
    ops::Deref,
    rc::Rc,
};

mod audio_output;
mod choice;
use choice::Choice;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::examples;

#[derive(Clone)]
pub(crate) struct FeatureEditor(Rc<EditorWidgets>);

pub(crate) struct EditorWidgets {
    pub(crate) root: GtkBox,
    selectors: GtkBox,
    category: gtk::DropDown,
    action: gtk::DropDown,
    action_model: gtk::StringList,
    first: Entry,
    second: Entry,
    text: TextView,
    number: Entry,
    direction: gtk::DropDown,
    flag: CheckButton,
    first_row: GtkBox,
    second_row: GtkBox,
    text_row: GtkBox,
    number_row: GtkBox,
    direction_row: GtkBox,
    flag_row: GtkBox,
    first_label: Label,
    second_label: Label,
    number_label: Label,
    description: Label,
    action_icon: gtk::Image,
    browse_file: gtk::Button,
    browse_folder: gtk::Button,
    generation: Cell<u64>,
    touchscreen: Cell<bool>,
    picker: RefCell<Option<gtk::FileChooserNative>>,
    audio_output: audio_output::OutputPicker,
}

impl Deref for FeatureEditor {
    type Target = EditorWidgets;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for EditorWidgets {
    fn drop(&mut self) {
        if let Some(picker) = self.picker.get_mut().take() {
            picker.destroy();
        }
    }
}

fn field(label: &str, widget: &impl IsA<gtk::Widget>) -> (GtkBox, Label) {
    let row = GtkBox::new(Orientation::Vertical, 4);
    let label = Label::new(Some(label));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_mnemonic_widget(Some(widget));
    widget
        .as_ref()
        .update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
    row.append(&label);
    row.append(widget);
    (row, label)
}

impl FeatureEditor {
    pub(crate) fn new() -> Self {
        let root = GtkBox::new(Orientation::Vertical, 12);
        let category =
            gtk::DropDown::from_strings(&["System", "Multimedia", "Organization", "Device", "OBS"]);
        let action_model = gtk::StringList::new(&[]);
        let action = gtk::DropDown::new(Some(action_model.clone()), gtk::Expression::NONE);
        let pickers = GtkBox::new(Orientation::Vertical, 12);
        pickers.append(&field("Category", &category).0);
        pickers.append(&field("Action", &action).0);
        root.append(&pickers);
        let guidance = GtkBox::new(Orientation::Horizontal, 8);
        let action_icon = gtk::Image::new();
        action_icon.set_pixel_size(20);
        action_icon.set_valign(gtk::Align::Start);
        let description = crate::ui::helper("");
        guidance.append(&action_icon);
        guidance.append(&description);
        root.append(&guidance);
        let first = Entry::builder().hexpand(true).width_chars(12).build();
        let second = Entry::builder().hexpand(true).width_chars(12).build();
        let text = TextView::new();
        text.set_wrap_mode(gtk::WrapMode::WordChar);
        let scroller = gtk::ScrolledWindow::builder()
            .min_content_height(60)
            .child(&text)
            .build();
        let number = Entry::builder().width_chars(8).build();
        let direction = gtk::DropDown::from_strings(&["Up", "Down", "Left", "Right"]);
        let flag = CheckButton::new();
        let (first_row, first_label) = field("", &first);
        let browse_file = crate::ui::button("document-open-symbolic", "Choose file...");
        let browse_folder = crate::ui::button("folder-open-symbolic", "Choose folder...");
        let browse = GtkBox::new(Orientation::Vertical, 8);
        browse_file.set_halign(gtk::Align::Start);
        browse_folder.set_halign(gtk::Align::Start);
        browse.append(&browse_file);
        browse.append(&browse_folder);
        first_row.append(&browse);
        let (second_row, second_label) = field("", &second);
        let (text_row, text_label) = field("Text", &scroller);
        text_label.set_mnemonic_widget(Some(&text));
        text.update_relation(&[gtk::accessible::Relation::LabelledBy(&[
            text_label.upcast_ref()
        ])]);
        text_row.append(&crate::ui::helper(
            "Enter the text to type, including any line breaks.",
        ));
        let (number_row, number_label) = field("", &number);
        let (direction_row, _) = field("Direction", &direction);
        let (flag_row, _) = field("Options", &flag);
        let audio_output = audio_output::OutputPicker::new();
        for row in [
            &first_row,
            &second_row,
            &text_row,
            &number_row,
            &direction_row,
            &flag_row,
        ] {
            root.append(row);
        }
        root.append(audio_output.root());
        let editor = Self(Rc::new(EditorWidgets {
            root,
            selectors: pickers,
            category,
            action,
            action_model,
            first,
            second,
            text,
            number,
            direction,
            flag,
            first_row,
            second_row,
            text_row,
            number_row,
            direction_row,
            flag_row,
            first_label,
            second_label,
            number_label,
            description,
            action_icon,
            browse_file,
            browse_folder,
            generation: Cell::new(0),
            touchscreen: Cell::new(false),
            picker: RefCell::new(None),
            audio_output,
        }));
        editor.refresh_choices();
        let weak = Rc::downgrade(&editor.0);
        editor.category.connect_selected_notify(move |_| {
            if let Some(editor) = weak.upgrade() {
                Self(editor).refresh_choices();
            }
        });
        let weak = Rc::downgrade(&editor.0);
        editor.action.connect_selected_notify(move |_| {
            if let Some(editor) = weak.upgrade() {
                Self(editor).update_fields();
            }
        });
        for (button, action) in [
            (&editor.browse_file, gtk::FileChooserAction::Open),
            (&editor.browse_folder, gtk::FileChooserAction::SelectFolder),
        ] {
            let weak = Rc::downgrade(&editor.0);
            button.connect_clicked(move |_| {
                if let Some(editor) = weak.upgrade() {
                    Self(editor).open_picker(action);
                }
            });
        }
        editor.reset();
        editor
    }

    fn choices(&self) -> impl Iterator<Item = Choice> {
        let category = self.category.selected();
        let touchscreen = self.touchscreen.get();
        Choice::ALL.into_iter().filter(move |choice| {
            choice.category() == category
                && (touchscreen
                    || !matches!(
                        choice,
                        Choice::NextTouchscreenPage | Choice::PreviousTouchscreenPage
                    ))
        })
    }

    fn choice(&self) -> Result<Choice, String> {
        let selected =
            usize::try_from(self.action.selected()).map_err(|error| error.to_string())?;
        self.choices()
            .nth(selected)
            .ok_or_else(|| "Select a feature action.".into())
    }

    pub(crate) fn set_touchscreen(&self, touchscreen: bool) {
        if self.touchscreen.replace(touchscreen) != touchscreen {
            self.refresh_choices();
        }
    }

    fn refresh_choices(&self) {
        let labels: Vec<_> = self.choices().map(Choice::label).collect();
        self.action_model
            .splice(0, self.action_model.n_items(), &labels);
        self.action.set_selected(0);
        self.update_fields();
    }

    fn invalidate_picker(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        if let Some(picker) = self.picker.borrow_mut().take() {
            picker.destroy();
        }
    }

    fn apply_picker_path(&self, generation: u64, path: &std::path::Path) {
        if self.generation.get() == generation && path.is_absolute() {
            self.first.set_text(&path.to_string_lossy());
        }
    }

    fn create_picker(&self, action: gtk::FileChooserAction) -> gtk::FileChooserNative {
        self.invalidate_picker();
        let parent = self
            .root
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok());
        let picker = gtk::FileChooserNative::new(
            Some(if action == gtk::FileChooserAction::SelectFolder {
                "Choose folder"
            } else {
                "Choose file"
            }),
            parent.as_ref(),
            action,
            Some("Choose"),
            Some("Cancel"),
        );
        picker.set_modal(true);
        if self.choice() == Ok(Choice::PlayAudio) {
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Audio files"));
            filter.add_mime_type("audio/*");
            for extension in [
                "wav", "mp3", "ogg", "flac", "m4a", "aac", "opus", "aiff", "wma",
            ] {
                filter.add_pattern(&format!("*.{extension}"));
                filter.add_pattern(&format!("*.{}", extension.to_uppercase()));
            }
            picker.add_filter(&filter);
            let all = gtk::FileFilter::new();
            all.set_name(Some("All files"));
            all.add_pattern("*");
            picker.add_filter(&all);
        }
        let generation = self.generation.get();
        let weak = Rc::downgrade(&self.0);
        picker.connect_response(move |picker, response| {
            if let Some(editor) = weak.upgrade() {
                let editor = Self(editor);
                if response == gtk::ResponseType::Accept {
                    if let Some(path) = picker.file().and_then(|file| file.path()) {
                        editor.apply_picker_path(generation, &path);
                    }
                }
                if editor.picker.borrow().as_ref() == Some(picker) {
                    editor.picker.borrow_mut().take();
                }
            }
            picker.destroy();
        });
        self.picker.replace(Some(picker.clone()));
        picker
    }

    fn open_picker(&self, action: gtk::FileChooserAction) {
        self.create_picker(action).show();
    }

    fn update_fields(&self) {
        self.invalidate_picker();
        self.browse_file.set_visible(false);
        self.browse_folder.set_visible(false);
        for row in [
            &self.first_row,
            &self.second_row,
            &self.text_row,
            &self.number_row,
            &self.direction_row,
            &self.flag_row,
        ] {
            row.set_visible(false);
        }
        let Ok(choice) = self.choice() else { return };
        self.audio_output
            .root()
            .set_visible(choice == Choice::SetAudioOutput);
        self.description.set_text(choice.description());
        self.action_icon.set_icon_name(Some(choice.icon()));
        self.first.set_placeholder_text(Some(match choice {
            Choice::ShortcutToggle => "CTRL+A",
            Choice::Open => "/home/user/Documents/report.pdf",
            Choice::Website => "https://example.com",
            Choice::Close => "obs",
            Choice::PlayAudio => "/home/user/Music/track.ogg",
            Choice::OpenFolder => "Studio",
            Choice::Scene | Choice::Visibility => "Main scene",
            Choice::ObsMute => "Microphone",
            _ => "",
        }));
        self.second
            .set_placeholder_text(Some(if choice == Choice::ShortcutToggle {
                "SUPER+D"
            } else {
                "Camera"
            }));
        self.number.set_placeholder_text(Some(match choice {
            Choice::Scroll => "3",
            Choice::GoToPage => "2",
            Choice::Timer => "90",
            Choice::Brightness => "50",
            Choice::AdjustBrightness => "-5",
            _ => "",
        }));
        self.browse_file
            .set_visible(matches!(choice, Choice::Open | Choice::PlayAudio));
        self.browse_folder.set_visible(choice == Choice::Open);
        let first = match choice {
            Choice::ShortcutToggle => Some("First shortcut"),
            Choice::Open => Some("File or folder path"),
            Choice::Website => Some("Website URL"),
            Choice::Close => Some("Executable name"),
            Choice::PlayAudio => Some("Audio file path"),
            Choice::OpenFolder => Some("Folder name"),
            Choice::Scene | Choice::Visibility => Some("Scene name"),
            Choice::ObsMute => Some("Input name"),
            _ => None,
        };
        if let Some(label) = first {
            self.first_label.set_text(label);
            self.first_row.set_visible(true);
        }
        match choice {
            Choice::ShortcutToggle => {
                self.second_label.set_text("Second shortcut");
                self.second_row.set_visible(true);
            }
            Choice::Visibility => {
                self.second_label.set_text("Source name");
                self.second_row.set_visible(true);
                self.flag.set_label(Some("Visible"));
                self.flag_row.set_visible(true);
            }
            Choice::Text => {
                self.text_row.set_visible(true);
                self.flag.set_label(Some("Press Enter after text"));
                self.flag_row.set_visible(true);
            }
            _ => {}
        }
        let numeric = match choice {
            Choice::Scroll => Some("Ticks (1–65535)"),
            Choice::GoToPage => Some("Page number (starts at 1)"),
            Choice::Timer => Some("Duration in seconds (1–86400)"),
            Choice::Brightness => Some("Brightness percent (0–100)"),
            Choice::AdjustBrightness => Some("Adjustment percent (signed)"),
            _ => None,
        };
        if let Some(label) = numeric {
            self.number_label.set_text(label);
            self.number_row.set_visible(true);
        }
        self.direction_row.set_visible(choice == Choice::Scroll);
    }

    pub(crate) fn reset(&self) {
        self.invalidate_picker();
        self.audio_output.select(None);
        self.first.set_text("");
        self.second.set_text("");
        self.text.buffer().set_text("");
        self.number.set_text("1");
        self.direction.set_selected(0);
        self.flag.set_active(false);
        self.category.set_selected(0);
        self.action.set_selected(0);
        self.update_fields();
    }

    pub(crate) fn show_direct_sound(&self, direct: bool) {
        if direct && self.choice() != Ok(Choice::PlayAudio) {
            self.populate(&FeatureAction::Desktop(DesktopAction::PlayAudio {
                path: std::path::PathBuf::new(),
            }));
        }
        if self.selectors.is_visible() == direct {
            self.invalidate_picker();
        }
        self.selectors.set_visible(!direct);
    }

    pub(crate) fn parse(&self) -> Result<FeatureAction, String> {
        let first = self.first.text().to_string();
        let second = self.second.text().to_string();
        let numeric = self.number.text();
        let invalid_number =
            |error: &dyn std::fmt::Display| format!("{}: {error}", self.number_label.text());
        let action = match self.choice()? {
            Choice::ShortcutToggle => {
                FeatureAction::Desktop(DesktopAction::ShortcutToggle { first, second })
            }
            Choice::Text => {
                let buffer = self.text.buffer();
                FeatureAction::Desktop(DesktopAction::Text {
                    value: buffer
                        .text(&buffer.start_iter(), &buffer.end_iter(), false)
                        .to_string(),
                    enter: self.flag.is_active(),
                })
            }
            Choice::Open => FeatureAction::Desktop(DesktopAction::Open { target: first }),
            Choice::Website => FeatureAction::Desktop(DesktopAction::Website { url: first }),
            Choice::Close => {
                FeatureAction::Desktop(DesktopAction::CloseApplication { executable: first })
            }
            Choice::PlayAudio => {
                FeatureAction::Desktop(DesktopAction::PlayAudio { path: first.into() })
            }
            Choice::StopAudio => FeatureAction::Desktop(DesktopAction::StopAudio),
            Choice::Scroll => FeatureAction::Desktop(DesktopAction::MouseScroll {
                direction: match self.direction.selected() {
                    0 => ScrollDirection::Up,
                    1 => ScrollDirection::Down,
                    2 => ScrollDirection::Left,
                    3 => ScrollDirection::Right,
                    _ => return Err("Select a scroll direction.".into()),
                },
                ticks: numeric.parse().map_err(|error| invalid_number(&error))?,
            }),
            Choice::PlayPause => {
                FeatureAction::Desktop(DesktopAction::Media(MediaCommand::PlayPause))
            }
            Choice::Stop => FeatureAction::Desktop(DesktopAction::Media(MediaCommand::Stop)),
            Choice::Next => FeatureAction::Desktop(DesktopAction::Media(MediaCommand::Next)),
            Choice::Previous => {
                FeatureAction::Desktop(DesktopAction::Media(MediaCommand::Previous))
            }
            Choice::VolumeUp => {
                FeatureAction::Desktop(DesktopAction::Media(MediaCommand::VolumeUp))
            }
            Choice::VolumeDown => {
                FeatureAction::Desktop(DesktopAction::Media(MediaCommand::VolumeDown))
            }
            Choice::Mute => FeatureAction::Desktop(DesktopAction::Media(MediaCommand::Mute)),
            Choice::SetAudioOutput => FeatureAction::Desktop(self.audio_output.action()),
            Choice::NextPage => FeatureAction::Navigation(NavigationAction::NextPage),
            Choice::PreviousPage => FeatureAction::Navigation(NavigationAction::PreviousPage),
            Choice::NextTouchscreenPage => FeatureAction::Touchscreen(TouchscreenAction::NextPage),
            Choice::PreviousTouchscreenPage => {
                FeatureAction::Touchscreen(TouchscreenAction::PreviousPage)
            }
            Choice::GoToPage => FeatureAction::Navigation(NavigationAction::GoToPage(
                numeric.parse().map_err(|error| invalid_number(&error))?,
            )),
            Choice::OpenFolder => FeatureAction::Navigation(NavigationAction::OpenFolder(first)),
            Choice::ParentFolder => FeatureAction::Navigation(NavigationAction::ParentFolder),
            Choice::Timer => FeatureAction::Timer {
                seconds: numeric.parse().map_err(|error| invalid_number(&error))?,
            },
            Choice::CancelTimer => FeatureAction::CancelTimer,
            Choice::Brightness => FeatureAction::Device(DeviceAction::Brightness(
                numeric.parse().map_err(|error| invalid_number(&error))?,
            )),
            Choice::AdjustBrightness => FeatureAction::Device(DeviceAction::AdjustBrightness(
                numeric.parse().map_err(|error| invalid_number(&error))?,
            )),
            Choice::Sleep => FeatureAction::Device(DeviceAction::Sleep),
            Choice::Wake => FeatureAction::Device(DeviceAction::Wake),
            Choice::Scene => FeatureAction::Obs(ObsAction::Scene(first)),
            Choice::Recording => FeatureAction::Obs(ObsAction::ToggleRecording),
            Choice::Streaming => FeatureAction::Obs(ObsAction::ToggleStreaming),
            Choice::StartRecording => FeatureAction::Obs(ObsAction::StartRecording),
            Choice::StopRecording => FeatureAction::Obs(ObsAction::StopRecording),
            Choice::PauseRecording => FeatureAction::Obs(ObsAction::PauseRecording),
            Choice::ResumeRecording => FeatureAction::Obs(ObsAction::ResumeRecording),
            Choice::StartStreaming => FeatureAction::Obs(ObsAction::StartStreaming),
            Choice::StopStreaming => FeatureAction::Obs(ObsAction::StopStreaming),
            Choice::ObsMute => FeatureAction::Obs(ObsAction::ToggleMute(first)),
            Choice::Visibility => FeatureAction::Obs(ObsAction::SourceVisibility {
                scene: first,
                source: second,
                visible: self.flag.is_active(),
            }),
        };
        action.validate().map_err(|error| error.to_string())?;
        Ok(action)
    }

    pub(crate) fn populate(&self, action: &FeatureAction) {
        self.reset();
        let choice = match action {
            FeatureAction::Desktop(action) => match action {
                DesktopAction::ShortcutToggle { first, second } => {
                    self.first.set_text(first);
                    self.second.set_text(second);
                    Choice::ShortcutToggle
                }
                DesktopAction::Text { value, enter } => {
                    self.text.buffer().set_text(value);
                    self.flag.set_active(*enter);
                    Choice::Text
                }
                DesktopAction::Open { target } => {
                    self.first.set_text(target);
                    Choice::Open
                }
                DesktopAction::Website { url } => {
                    self.first.set_text(url);
                    Choice::Website
                }
                DesktopAction::CloseApplication { executable } => {
                    self.first.set_text(executable);
                    Choice::Close
                }
                DesktopAction::PlayAudio { path } => {
                    self.first.set_text(&path.to_string_lossy());
                    Choice::PlayAudio
                }
                DesktopAction::StopAudio => Choice::StopAudio,
                DesktopAction::SetAudioOutput { name } => {
                    self.audio_output.select(Some(name));
                    Choice::SetAudioOutput
                }
                DesktopAction::CycleAudioOutput => {
                    self.audio_output.select(None);
                    Choice::SetAudioOutput
                }
                DesktopAction::Media(command) => match command {
                    MediaCommand::PlayPause => Choice::PlayPause,
                    MediaCommand::Stop => Choice::Stop,
                    MediaCommand::Next => Choice::Next,
                    MediaCommand::Previous => Choice::Previous,
                    MediaCommand::VolumeUp => Choice::VolumeUp,
                    MediaCommand::VolumeDown => Choice::VolumeDown,
                    MediaCommand::Mute => Choice::Mute,
                },
                DesktopAction::MouseScroll { direction, ticks } => {
                    self.number.set_text(&ticks.to_string());
                    self.direction.set_selected(match direction {
                        ScrollDirection::Up => 0,
                        ScrollDirection::Down => 1,
                        ScrollDirection::Left => 2,
                        ScrollDirection::Right => 3,
                    });
                    Choice::Scroll
                }
            },
            FeatureAction::Navigation(action) => match action {
                NavigationAction::NextPage => Choice::NextPage,
                NavigationAction::PreviousPage => Choice::PreviousPage,
                NavigationAction::GoToPage(page) => {
                    self.number.set_text(&page.to_string());
                    Choice::GoToPage
                }
                NavigationAction::OpenFolder(folder) => {
                    self.first.set_text(folder);
                    Choice::OpenFolder
                }
                NavigationAction::ParentFolder => Choice::ParentFolder,
            },
            FeatureAction::Touchscreen(TouchscreenAction::NextPage) => Choice::NextTouchscreenPage,
            FeatureAction::Touchscreen(TouchscreenAction::PreviousPage) => {
                Choice::PreviousTouchscreenPage
            }
            FeatureAction::Device(action) => match action {
                DeviceAction::Brightness(value) => {
                    self.number.set_text(&value.to_string());
                    Choice::Brightness
                }
                DeviceAction::AdjustBrightness(value) => {
                    self.number.set_text(&value.to_string());
                    Choice::AdjustBrightness
                }
                DeviceAction::Sleep => Choice::Sleep,
                DeviceAction::Wake => Choice::Wake,
            },
            FeatureAction::Obs(action) => match action {
                ObsAction::Scene(scene) => {
                    self.first.set_text(scene);
                    Choice::Scene
                }
                ObsAction::ToggleRecording => Choice::Recording,
                ObsAction::ToggleStreaming => Choice::Streaming,
                ObsAction::StartRecording => Choice::StartRecording,
                ObsAction::StopRecording => Choice::StopRecording,
                ObsAction::PauseRecording => Choice::PauseRecording,
                ObsAction::ResumeRecording => Choice::ResumeRecording,
                ObsAction::StartStreaming => Choice::StartStreaming,
                ObsAction::StopStreaming => Choice::StopStreaming,
                ObsAction::ToggleMute(input) => {
                    self.first.set_text(input);
                    Choice::ObsMute
                }
                ObsAction::SourceVisibility {
                    scene,
                    source,
                    visible,
                } => {
                    self.first.set_text(scene);
                    self.second.set_text(source);
                    self.flag.set_active(*visible);
                    Choice::Visibility
                }
            },
            FeatureAction::Timer { seconds } => {
                self.number.set_text(&seconds.to_string());
                Choice::Timer
            }
            FeatureAction::CancelTimer => Choice::CancelTimer,
        };
        self.category.set_selected(choice.category());
        if let Some(index) = self.choices().position(|value| value == choice) {
            if let Ok(index) = u32::try_from(index) {
                self.action.set_selected(index);
            }
        }
        self.update_fields();
    }
}
