use super::*;
use gtk::glib::translate::IntoGlib;

impl FeatureEditor {
    pub(crate) fn wait_for_audio_outputs(&self) {
        self.audio_output.wait_until_loaded();
    }

    pub(crate) fn select_audio_rotation(&self) {
        self.audio_output.select_rotation();
    }

    pub(crate) fn assert_audio_output_choices(&self) {
        use sdeck_plus::desktop_actions::audio_output::AudioOutput;
        let selected = FeatureAction::Desktop(DesktopAction::SetAudioOutput {
            name: "speakers".into(),
        });
        self.populate(&selected);
        self.audio_output.set_outputs(vec![
            AudioOutput {
                name: "headphones".into(),
                description: "USB headphones".into(),
            },
            AudioOutput {
                name: "speakers".into(),
                description: "Monitor speakers".into(),
            },
        ]);
        assert_eq!(self.parse().unwrap(), selected);
        // A disconnected saved device remains selected rather than silently changing targets.
        self.audio_output.set_outputs(Vec::new());
        assert_eq!(self.parse().unwrap(), selected);
        self.select_audio_rotation();
        let rotation = FeatureAction::Desktop(DesktopAction::CycleAudioOutput);
        assert_eq!(self.parse().unwrap(), rotation);
        self.audio_output.set_outputs(vec![AudioOutput {
            name: "headphones".into(),
            description: "USB headphones".into(),
        }]);
        assert_eq!(
            self.parse().unwrap(),
            rotation,
            "refresh must retain rotation"
        );
        self.audio_output.select(Some("headphones"));
        assert_eq!(
            self.parse().unwrap(),
            FeatureAction::Desktop(DesktopAction::SetAudioOutput {
                name: "headphones".into(),
            })
        );
    }

    pub(crate) fn select_category(&self, category: u32) {
        self.reset();
        self.category.set_selected(category);
        self.refresh_choices();
    }

    pub(crate) fn assert_roundtrips(&self) {
        assert!(
            self.root.is_visible(),
            "extended action editor must be visible before checking its fields"
        );
        for action in examples() {
            self.populate(&action);
            assert_eq!(self.parse().unwrap(), action);
            let expected = match action {
                FeatureAction::Desktop(DesktopAction::Text { .. }) => {
                    vec![false, false, true, false, false, true]
                }
                FeatureAction::Desktop(DesktopAction::ShortcutToggle { .. }) => {
                    vec![true, true, false, false, false, false]
                }
                FeatureAction::Desktop(DesktopAction::MouseScroll { .. }) => {
                    vec![false, false, false, true, true, false]
                }
                FeatureAction::Obs(ObsAction::SourceVisibility { .. }) => {
                    vec![true, true, false, false, false, true]
                }
                FeatureAction::Desktop(
                    DesktopAction::Open { .. }
                    | DesktopAction::Website { .. }
                    | DesktopAction::CloseApplication { .. }
                    | DesktopAction::PlayAudio { .. },
                )
                | FeatureAction::Navigation(NavigationAction::OpenFolder(_))
                | FeatureAction::Obs(ObsAction::Scene(_) | ObsAction::ToggleMute(_)) => {
                    vec![true, false, false, false, false, false]
                }
                FeatureAction::Timer { .. }
                | FeatureAction::Navigation(NavigationAction::GoToPage(_))
                | FeatureAction::Device(
                    DeviceAction::Brightness(_) | DeviceAction::AdjustBrightness(_),
                ) => vec![false, false, false, true, false, false],
                _ => vec![false; 6],
            };
            let visible: Vec<_> = [
                &self.first_row,
                &self.second_row,
                &self.text_row,
                &self.number_row,
                &self.direction_row,
                &self.flag_row,
            ]
            .into_iter()
            .map(|row| row.is_visible())
            .collect();
            assert_eq!(visible, expected, "visible fields for {action:?}");
            assert_eq!(
                self.audio_output.root().is_visible(),
                matches!(
                    action,
                    FeatureAction::Desktop(
                        DesktopAction::SetAudioOutput { .. } | DesktopAction::CycleAudioOutput
                    )
                ),
            );
            assert_eq!(
                self.browse_file.is_visible(),
                matches!(self.choice().unwrap(), Choice::Open | Choice::PlayAudio)
            );
            assert_eq!(
                self.browse_folder.is_visible(),
                self.choice() == Ok(Choice::Open)
            );
            for (entry, label, row) in [
                (&self.first, &self.first_label, &self.first_row),
                (&self.second, &self.second_label, &self.second_row),
                (&self.number, &self.number_label, &self.number_row),
            ] {
                if row.is_visible() {
                    assert_eq!(label.mnemonic_widget().as_ref(), Some(entry.upcast_ref()));
                    assert!(!entry.placeholder_text().unwrap().is_empty());
                }
            }
        }
        self.populate(&FeatureAction::Timer { seconds: 5 });
        self.number.set_text("0");
        assert!(self.parse().is_err());
        self.number.set_text("not a number");
        assert!(self.parse().is_err());
        self.populate(&FeatureAction::Device(DeviceAction::Brightness(50)));
        self.number.set_text("101");
        assert!(self.parse().is_err());
        self.populate(&FeatureAction::Desktop(DesktopAction::Website {
            url: "https://example.com".into(),
        }));
        self.first.set_text("not a website");
        assert!(self.parse().is_err());
        self.reset();
        assert!(self.first.text().is_empty());
        assert!(self.second.text().is_empty());
        assert!(!self.flag.is_active());
        self.assert_picker_ownership();
    }

