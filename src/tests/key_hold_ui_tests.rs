use super::*;
use sdeck_plus::{features::FeatureAction, info::InfoSource, layers::PageReturn};

pub(super) struct HoldFixture {
    pub root: std::path::PathBuf,
    pub layers: Rc<RefCell<LayerStore>>,
    pub artwork: ArtworkUi,
    pub hardware: mpsc::Receiver<ArtworkCommand>,
    pub requests: mpsc::Receiver<LayerCommand>,
    pub widgets: Option<MonitorWidgets>,
    pub runtime: feature_runtime::FeatureRuntime,
    window: ApplicationWindow,
}

impl HoldFixture {
    pub fn new(tag: &str) -> Self {
        let (root, store, images, _) = artwork_fixture(tag);
        let layers = Rc::new(RefCell::new(store));
        let (output, hardware) = mpsc::channel();
        let artwork = ArtworkUi {
            store: Rc::new(RefCell::new(Some(images))),
            layers: layers.clone(),
            pictures: Rc::default(),
            output,
            last_layer: Rc::default(),
            chooser_owners: Rc::default(),
            selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
            text_editor: Rc::default(),
            info: Rc::default(),
        };
        let (commands, requests) = mpsc::channel();
        let window = ApplicationWindow::builder().build();
        let widgets = build_widgets(window.clone(), layers.clone(), artwork.clone(), commands);
        window.set_child(Some(&widgets.root));
        window.present();
        wait_for_frame(&window);
        let runtime = widgets.feature_runtime.clone();
        runtime.set_timer_sound_for_test(
            gtk::gio::SubprocessLauncher::new(gtk::gio::SubprocessFlags::NONE),
            vec!["/bin/true".into()],
        );
        Self {
            root,
            layers,
            artwork,
            hardware,
            requests,
            widgets: Some(widgets),
            runtime,
            window,
        }
    }
}
impl Drop for HoldFixture {
    fn drop(&mut self) {
        self.runtime.cancel_key_holds();
        self.runtime.cancel_timer();
        self.window.destroy();
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn hold_editor_saves_reloads_removes_and_undoes_independently() {
    run_gtk_test(|| {
        let fixture = HoldFixture::new("hold-ui");
        let widgets = fixture.widgets.as_ref().unwrap();
        let ui = &widgets.binding_ui;
        let mut buttons = Vec::new();
        collect_layout_buttons(widgets.root.upcast_ref(), &mut buttons);
        let click = |caption: &str| {
            buttons
                .iter()
                .find(|b| button_caption(b).as_deref() == Some(caption))
                .unwrap()
                .emit_clicked()
        };
        assert!(ui.hold_toggle.is_visible());
        assert!(!ui
            .event_picker
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .is_visible());
        assert!(fixture
            .layers
            .borrow()
            .binding("Base", &Trigger::KeyHold(1))
            .is_none());

        ui.populate_action(&Action::Feature(FeatureAction::Timer { seconds: 45 }));
        ui.info_toggle.set_active(true);
        ui.info_source.set_selected(
            InfoSource::ALL
                .iter()
                .position(|s| *s == InfoSource::Volume)
                .unwrap() as u32,
        );
        click("Save binding");
        assert!(matches!(
            fixture.requests.try_recv().unwrap(),
            LayerCommand::SaveBinding(..)
        ));
        assert!(!fixture.runtime.timer_active());
        ui.hold_toggle.set_active(true);
        assert!(!ui.info_toggle.is_visible());
        assert!(!ui.info_row.is_visible());
        assert!(!ui.info_toggle.is_active());
        assert!(
            fixture.requests.try_recv().is_err(),
            "toggling never saves or executes"
        );
        ui.populate_action(&Action::Feature(FeatureAction::CancelTimer));
        ui.return_toggle.set_active(true);
        ui.return_seconds.set_text("90");
        assert_eq!(
            ui.selected_layer_and_trigger(&fixture.layers).unwrap(),
            ("Base".into(), Trigger::KeyHold(1))
        );
        assert_eq!(
            ui.binding_from_widgets(&fixture.layers).unwrap().2,
            Action::Feature(FeatureAction::CancelTimer)
        );
        click("Save binding");
        assert_eq!(
            fixture
                .layers
                .borrow()
                .binding("Base", &Trigger::KeyHold(1)),
            Some(Action::Feature(FeatureAction::CancelTimer))
        );
        let LayerCommand::SaveBinding(snapshot, _) = fixture.requests.try_recv().unwrap() else {
            panic!("expected save")
        };
        snapshot.persist().unwrap();
        let loaded = LayerStore::open_at(snapshot.root()).unwrap();
        assert_eq!(
            loaded.binding("Base", &Trigger::KeyHold(1)),
            Some(Action::Feature(FeatureAction::CancelTimer))
        );
        assert_eq!(
            loaded.page_return("Base", &Trigger::KeyHold(1)),
            Some(PageReturn {
                layer: "Base".into(),
                seconds: 90
            })
        );
        assert_eq!(
            loaded.info_source("Base", &Trigger::KeyPress(1)),
            Some(InfoSource::Volume)
        );
        assert!(!fixture.runtime.timer_active());
        assert!(!fixture.runtime.page_return_active());
        ui.hold_toggle.set_active(false);
        assert_eq!(
            ui.binding_from_widgets(&fixture.layers).unwrap().2,
            Action::Feature(FeatureAction::Timer { seconds: 45 })
        );
        assert!(!ui.return_toggle.is_active());
        assert!(ui.info_toggle.is_active());
        ui.hold_toggle.set_active(true);
        assert_eq!(
            ui.binding_from_widgets(&fixture.layers).unwrap().2,
            Action::Feature(FeatureAction::CancelTimer)
        );
        assert_eq!(ui.return_seconds.text(), "90");
        click("Remove binding");
        assert!(fixture
            .layers
            .borrow()
            .binding("Base", &Trigger::KeyHold(1))
            .is_none());
        assert!(fixture
            .layers
            .borrow()
            .binding("Base", &Trigger::KeyPress(1))
            .is_some());
        ui.undo_remove.emit_clicked();
        assert_eq!(
            fixture
                .layers
                .borrow()
                .page_return("Base", &Trigger::KeyHold(1)),
            Some(PageReturn {
                layer: "Base".into(),
                seconds: 90
            })
        );
        assert_eq!(
            fixture
                .layers
                .borrow()
                .info_source("Base", &Trigger::KeyPress(1)),
            Some(InfoSource::Volume)
        );
        key_button(&widgets.key_labels[1]).emit_clicked();
        assert!(
            !ui.hold_toggle.is_active(),
            "new key resets mode without loading recursively"
        );
        ui.select_control(ControlKind::Encoder, Some(1), true)
            .unwrap();
        assert!(!ui.hold_toggle.is_visible());
        ui.select_control(ControlKind::Touch, Some(1), true)
            .unwrap();
        assert!(!ui.hold_toggle.is_visible());
    });
}
