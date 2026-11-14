//! Plugin display values on keys and touch sections: which controls qualify, what wins when
//! two things want one control, and the live path from a query answer to the picture.
use super::*;
use crate::feature_runtime::plugin_values::Shown;
use sdeck_plus::organization::Organization;
use sdeck_plus::{
    features::FeatureAction,
    info::{Glyph, InfoSource, InfoValue},
    plugins::{PluginBinding, PluginRegistry, QueryOutcome},
};
use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;

/// A temporary plugins folder holding one `demo` plugin: `show` has a display, `plain` has not.
/// The script answers queries from `value.json` (or fails while `fail` exists) and logs every
/// request kind to `calls.log`.
struct ValuePlugin(std::path::PathBuf);

impl ValuePlugin {
    fn new(tag: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("sdeck-plugin-value-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("demo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"id":"demo","version":"1.0.0","protocol_version":1,"name":"Demo","executable":"run",
                "values":[{"id":"level","label":"Level"}],
                "actions":[
                  {"id":"show","name":"Show level","controls":["key","dial_press","dial_turn","touch"],
                   "display":"level",
                   "settings":[{"type":"integer","id":"zone","label":"Zone","min":0,"max":9,"default":1}]},
                  {"id":"plain","name":"Plain","controls":["key","dial_press","dial_turn","touch"],"settings":[]}]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("run"),
            r#"#!/bin/sh
read line
id=$(printf '%s' "$line" | sed 's/.*"request_id":"\([^"]*\)".*/\1/')
case "$line" in *'"type":"query"'*) kind=query ;; *) kind=invoke ;; esac
echo "$kind" >> calls.log
if [ "$kind" = invoke ]; then
  printf '{"protocol_version":1,"request_id":"%s","ok":true}\n' "$id"
elif [ -f fail ]; then
  printf '{"protocol_version":1,"request_id":"%s","ok":false,"error":"broken"}\n' "$id"
else
  printf '{"protocol_version":1,"request_id":"%s","ok":true,"value":%s}\n' "$id" "$(cat value.json)"
fi
"#,
        )
        .unwrap();
        std::fs::set_permissions(dir.join("run"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let plugin = Self(root);
        plugin.set_value(r#"{"headline":"70%","detail":"Brightness","level":70}"#);
        plugin
    }

    fn enable(&self) {
        PluginRegistry::open(&self.0)
            .set_enabled("demo", true)
            .unwrap();
    }

    fn set_value(&self, json: &str) {
        std::fs::write(self.0.join("demo/value.json"), json).unwrap();
    }

    fn fail(&self) {
        std::fs::write(self.0.join("demo/fail"), "").unwrap();
    }

    fn calls(&self, kind: &str) -> usize {
        std::fs::read_to_string(self.0.join("demo/calls.log"))
            .unwrap_or_default()
            .lines()
            .filter(|line| *line == kind)
            .count()
    }
}

impl Drop for ValuePlugin {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn show(zone: u64) -> PluginBinding {
    PluginBinding::new(
        "demo",
        "show",
        serde_json::json!({ "zone": zone })
            .as_object()
            .cloned()
            .unwrap(),
    )
}

fn plain() -> PluginBinding {
    PluginBinding::new("demo", "plain", sdeck_plus::plugins::Settings::new())
}

fn plugin_action(binding: PluginBinding) -> Action {
    Action::Feature(FeatureAction::Plugin(binding))
}

fn displays() -> BTreeSet<(String, String)> {
    BTreeSet::from([("demo".to_owned(), "show".to_owned())])
}

fn outcome(headline: &str) -> QueryOutcome {
    QueryOutcome {
        headline: headline.into(),
        detail: None,
        level: None,
    }
}

/// Every information picture queued for the device, flattened, in order.
fn composed(commands: &mpsc::Receiver<ArtworkCommand>) -> Vec<(ArtworkTarget, InfoValue)> {
    let mut found = Vec::new();
    for command in commands.try_iter() {
        let list = match command {
            ArtworkCommand::Replace(list) => list,
            other => vec![other],
        };
        for command in list {
            match command {
                ArtworkCommand::ComposeInfo { target, value, .. } => found.push((target, value)),
                ArtworkCommand::ComposeTouchscreenZone {
                    zone,
                    value: Some(value),
                    ..
                } => found.push((ArtworkTarget::EncoderZone(zone), value)),
                _ => {}
            }
        }
    }
    found
}

fn bare_ui(
    tag: &str,
) -> (
    std::path::PathBuf,
    ArtworkUi,
    mpsc::Receiver<ArtworkCommand>,
) {
    let (root, store, images, _) = artwork_fixture(tag);
    let layers = Rc::new(RefCell::new(store));
    let (output, hardware) = mpsc::channel();
    let artwork = ArtworkUi {
        store: Rc::new(RefCell::new(Some(images))),
        layers,
        pictures: Rc::default(),
        output,
        last_layer: Rc::default(),
        chooser_owners: Rc::default(),
        selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
        text_editor: Rc::default(),
        info: Rc::default(),
    };
    (root, artwork, hardware)
}

// ---- which controls qualify -----------------------------------------------------------------

#[test]
fn keys_and_zones_with_a_display_action_qualify_in_the_zone_trigger_order() {
    let (root, artwork, _hardware) = bare_ui("targets");
    {
        let mut layers = artwork.layers.borrow_mut();
        layers
            .bind("Base", Trigger::KeyPress(1), plugin_action(show(1)))
            .unwrap();
        layers
            .bind("Base", Trigger::KeyPress(2), plugin_action(plain()))
            .unwrap();
        layers
            .bind("Base", Trigger::KeyPress(3), Action::Command("true".into()))
            .unwrap();
        // Zone 2: a command on the press does not count; the first plugin binding in
        // press, clockwise, counter-clockwise, tap, hold order does.
        layers
            .bind(
                "Base",
                Trigger::EncoderPress(2),
                Action::Command("true".into()),
            )
            .unwrap();
        layers
            .bind("Base", Trigger::ZoneTap(2), plugin_action(show(2)))
            .unwrap();
        layers
            .bind("Base", Trigger::EncoderClockwise(2), plugin_action(show(3)))
            .unwrap();
        // Zone 3: the plugin binding earlier in the order has no display, so the next counts.
        layers
            .bind("Base", Trigger::EncoderPress(3), plugin_action(plain()))
            .unwrap();
        layers
            .bind("Base", Trigger::ZonePress(3), plugin_action(show(4)))
            .unwrap();
    }
    artwork.set_plugin_displays(displays());
    assert_eq!(
        artwork.plugin_targets("Base"),
        vec![
            (ArtworkTarget::Keypad(1), show(1)),
            (ArtworkTarget::EncoderZone(2), show(3)),
            (ArtworkTarget::EncoderZone(3), show(4)),
        ]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_plugin_that_is_not_enabled_or_not_offering_a_display_qualifies_nothing() {
    let (root, artwork, hardware) = bare_ui("disabled");
    artwork
        .layers
        .borrow_mut()
        .bind("Base", Trigger::KeyPress(1), plugin_action(show(1)))
        .unwrap();
    assert_eq!(artwork.plugin_targets("Base"), vec![]);
    assert!(artwork.shown_targets("Base").is_empty());
    artwork.update_plugin_value(
        "Base".into(),
        ArtworkTarget::Keypad(1),
        show(1),
        Ok(outcome("1")),
    );
    assert!(composed(&hardware).is_empty(), "nothing is drawn for it");
    artwork.set_plugin_displays(displays());
    assert_eq!(artwork.plugin_targets("Base").len(), 1);
    artwork.set_plugin_displays(BTreeSet::new());
    assert_eq!(artwork.plugin_targets("Base"), vec![]);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_zone_follows_the_touch_page_its_profile_owns() {
    let (root, mut layers, mut images, _) = artwork_fixture("ownership");
    let mut organization = Organization::open_at(root.join("org"), &layers).unwrap();
    let folder = organization.create_folder(&mut layers, "Tools").unwrap();
    organization.sync(&layers);
    layers
        .initialize_touchscreens(&organization, &mut images)
        .unwrap();
    layers
        .bind("Base", Trigger::ZoneTap(2), plugin_action(show(1)))
        .unwrap();
    let first = layers.touchscreen_owner("Base").unwrap().to_owned();
    let layers = Rc::new(RefCell::new(layers));
    let (output, _hardware) = mpsc::channel();
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
    artwork.set_plugin_displays(displays());
    let zone = vec![(ArtworkTarget::EncoderZone(2), show(1))];
    // The folder shares the profile's touch page.
    assert_eq!(artwork.plugin_targets(&folder), zone);
    layers.borrow_mut().create_touchscreen_page("Base").unwrap();
    assert_eq!(
        artwork.plugin_targets(&folder),
        vec![],
        "the new page is empty"
    );
    assert!(layers.borrow_mut().select_touchscreen_page("Base", &first));
    assert_eq!(artwork.plugin_targets(&folder), zone);
    std::fs::remove_dir_all(root).unwrap();
}

// ---- precedence -------------------------------------------------------------------------------

#[test]
fn an_explicit_information_choice_wins_over_a_plugin_value_on_the_same_control() {
    let (root, artwork, hardware) = bare_ui("precedence");
    {
        let mut layers = artwork.layers.borrow_mut();
        layers
            .bind("Base", Trigger::KeyPress(1), plugin_action(show(1)))
            .unwrap();
        layers
            .set_info("Base", &Trigger::KeyPress(1), Some(InfoSource::Clock))
            .unwrap();
        layers
            .bind("Base", Trigger::ZoneTap(2), plugin_action(show(1)))
            .unwrap();
        layers
            .set_info("Base", &Trigger::ZoneTap(2), Some(InfoSource::Cpu))
            .unwrap();
        layers
            .bind("Base", Trigger::KeyPress(3), plugin_action(show(1)))
            .unwrap();
    }
    artwork.set_plugin_displays(displays());
    assert_eq!(
        artwork.plugin_targets("Base"),
        vec![(ArtworkTarget::Keypad(3), show(1))]
    );
    assert_eq!(
        artwork.shown_targets("Base"),
        vec![
            (ArtworkTarget::Keypad(1), Shown::Info(InfoSource::Clock)),
            (ArtworkTarget::Keypad(3), Shown::Plugin(show(1))),
            (ArtworkTarget::EncoderZone(2), Shown::Info(InfoSource::Cpu)),
        ]
    );
    // An answer for the covered control is dropped, not drawn.
    artwork.update_plugin_value(
        "Base".into(),
        ArtworkTarget::Keypad(1),
        show(1),
        Ok(outcome("9")),
    );
    assert!(composed(&hardware).is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_running_countdown_keeps_its_key_over_a_plugin_value() {
    let (root, artwork, hardware) = bare_ui("countdown");
    artwork
        .layers
        .borrow_mut()
        .bind("Base", Trigger::KeyPress(1), plugin_action(show(1)))
        .unwrap();
    artwork.set_plugin_displays(displays());
    artwork.start_key_countdown("Base".into(), 1, 60);
    composed(&hardware);
    artwork.update_plugin_value(
        "Base".into(),
        ArtworkTarget::Keypad(1),
        show(1),
        Ok(outcome("5")),
    );
    assert!(
        composed(&hardware).is_empty(),
        "the countdown picture stays on the key"
    );
    std::fs::remove_dir_all(root).unwrap();
}

// ---- answers --------------------------------------------------------------------------------

#[test]
fn an_answer_becomes_headline_detail_and_level_and_an_error_becomes_two_dashes() {
    let (root, artwork, hardware) = bare_ui("mapping");
    artwork
        .layers
        .borrow_mut()
        .bind("Base", Trigger::KeyPress(1), plugin_action(show(1)))
        .unwrap();
    artwork.set_plugin_displays(displays());
    let key = ArtworkTarget::Keypad(1);
    artwork.update_plugin_value(
        "Base".into(),
        key,
        show(1),
        Ok(QueryOutcome {
            headline: "70%".into(),
            detail: Some("Brightness".into()),
            level: Some(70),
        }),
    );
    assert_eq!(
        composed(&hardware),
        vec![(
            key,
            InfoValue {
                level: Some(70),
                detail: "Brightness".into(),
                ..InfoValue::text(Glyph::None, "70%")
            }
        )]
    );
    artwork.update_plugin_value("Base".into(), key, show(1), Err("broken".into()));
    assert_eq!(
        composed(&hardware),
        vec![(
            key,
            InfoValue {
                inactive: true,
                ..InfoValue::text(Glyph::None, "--")
            }
        )]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_unchanged_answer_is_not_redrawn_and_a_stale_one_is_dropped() {
    let (root, artwork, hardware) = bare_ui("stale");
    artwork
        .layers
        .borrow_mut()
        .bind("Base", Trigger::KeyPress(1), plugin_action(show(1)))
        .unwrap();
    artwork.set_plugin_displays(displays());
    let key = ArtworkTarget::Keypad(1);
    for _ in 0..3 {
        artwork.update_plugin_value("Base".into(), key, show(1), Ok(outcome("70%")));
    }
    assert_eq!(composed(&hardware).len(), 1);
    // The binding changed while the query ran: the reply is for another binding.
    artwork.update_plugin_value("Base".into(), key, show(7), Ok(outcome("99%")));
    // The layer is gone or the control was rebound.
    artwork.update_plugin_value("Gone".into(), key, show(1), Ok(outcome("99%")));
    artwork.update_plugin_value(
        "Base".into(),
        ArtworkTarget::Keypad(5),
        show(1),
        Ok(outcome("99%")),
    );
    assert!(composed(&hardware).is_empty());
    assert_eq!(artwork.plugin_value("Base", key, &show(1)).headline, "70%");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn controls_without_plugin_values_are_unchanged() {
    let (root, artwork, hardware) = bare_ui("unchanged");
    {
        let mut layers = artwork.layers.borrow_mut();
        layers
            .bind("Base", Trigger::KeyPress(1), Action::Command("true".into()))
            .unwrap();
        layers
            .set_info("Base", &Trigger::KeyPress(1), Some(InfoSource::Clock))
            .unwrap();
        layers
            .bind("Base", Trigger::KeyPress(2), plugin_action(plain()))
            .unwrap();
    }
    artwork.set_plugin_displays(displays());
    assert_eq!(
        artwork.shown_targets("Base"),
        vec![(ArtworkTarget::Keypad(1), Shown::Info(InfoSource::Clock))]
    );
    assert_eq!(
        artwork.shown_targets("Base").len(),
        artwork.info_targets("Base").len()
    );
    artwork.refresh(true);
    let drawn = composed(&hardware);
    assert_eq!(drawn.len(), 1);
    assert_eq!(drawn[0].1, InfoSource::Clock.placeholder());
    // Without a query worker, refreshing asks nobody anything.
    artwork.request_plugin_values("Base", None);
    std::fs::remove_dir_all(root).unwrap();
}

// ---- live: queries on the plugin query worker -------------------------------------------------

struct Session {
    plugin: ValuePlugin,
    runtime: feature_runtime::FeatureRuntime,
    hub: plugin_ui::PluginHub,
    artwork: ArtworkUi,
    layers: Rc<RefCell<LayerStore>>,
    seen: RefCell<Vec<(ArtworkTarget, InfoValue)>>,
    icons: RefCell<Vec<ArtworkTarget>>,
    hardware: mpsc::Receiver<ArtworkCommand>,
    events: mpsc::Sender<MonitorEvent>,
}

impl Session {
    fn drain(&self) {
        for command in self.hardware.try_iter() {
            let list = match command {
                ArtworkCommand::Replace(list) => list,
                other => vec![other],
            };
            for command in list {
                match command {
                    ArtworkCommand::ComposeInfo { target, value, .. } => {
                        self.seen.borrow_mut().push((target, value))
                    }
                    ArtworkCommand::ComposeTouchscreenZone {
                        zone,
                        value: Some(value),
                        ..
                    } => self
                        .seen
                        .borrow_mut()
                        .push((ArtworkTarget::EncoderZone(zone), value)),
                    ArtworkCommand::ComposeIcon { target, .. } => {
                        self.icons.borrow_mut().push(target)
                    }
                    _ => {}
                }
            }
        }
    }

    fn latest(&self, target: ArtworkTarget) -> Option<InfoValue> {
        self.drain();
        self.seen
            .borrow()
            .iter()
            .rev()
            .find(|(shown, _)| *shown == target)
            .map(|(_, value)| value.clone())
    }

    /// Runs the main loop until `target` shows `headline`, failing after a bounded wait.
    fn await_headline(&self, target: ArtworkTarget, headline: &str) {
        let wake =
            glib::timeout_add_local(Duration::from_millis(20), || glib::ControlFlow::Continue);
        let started = std::time::Instant::now();
        while self
            .latest(target)
            .is_none_or(|value| value.headline != headline)
        {
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "timed out waiting for {target:?} to show {headline:?}; last: {:?}",
                self.latest(target)
            );
            glib::MainContext::default().iteration(true);
        }
        wake.remove();
    }
}

/// A full window over a temporary plugins folder; `setup` binds controls before it opens.
fn with_session(
    tag: &str,
    enabled: bool,
    setup: impl FnOnce(&mut LayerStore),
    check: impl FnOnce(&Session),
) {
    if !init_gtk_for_test() {
        return;
    }
    let plugin = ValuePlugin::new(tag);
    if enabled {
        plugin.enable();
    }
    let (root, mut store, images, _) = artwork_fixture(tag);
    setup(&mut store);
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
    let (commands, _requests) = mpsc::channel();
    let window = ApplicationWindow::builder()
        .default_width(1100)
        .default_height(900)
        .build();
    install_style(&window);
    let widgets = build_widgets(window.clone(), layers.clone(), artwork.clone(), commands);
    window.set_child(Some(&widgets.root));
    window.present();
    wait_for_frame(&window);
    let (runtime, hub) = (
        widgets.feature_runtime.clone(),
        widgets.binding_ui.plugins.clone(),
    );
    hub.load(plugin.0.clone());
    // What the window drew while it was built is not what a started session shows.
    while hardware.try_recv().is_ok() {}
    let (events, receiver) = mpsc::channel();
    let mut workers = vec![
        runtime.start_plugins(&events).unwrap().unwrap(),
        runtime.start_plugin_values(&events).unwrap().unwrap(),
    ];
    let (layer_sender, _layer_commands) = mpsc::channel();
    let polling = start_event_updates(
        widgets,
        receiver,
        layers.clone(),
        artwork.clone(),
        layer_sender,
        events.clone(),
    );
    let session = Session {
        plugin,
        runtime,
        hub,
        artwork,
        layers,
        seen: RefCell::default(),
        icons: RefCell::default(),
        hardware,
        events,
    };
    check(&session);
    polling.remove();
    for worker in &mut workers {
        assert!(worker.shutdown());
    }
    window.close();
    drop(session);
    std::fs::remove_dir_all(root).unwrap();
}

fn picture_size(artwork: &ArtworkUi, target: ArtworkTarget) -> Option<(i32, i32)> {
    let pictures = artwork.pictures.borrow();
    let (_, picture) = pictures.iter().find(|(shown, _)| *shown == target)?;
    let paintable = picture.paintable().filter(|_| picture.get_visible())?;
    Some((paintable.intrinsic_width(), paintable.intrinsic_height()))
}

#[test]
fn a_key_bound_to_a_display_action_shows_a_placeholder_then_the_plugins_value() {
    run_gtk_test(|| {
        with_session(
            "key",
            true,
            |layers| {
                layers
                    .bind("Base", Trigger::KeyPress(1), plugin_action(show(1)))
                    .unwrap();
            },
            |session| {
                let key = ArtworkTarget::Keypad(1);
                assert_eq!(
                    session.latest(key).map(|value| value.headline),
                    Some("...".into()),
                    "the placeholder is drawn before the first answer"
                );
                assert!(session.latest(key).unwrap().glyph == Glyph::None);
                session.await_headline(key, "70%");
                let value = session.latest(key).unwrap();
                assert_eq!(value.detail, "Brightness");
                assert_eq!(value.level, Some(70));
                assert_eq!(
                    picture_size(&session.artwork, key),
                    Some((120, 120)),
                    "the window preview shows the value too"
                );
                assert_eq!(session.plugin.calls("query"), 1);
            },
        )
    });
}

#[test]
fn a_touch_zone_bound_to_a_display_action_shows_the_plugins_value() {
    run_gtk_test(|| {
        with_session(
            "zone",
            true,
            |layers| {
                layers
                    .bind("Base", Trigger::ZoneTap(2), plugin_action(show(1)))
                    .unwrap();
            },
            |session| {
                session.await_headline(ArtworkTarget::EncoderZone(2), "70%");
                assert!(picture_size(&session.artwork, ArtworkTarget::EncoderZone(2)).is_some());
            },
        )
    });
}

#[test]
fn an_explicit_live_information_source_is_drawn_and_its_plugin_is_never_asked() {
    run_gtk_test(|| {
        with_session(
            "explicit",
            true,
            |layers| {
                layers
                    .bind("Base", Trigger::KeyPress(1), plugin_action(show(1)))
                    .unwrap();
                layers
                    .set_info("Base", &Trigger::KeyPress(1), Some(InfoSource::Clock))
                    .unwrap();
                layers
                    .bind("Base", Trigger::KeyPress(2), plugin_action(show(1)))
                    .unwrap();
            },
            |session| {
                session.await_headline(ArtworkTarget::Keypad(2), "70%");
                assert_eq!(
                    session.latest(ArtworkTarget::Keypad(1)),
                    Some(InfoSource::Clock.placeholder())
                );
                assert_eq!(session.plugin.calls("query"), 1, "only key 2 was asked");
            },
        )
    });
}

#[test]
fn the_more_icon_is_not_drawn_over_a_value_but_still_over_other_plugin_actions() {
    run_gtk_test(|| {
        with_session(
            "icon",
            true,
            |layers| {
                layers
                    .bind("Base", Trigger::KeyPress(1), plugin_action(show(1)))
                    .unwrap();
                layers
                    .bind("Base", Trigger::KeyPress(2), plugin_action(plain()))
                    .unwrap();
            },
            |session| {
                session.drain();
                assert!(session
                    .artwork
                    .control_icon("Base", ArtworkTarget::Keypad(1))
                    .is_none());
                assert!(session
                    .artwork
                    .control_icon("Base", ArtworkTarget::Keypad(2))
                    .is_some());
                let icons = session.icons.borrow();
                assert!(!icons.contains(&ArtworkTarget::Keypad(1)), "{icons:?}");
                assert!(icons.contains(&ArtworkTarget::Keypad(2)), "{icons:?}");
            },
        )
    });
}

#[test]
fn a_disabled_plugin_shows_nothing_until_it_is_enabled() {
    run_gtk_test(|| {
        with_session(
            "disabled",
            false,
            |layers| {
                layers
                    .bind("Base", Trigger::KeyPress(1), plugin_action(show(1)))
                    .unwrap();
            },
            |session| {
                let key = ArtworkTarget::Keypad(1);
                assert_eq!(session.latest(key), None);
                assert!(session.artwork.shown_targets("Base").is_empty());
                assert!(
                    session.artwork.control_icon("Base", key).is_some(),
                    "the control keeps its usual icon"
                );
                assert_eq!(session.plugin.calls("query"), 0);
                session.hub.set_enabled("demo", true).unwrap();
                session.await_headline(key, "70%");
                assert!(session.artwork.control_icon("Base", key).is_none());
                session.hub.set_enabled("demo", false).unwrap();
                assert!(session.artwork.shown_targets("Base").is_empty());
            },
        )
    });
}

#[test]
fn changing_the_layer_asks_the_plugin_again() {
    run_gtk_test(|| {
        with_session(
            "layers",
            true,
            |layers| {
                layers
                    .bind("Base", Trigger::KeyPress(1), plugin_action(show(1)))
                    .unwrap();
                layers
                    .bind("Studio", Trigger::KeyPress(1), plugin_action(show(2)))
                    .unwrap();
            },
            |session| {
                let key = ArtworkTarget::Keypad(1);
                session.await_headline(key, "70%");
                session.plugin.set_value(r#"{"headline":"80%"}"#);
                assert!(session.layers.borrow_mut().select("Studio"));
                session.artwork.refresh(false);
                session.await_headline(key, "80%");
                assert_eq!(session.plugin.calls("query"), 2);
            },
        )
    });
}

#[test]
fn running_an_action_of_the_same_plugin_asks_again_without_a_busy_press() {
    run_gtk_test(|| {
        with_session(
            "invoke",
            true,
            |layers| {
                layers
                    .bind("Base", Trigger::KeyPress(1), plugin_action(show(1)))
                    .unwrap();
            },
            |session| {
                let key = ArtworkTarget::Keypad(1);
                session.await_headline(key, "70%");
                session.plugin.set_value(r#"{"headline":"55%"}"#);
                session
                    .runtime
                    .execute("key".into(), plugin_action(plain()));
                session.await_headline(key, "55%");
                assert_eq!(session.plugin.calls("invoke"), 1);
                assert_eq!(session.plugin.calls("query"), 2);
                // Another plugin finishing asks nobody.
                session
                    .events
                    .send(MonitorEvent::PluginFinished {
                        plugin_id: "other".into(),
                    })
                    .unwrap();
            },
        )
    });
}

#[test]
fn a_failing_plugin_shows_two_dashes() {
    run_gtk_test(|| {
        with_session(
            "error",
            true,
            |layers| {
                layers
                    .bind("Base", Trigger::KeyPress(1), plugin_action(show(1)))
                    .unwrap();
            },
            |session| {
                session.plugin.fail();
                session.artwork.refresh(true);
                session.await_headline(ArtworkTarget::Keypad(1), "--");
                assert!(session.latest(ArtworkTarget::Keypad(1)).unwrap().inactive);
            },
        )
    });
}