    fn assert_picker_ownership(&self) {
        let open = FeatureAction::Desktop(DesktopAction::Open {
            target: "/tmp/original.txt".into(),
        });
        self.populate(&open);
        let picker = self.create_picker(gtk::FileChooserAction::Open);
        assert!(picker.is_modal());
        assert_eq!(picker.action(), gtk::FileChooserAction::Open);
        let generation = self.generation.get();
        self.apply_picker_path(generation, std::path::Path::new("/tmp/chosen file.txt"));
        assert_eq!(self.first.text(), "/tmp/chosen file.txt");
        self.apply_picker_path(generation, std::path::Path::new("relative.txt"));
        assert_eq!(self.first.text(), "/tmp/chosen file.txt");
        picker.emit_by_name::<()>("response", &[&gtk::ResponseType::Cancel.into_glib()]);
        assert!(self.picker.borrow().is_none());
        assert_eq!(self.first.text(), "/tmp/chosen file.txt");

        for transition in 0..4 {
            self.populate(&open);
            let picker = self.create_picker(gtk::FileChooserAction::Open);
            let generation = self.generation.get();
            match transition {
                0 => self.reset(),
                1 => self.populate(&open),
                2 => self.action.set_selected(3),
                3 => self.category.set_selected(1),
                _ => unreachable!(),
            }
            let value = self.first.text();
            assert!(self.picker.borrow().is_none());
            self.apply_picker_path(generation, std::path::Path::new("/tmp/stale.txt"));
            picker.emit_by_name::<()>("response", &[&gtk::ResponseType::Accept.into_glib()]);
            assert_eq!(self.first.text(), value);
        }

        self.populate(&open);
        let folder = self.create_picker(gtk::FileChooserAction::SelectFolder);
        assert_eq!(folder.action(), gtk::FileChooserAction::SelectFolder);
        let old_generation = self.generation.get();
        let replacement = self.create_picker(gtk::FileChooserAction::Open);
        folder.emit_by_name::<()>("response", &[&gtk::ResponseType::Cancel.into_glib()]);
        assert_eq!(self.picker.borrow().as_ref(), Some(&replacement));
        self.apply_picker_path(old_generation, std::path::Path::new("/tmp/stale"));
        assert_eq!(self.parse().unwrap(), open);
        replacement.emit_by_name::<()>("response", &[&gtk::ResponseType::Cancel.into_glib()]);

        self.populate(&FeatureAction::Desktop(DesktopAction::PlayAudio {
            path: "/tmp/track.ogg".into(),
        }));
        let audio = self.create_picker(gtk::FileChooserAction::Open);
        assert_eq!(audio.filters().n_items(), 2);
        audio.emit_by_name::<()>("response", &[&gtk::ResponseType::Cancel.into_glib()]);
        self.show_direct_sound(true);
        assert!(!self.selectors.is_visible());
        assert!(self.browse_file.is_visible());
        let picker = self.create_picker(gtk::FileChooserAction::Open);
        let generation = self.generation.get();
        self.apply_picker_path(generation, std::path::Path::new("/tmp/recorded sound.MP3"));
        assert_eq!(
            self.parse().unwrap(),
            FeatureAction::Desktop(DesktopAction::PlayAudio {
                path: "/tmp/recorded sound.MP3".into()
            })
        );
        self.show_direct_sound(false);
        assert!(self.selectors.is_visible());
        assert!(self.picker.borrow().is_none());
        self.apply_picker_path(generation, std::path::Path::new("/tmp/stale.wav"));
        assert_eq!(self.first.text(), "/tmp/recorded sound.MP3");
        picker.emit_by_name::<()>("response", &[&gtk::ResponseType::Cancel.into_glib()]);
        self.reset();

        let editor = FeatureEditor::new();
        let weak = Rc::downgrade(&editor.0);
        let root = editor.root.clone();
        let picker = editor.create_picker(gtk::FileChooserAction::Open);
        drop(editor);
        assert!(
            weak.upgrade().is_none(),
            "signals must not retain the editor"
        );
        picker.emit_by_name::<()>("response", &[&gtk::ResponseType::Accept.into_glib()]);
        assert!(root.first_child().is_some());
    }
}

pub(crate) fn examples() -> Vec<FeatureAction> {
    let mut actions = vec![
        FeatureAction::Desktop(DesktopAction::ShortcutToggle {
            first: "CTRL+A".into(),
            second: "SUPER+D".into(),
        }),
        FeatureAction::Desktop(DesktopAction::Text {
            value: "  Text\n日本語  ".into(),
            enter: true,
        }),
        FeatureAction::Desktop(DesktopAction::Text {
            value: String::new(),
            enter: false,
        }),
        FeatureAction::Desktop(DesktopAction::Open {
            target: "/tmp/a file.txt".into(),
        }),
        FeatureAction::Desktop(DesktopAction::Website {
            url: "https://example.com/docs".into(),
        }),
        FeatureAction::Desktop(DesktopAction::CloseApplication {
            executable: "example".into(),
        }),
        FeatureAction::Desktop(DesktopAction::PlayAudio {
            path: "/tmp/a track.ogg".into(),
        }),
        FeatureAction::Desktop(DesktopAction::StopAudio),
        FeatureAction::Desktop(DesktopAction::CycleAudioOutput),
        FeatureAction::Desktop(DesktopAction::SetAudioOutput {
            name: "alsa_output.pci-0000_06_00.6.analog-stereo".into(),
        }),
        FeatureAction::Navigation(NavigationAction::NextPage),
        FeatureAction::Navigation(NavigationAction::PreviousPage),
        FeatureAction::Navigation(NavigationAction::GoToPage(2)),
        FeatureAction::Navigation(NavigationAction::OpenFolder("Studio".into())),
        FeatureAction::Navigation(NavigationAction::ParentFolder),
        FeatureAction::Timer { seconds: 90 },
        FeatureAction::CancelTimer,
        FeatureAction::Device(DeviceAction::Brightness(0)),
        FeatureAction::Device(DeviceAction::Brightness(100)),
        FeatureAction::Device(DeviceAction::AdjustBrightness(-5)),
        FeatureAction::Device(DeviceAction::AdjustBrightness(5)),
        FeatureAction::Device(DeviceAction::Sleep),
        FeatureAction::Device(DeviceAction::Wake),
        FeatureAction::Obs(ObsAction::Scene("Main scene".into())),
        FeatureAction::Obs(ObsAction::ToggleRecording),
        FeatureAction::Obs(ObsAction::ToggleStreaming),
        FeatureAction::Obs(ObsAction::StartRecording),
        FeatureAction::Obs(ObsAction::StopRecording),
        FeatureAction::Obs(ObsAction::PauseRecording),
        FeatureAction::Obs(ObsAction::ResumeRecording),
        FeatureAction::Obs(ObsAction::StartStreaming),
        FeatureAction::Obs(ObsAction::StopStreaming),
        FeatureAction::Obs(ObsAction::ToggleMute("Microphone".into())),
        FeatureAction::Obs(ObsAction::SourceVisibility {
            scene: "Main scene".into(),
            source: "Camera".into(),
            visible: true,
        }),
        FeatureAction::Obs(ObsAction::SourceVisibility {
            scene: "Main scene".into(),
            source: "Camera".into(),
            visible: false,
        }),
    ];
    for command in [
        MediaCommand::PlayPause,
        MediaCommand::Stop,
        MediaCommand::Next,
        MediaCommand::Previous,
        MediaCommand::VolumeUp,
        MediaCommand::VolumeDown,
        MediaCommand::Mute,
    ] {
        actions.push(FeatureAction::Desktop(DesktopAction::Media(command)));
    }
    for direction in [
        ScrollDirection::Up,
        ScrollDirection::Down,
        ScrollDirection::Left,
        ScrollDirection::Right,
    ] {
        actions.push(FeatureAction::Desktop(DesktopAction::MouseScroll {
            direction,
            ticks: 3,
        }));
    }
    actions
}
