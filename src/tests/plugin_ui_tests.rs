use super::*;
use action_catalog_tests::{catalog_drop, catalog_fixture, collect_catalog_buttons};
use sdeck_plus::{features::FeatureAction, plugins::PluginBinding};
use std::os::unix::fs::PermissionsExt;

/// A temporary plugin folder with two valid plugins and one malformed one. The executables
/// would leave a marker if anything ever ran them.
struct PluginRoot(std::path::PathBuf);

impl PluginRoot {
    fn new(tag: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("sdeck-plugin-ui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (id, name, actions) in [
            (
                "demo",
                "Demo",
                r#"[{"id":"send","name":"Send message","description":"Sends one message.",
                    "controls":["key","dial_press"],
                    "settings":[
                      {"type":"text","id":"channel","label":"Channel","required":true,"max_length":32},
                      {"type":"integer","id":"level","label":"Level","min":0,"max":255,"default":10},
                      {"type":"boolean","id":"loud","label":"Loud"}]}]"#,
            ),
            (
                "lights",
                "Lights",
                r#"[{"id":"toggle","name":"Toggle lights","controls":["key"],"settings":[]}]"#,
            ),
        ] {
            let dir = root.join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("manifest.json"),
                format!(
                    r#"{{"id":"{id}","version":"1.0.0","protocol_version":1,"name":"{name}","executable":"run","actions":{actions}}}"#
                ),
            )
            .unwrap();
            std::fs::write(dir.join("run"), "#!/bin/sh\ntouch ran.marker\n").unwrap();
            std::fs::set_permissions(dir.join("run"), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        std::fs::create_dir_all(root.join("broken")).unwrap();
        std::fs::write(root.join("broken/manifest.json"), "{not json").unwrap();
        Self(root)
    }

    fn ran(&self) -> bool {
        ["demo", "lights"]
            .iter()
            .any(|id| self.0.join(id).join("ran.marker").exists())
    }
}

impl Drop for PluginRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn descendants<T: IsA<gtk::Widget>>(widget: &gtk::Widget, found: &mut Vec<T>) {
    if let Some(item) = widget.downcast_ref::<T>() {
        found.push(item.clone());
    }
    let mut child = widget.first_child();
    while let Some(next) = child {
        descendants(&next, found);
        child = next.next_sibling();
    }
}

fn all<T: IsA<gtk::Widget>>(widgets: &MonitorWidgets) -> Vec<T> {
    let mut found = Vec::new();
    descendants(widgets.root.upcast_ref(), &mut found);
    found
}

fn headers(widgets: &MonitorWidgets) -> Vec<String> {
    let mut buttons = Vec::new();
    collect_catalog_buttons(widgets.root.upcast_ref(), &mut buttons);
    buttons
        .iter()
        .filter_map(|button| button.tooltip_text())
        .filter_map(|tip| {
            tip.strip_prefix("Show or hide ")?
                .strip_suffix(" actions")
                .map(str::to_owned)
        })
        .collect()
}

fn texts(widgets: &MonitorWidgets) -> Vec<String> {
    all::<Label>(widgets)
        .iter()
        .map(|label| label.text().to_string())
        .collect()
}

fn row(widgets: &MonitorWidgets, caption: &str) -> Button {
    let mut buttons = Vec::new();
    collect_catalog_buttons(widgets.root.upcast_ref(), &mut buttons);
    buttons
        .into_iter()
        .filter(|button| button.has_css_class("catalog-action"))
        .find(|button| {
            let mut labels = Vec::<Label>::new();
            descendants(button.upcast_ref(), &mut labels);
            labels.iter().any(|label| label.text() == caption)
        })
        .unwrap_or_else(|| panic!("no catalog row {caption}"))
}

fn save_binding(widgets: &MonitorWidgets) {
    let mut buttons = Vec::new();
    collect_layout_buttons(widgets.root.upcast_ref(), &mut buttons);
    buttons
        .iter()
        .find(|b| button_caption(b).as_deref() == Some("Save binding"))
        .unwrap()
        .emit_clicked();
}

fn enable(widgets: &MonitorWidgets, label: &str) {
    all::<CheckButton>(widgets)
        .into_iter()
        .find(|check| check.label().as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no enable switch {label}"))
        .set_active(true);
}

#[test]
fn every_plugin_is_its_own_sidebar_item_with_status_and_visible_problems() {
    run_gtk_test(|| {
        catalog_fixture("plugin-sidebar", |widgets, _layers, _requests| {
            let root = PluginRoot::new("sidebar");
            let hub = &widgets.binding_ui.plugins;
            assert!(headers(widgets).iter().all(|name| !name.contains("Demo")));
            let choices = || widgets.binding_ui.action_picker.model().unwrap().n_items();
            assert_eq!(choices(), 8, "no plugin choice without plugins");
            hub.load(root.0.clone());
            assert_eq!(choices(), 9, "the plugin choice follows the first plugin");
            let titles = headers(widgets);
            assert!(titles.contains(&"Demo (disabled)".to_owned()), "{titles:?}");
            assert!(
                titles.contains(&"Lights (disabled)".to_owned()),
                "{titles:?}"
            );
            assert!(!titles.iter().any(|name| name == "Plugins"), "{titles:?}");
            let texts = texts(widgets);
            assert!(
                texts.iter().any(|t| t == sdeck_plus_untrusted()),
                "warning must be visible"
            );
            let problem = texts
                .iter()
                .find(|t| t.contains("broken"))
                .expect("malformed plugin reported");
            assert!(problem.contains("skipped"), "{problem}");
            // Disabled actions are listed but dimmed, and nothing has been enabled or run.
            assert!(!row(widgets, "Send message").is_sensitive());
            assert!(hub.plugins().iter().all(|(_, enabled)| !enabled));
            assert!(!root.ran());
            // A plugin added later appears after an explicit rescan.
            let extra = root.0.join("extra");
            std::fs::create_dir_all(&extra).unwrap();
            std::fs::write(
                extra.join("manifest.json"),
                r#"{"id":"extra","version":"1","protocol_version":1,"name":"Extra","executable":"run","actions":[{"id":"a","name":"A","controls":["key"]}]}"#,
            )
            .unwrap();
            std::fs::write(extra.join("run"), "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(extra.join("run"), std::fs::Permissions::from_mode(0o755))
                .unwrap();
            assert!(!headers(widgets)
                .iter()
                .any(|name| name.starts_with("Extra")));
            update_plugins(widgets).emit_clicked();
            assert!(headers(widgets)
                .iter()
                .any(|name| name.starts_with("Extra")));
            assert!(!root.ran());
        })
    });
}

/// The one rescan control, found by its tooltip.
fn update_plugins(widgets: &MonitorWidgets) -> Button {
    let found: Vec<Button> = all::<Button>(widgets)
        .into_iter()
        .filter(|b| b.tooltip_text().as_deref() == Some("Update plugins"))
        .collect();
    assert_eq!(found.len(), 1, "exactly one rescan control");
    found.into_iter().next().unwrap()
}

/// The one add control, found by its tooltip.
fn add_plugin(widgets: &MonitorWidgets) -> Button {
    let found: Vec<Button> = all::<Button>(widgets)
        .into_iter()
        .filter(|b| b.tooltip_text().as_deref() == Some("Add plugin…"))
        .collect();
    assert_eq!(found.len(), 1, "exactly one add control");
    found.into_iter().next().unwrap()
}

fn tool(widgets: &MonitorWidgets, name: &str) -> Button {
    all::<Button>(widgets)
        .into_iter()
        .find(|b| {
            b.tooltip_text()
                .is_some_and(|tip| tip.lines().next() == Some(name))
        })
        .unwrap_or_else(|| panic!("no {name} button"))
}

fn plugin_headings(widgets: &MonitorWidgets) -> Vec<Label> {
    all::<Label>(widgets)
        .into_iter()
        .filter(|label| label.has_css_class("plugin-heading"))
        .collect()
}

#[test]
fn the_add_and_rescan_controls_are_icons_at_the_right_end_of_the_binding_toolbar() {
    run_gtk_test(|| {
        catalog_fixture("plugin-rescan-toolbar", |widgets, _layers, _requests| {
            let rescan = update_plugins(widgets);
            assert!(rescan.label().is_none(), "icon only");
            let image = rescan.child().and_downcast::<gtk::Image>().expect("icon");
            assert_eq!(image.icon_name().as_deref(), Some("view-refresh-symbolic"));
            assert_eq!(rescan.tooltip_text().as_deref(), Some("Update plugins"));
            assert!(rescan.is_focusable() && rescan.is_sensitive());
            assert!(
                all::<Button>(widgets)
                    .iter()
                    .all(|b| button_caption(b).as_deref() != Some("Rescan plugins")),
                "the old text button is gone"
            );
            let row = rescan.parent().expect("toolbar row");
            // Save, Test and Remove now share the editor heading row; the plugin controls stay
            // in the side panel toolbar.
            let save = tool(widgets, "Save binding");
            let heading_row = save.parent().expect("editor heading row");
            assert!(heading_row.has_css_class("editor-heading-row"));
            assert_ne!(heading_row, row, "the plugin controls are not in that row");
            assert_eq!(
                tool(widgets, "Test action").parent().as_ref(),
                Some(&heading_row)
            );
            let remove = tool(widgets, "Remove binding");
            assert_eq!(remove.parent().as_ref(), Some(&heading_row));
            assert_eq!(row.last_child().as_ref(), Some(rescan.upcast_ref()));
            assert_eq!(rescan.halign(), gtk::Align::End);
            let add = add_plugin(widgets);
            assert!(add.label().is_none(), "icon only");
            let image = add.child().and_downcast::<gtk::Image>().expect("icon");
            assert_eq!(image.icon_name().as_deref(), Some("list-add-symbolic"));
            assert!(add.is_focusable() && add.is_sensitive());
            assert_eq!(add.parent().as_ref(), Some(&row), "same toolbar row");
            assert_eq!(
                add.next_sibling().as_ref(),
                Some(rescan.upcast_ref()),
                "add sits immediately before reload"
            );
            let bounds = |b: &Button| b.compute_bounds(&widgets.root);
            let (add_box, rescan_box_now) = (bounds(&add).unwrap(), bounds(&rescan).unwrap());
            let gap = rescan_box_now.x() - (add_box.x() + add_box.width());
            assert!(
                (0.0..=8.5).contains(&gap),
                "the normal toolbar spacing between add and reload: {gap}"
            );
            assert_eq!(add.height(), rescan.height(), "add height");
            let rescan_box = bounds(&rescan).unwrap();
            for name in ["Save binding", "Test action", "Remove binding"] {
                let other = tool(widgets, name);
                assert_eq!(other.height(), rescan.height(), "{name} height");
                assert!(rescan.height() > 0);
            }
            let rescan_box_end = rescan_box.x() + rescan_box.width();
            let row_box = row.compute_bounds(&widgets.root).unwrap();
            assert!(
                (row_box.x() + row_box.width() - rescan_box_end).abs() < 1.5,
                "flush with the right end"
            );
        })
    });
}

#[test]
fn the_toolbar_add_button_opens_one_dialog_and_the_hint_points_at_it() {
    run_gtk_test(|| {
        catalog_fixture("plugin-add-toolbar", |widgets, _layers, _requests| {
            let hub = &widgets.binding_ui.plugins;
            let hint = || {
                texts(widgets)
                    .into_iter()
                    .find(|text| text.starts_with("No plugins yet"))
                    .expect("empty-state hint")
            };
            assert_eq!(
                hint(),
                "No plugins yet. Use the Add plugin button in the toolbar to add one."
            );
            let add = add_plugin(widgets);
            assert!(plugin_add_ui::current(hub).is_none());
            add.emit_clicked();
            let dialog = plugin_add_ui::current(hub).expect("dialog opened");
            add.emit_clicked();
            let again = plugin_add_ui::current(hub).expect("still open");
            assert_eq!(dialog.window(), again.window(), "one instance at a time");
            assert!(
                dialog.window().transient_for().is_some(),
                "app is the parent"
            );
            let titled = gtk::Window::list_toplevels()
                .into_iter()
                .filter_map(|window| window.downcast::<gtk::Window>().ok())
                .filter(|w| w.is_visible() && w.title().as_deref() == Some("Add plugin"))
                .count();
            assert_eq!(titled, 1);
            dialog.window().close();
            // Reachable with a dial selected too, like reload.
            assert!(add_plugin(widgets).is_sensitive());
        })
    });
}

#[test]
fn the_plugin_heading_marks_where_plugins_begin_and_is_visible_without_plugins() {
    run_gtk_test(|| {
        catalog_fixture("plugin-heading", |widgets, _layers, _requests| {
            assert_eq!(plugin_headings(widgets).len(), 1, "always visible");
            let built_in = headers(widgets);
            let root = PluginRoot::new("heading");
            widgets.binding_ui.plugins.load(root.0.clone());
            let found = plugin_headings(widgets);
            assert_eq!(found.len(), 1);
            let heading = &found[0];
            assert_eq!(heading.text(), "Plugins");
            assert_eq!(heading.accessible_role(), gtk::AccessibleRole::Heading);
            let row = heading.parent().expect("heading row");
            assert_eq!(row.first_child().as_ref(), Some(heading.upcast_ref()));
            assert_eq!(
                row.last_child().as_ref(),
                Some(heading.upcast_ref()),
                "the heading row shows only the label"
            );
            assert!(
                !all::<Button>(widgets)
                    .iter()
                    .any(|b| b.is_ancestor(&row) || b.parent().as_ref() == Some(&row)),
                "no button in the heading row"
            );
            let mut buttons = Vec::new();
            collect_catalog_buttons(widgets.root.upcast_ref(), &mut buttons);
            // The heading's block and the first plugin item are siblings, the heading first.
            let region = heading
                .parent()
                .and_then(|row| row.parent())
                .and_then(|bar| bar.parent())
                .expect("region");
            let position = |inner: &gtk::Widget| {
                let mut index = 0;
                let mut child = region.first_child();
                while let Some(current) = child {
                    if inner == &current || inner.is_ancestor(&current) {
                        return index;
                    }
                    index += 1;
                    child = current.next_sibling();
                }
                panic!("not in the plugin region");
            };
            let demo = buttons
                .iter()
                .find(|b| {
                    b.tooltip_text().as_deref() == Some("Show or hide Demo (disabled) actions")
                })
                .expect("demo item");
            assert_eq!(position(heading.upcast_ref()), 0);
            assert!(position(demo.upcast_ref()) > 0, "plugin items follow it");
            let after = headers(widgets);
            assert!(after.starts_with(&built_in), "built-in order unchanged");
        })
    });
}

fn sdeck_plus_untrusted() -> &'static str {
    crate::plugin_ui::UNTRUSTED_WARNING
}

#[test]
fn enabling_is_explicit_saved_and_never_runs_the_plugin() {
    run_gtk_test(|| {
        catalog_fixture("plugin-enable", |widgets, _layers, _requests| {
            let root = PluginRoot::new("enable");
            let hub = &widgets.binding_ui.plugins;
            hub.load(root.0.clone());
            enable(widgets, "Enable Demo (v1.0.0)");
            assert_eq!(hub.manifest("demo").map(|(_, enabled)| enabled), Some(true));
            assert_eq!(
                hub.manifest("lights").map(|(_, enabled)| enabled),
                Some(false)
            );
            let state = std::fs::read_to_string(root.0.join("plugins.json")).unwrap();
            assert!(
                state.contains("demo") && !state.contains("lights"),
                "{state}"
            );
            assert!(headers(widgets).contains(&"Demo".to_owned()));
            assert!(row(widgets, "Send message").is_sensitive());
            all::<CheckButton>(widgets)
                .into_iter()
                .find(|check| check.label().as_deref() == Some("Enable Demo (v1.0.0)"))
                .unwrap()
                .set_active(false);
            assert_eq!(
                hub.manifest("demo").map(|(_, enabled)| enabled),
                Some(false)
            );
            assert!(!root.ran());
        })
    });
}

#[test]
fn an_invalid_enabled_state_is_reported_accessibly_and_blocks_enabling() {
    run_gtk_test(|| {
        catalog_fixture("plugin-state", |widgets, _layers, _requests| {
            let root = PluginRoot::new("state");
            std::fs::write(root.0.join("plugins.json"), "{broken").unwrap();
            widgets.binding_ui.plugins.load(root.0.clone());
            let alerts: Vec<Label> = all::<Label>(widgets)
                .into_iter()
                .filter(|label| label.accessible_role() == gtk::AccessibleRole::Alert)
                .collect();
            assert!(
                alerts
                    .iter()
                    .any(|label| label.text().contains("plugins.json")),
                "state error must be an alert"
            );
            enable(widgets, "Enable Demo (v1.0.0)");
            assert!(widgets
                .binding_ui
                .status
                .text()
                .contains("invalid plugin state"));
            assert_eq!(
                std::fs::read(root.0.join("plugins.json")).unwrap(),
                b"{broken"
            );
            // Built-in actions are unaffected.
            assert!(row(widgets, "Play / pause").is_sensitive());
        })
    });
}

#[test]
fn assigning_a_plugin_action_shows_a_generic_form_and_saving_never_runs_it() {
    run_gtk_test(|| {
        catalog_fixture("plugin-assign", |widgets, layers, requests| {
            let root = PluginRoot::new("assign");
            let ui = &widgets.binding_ui;
            ui.plugins.load(root.0.clone());
            enable(widgets, "Enable Demo (v1.0.0)");
            while requests.try_recv().is_ok() {}
            assert!(
                catalog_drop(widgets, "Send message", 1),
                "{}",
                ui.status.text()
            );
            assert_eq!(ui.action_kind(), ActionKind::Plugin);
            assert!(ui.plugin_editor.is_visible());
            assert!(!ui.feature_editor.root.is_visible());
            assert_eq!(ui.plugin_editor.field_count(), 3);
            assert_eq!(ui.plugin_editor.integer_range("level"), Some((0.0, 255.0)));
            // The draft is not saved or run until the user saves.
            assert_eq!(
                layers.borrow().binding("Base", &Trigger::KeyPress(2)),
                Some(Action::Command("original".into()))
            );
            assert!(requests.try_recv().is_err());
            let missing = ui.binding_from_widgets(layers).unwrap_err();
            assert!(missing.contains("channel"), "{missing}");
            ui.plugin_editor.set_text("channel", "general");
            save_binding(widgets);
            let mut saved = false;
            while let Ok(command) = requests.try_recv() {
                match command {
                    LayerCommand::SaveBinding(..) => saved = true,
                    LayerCommand::Execute { .. } => panic!("saving must not execute"),
                    _ => {}
                }
            }
            assert!(saved);
            let Some(Action::Feature(FeatureAction::Plugin(binding))) =
                layers.borrow().binding("Base", &Trigger::KeyPress(2))
            else {
                panic!("a plugin binding is stored");
            };
            assert_eq!(binding.plugin_id, "demo");
            assert_eq!(binding.settings["channel"], "general");
            assert_eq!(binding.settings["level"], 10);
            assert!(!root.ran());
        })
    });
}

#[test]
fn actions_are_filtered_by_the_selected_control() {
    run_gtk_test(|| {
        catalog_fixture("plugin-filter", |widgets, _layers, _requests| {
            let root = PluginRoot::new("filter");
            let ui = &widgets.binding_ui;
            ui.plugins.load(root.0.clone());
            enable(widgets, "Enable Demo (v1.0.0)");
            enable(widgets, "Enable Lights (v1.0.0)");
            assert!(row(widgets, "Toggle lights").is_sensitive());
            ui.select_control(ControlKind::Encoder, Some(1), true)
                .unwrap();
            ui.update_action_visibility();
            // Dial press fits Send message; Toggle lights is key-only.
            assert!(!row(widgets, "Toggle lights").is_sensitive());
            ui.action_picker.set_selected(plugin_ui::PLUGIN_PICKER);
            ui.update_action_visibility();
            assert_eq!(ui.plugin_editor.action_labels(), ["Send message"]);
            ui.event_picker.set_selected(1);
            ui.update_action_visibility();
            assert!(
                ui.plugin_editor.action_labels().is_empty(),
                "dial turn fits neither"
            );
            assert!(ui.plugin_editor.binding().is_err());
            ui.select_control(ControlKind::Button, Some(1), true)
                .unwrap();
            ui.update_action_visibility();
            assert!(row(widgets, "Toggle lights").is_sensitive());
            assert_eq!(ui.plugin_editor.action_labels().len(), 1);
        })
    });
}

#[test]
fn a_binding_for_a_missing_plugin_is_kept_verbatim_through_edit_and_save() {
    run_gtk_test(|| {
        catalog_fixture("plugin-missing", |widgets, layers, _requests| {
            let ui = &widgets.binding_ui;
            let root = PluginRoot::new("missing");
            ui.plugins.load(root.0.clone());
            let ghost = Action::Feature(FeatureAction::Plugin(PluginBinding::new(
                "ghost",
                "wave",
                serde_json::json!({"who": "everyone"})
                    .as_object()
                    .cloned()
                    .unwrap(),
            )));
            layers
                .borrow_mut()
                .bind("Base", Trigger::KeyPress(3), ghost.clone())
                .unwrap();
            ui.select_control(ControlKind::Button, Some(3), true)
                .unwrap();
            ui.load_selected_binding(layers);
            assert_eq!(ui.action_kind(), ActionKind::Plugin);
            assert!(
                ui.plugin_editor.notice().contains("not installed"),
                "{}",
                ui.plugin_editor.notice()
            );
            assert!(!ui.is_dirty());
            assert_eq!(ui.binding_from_widgets(layers).unwrap().2, ghost);
            save_binding(widgets);
            assert_eq!(
                layers.borrow().binding("Base", &Trigger::KeyPress(3)),
                Some(ghost)
            );
        })
    });
}

#[test]
fn plugin_actions_run_on_the_plugin_worker_and_never_the_layer_worker() {
    run_gtk_test(|| {
        catalog_fixture("plugin-run", |widgets, _layers, requests| {
            let root = PluginRoot::new("run");
            let ui = &widgets.binding_ui;
            let runtime = &widgets.feature_runtime;
            ui.plugins.load(root.0.clone());
            let action = Action::Feature(FeatureAction::Plugin(PluginBinding::new(
                "demo",
                "send",
                serde_json::json!({"channel": "general"})
                    .as_object()
                    .cloned()
                    .unwrap(),
            )));
            runtime.execute("key".into(), action.clone());
            assert_eq!(runtime.status.text(), "Plugin actions are unavailable.");
            let (events, results) = mpsc::channel();
            let mut worker = runtime
                .start_plugins(&events)
                .unwrap()
                .expect("a loaded root has a worker");
            ui.plugins.set_enabled("demo", true).unwrap();
            while requests.try_recv().is_ok() {}
            runtime.execute("key".into(), action);
            assert!(
                runtime.status.text().contains("queued"),
                "{}",
                runtime.status.text()
            );
            while let Ok(command) = requests.try_recv() {
                assert!(
                    !matches!(command, LayerCommand::Execute { .. }),
                    "plugins must not use the layer worker"
                );
            }
            // The fixture prints no protocol response, so the worker reports a failure.
            let MonitorEvent::LayerError(message) = results
                .recv_timeout(Duration::from_secs(10))
                .expect("a plugin result")
            else {
                panic!("a failed plugin is reported as a layer error");
            };
            assert!(
                message.contains("demo/send") && message.contains("request"),
                "{message}"
            );
            assert!(worker.shutdown());
        })
    });
}

fn demo_binding(level: u64) -> PluginBinding {
    PluginBinding::new(
        "demo",
        "send",
        serde_json::json!({"channel": "general", "level": level, "loud": false})
            .as_object()
            .cloned()
            .unwrap(),
    )
}

#[test]
fn a_saved_binding_whose_plugin_disappears_on_rescan_is_kept_until_another_action_is_chosen() {
    run_gtk_test(|| {
        catalog_fixture("plugin-rescan-gone", |widgets, _layers, _requests| {
            let root = PluginRoot::new("rescan-gone");
            let ui = &widgets.binding_ui;
            ui.plugins.load(root.0.clone());
            let saved = demo_binding(42);
            ui.plugin_editor.populate(&saved);
            assert_eq!(ui.plugin_editor.binding().unwrap(), saved);
            std::fs::remove_dir_all(root.0.join("demo")).unwrap();
            ui.plugins.rescan();
            assert_eq!(ui.plugin_editor.binding().unwrap(), saved);
            assert!(ui.plugin_editor.snapshot().starts_with("kept:demo/send"));
            assert!(
                ui.plugin_editor.notice().contains("not installed"),
                "{}",
                ui.plugin_editor.notice()
            );
            ui.plugin_editor.select("lights", "toggle");
            let chosen = ui.plugin_editor.binding().unwrap();
            assert_eq!(
                (chosen.plugin_id.as_str(), chosen.action_id.as_str()),
                ("lights", "toggle")
            );
        })
    });
}

#[test]
fn a_saved_binding_whose_action_or_settings_stop_fitting_on_rescan_is_kept_verbatim() {
    run_gtk_test(|| {
        catalog_fixture("plugin-rescan-changed", |widgets, _layers, _requests| {
            let root = PluginRoot::new("rescan-changed");
            let ui = &widgets.binding_ui;
            ui.plugins.load(root.0.clone());
            let saved = demo_binding(200);
            ui.plugin_editor.populate(&saved);
            assert_eq!(ui.plugin_editor.binding().unwrap(), saved);
            let manifest = root.0.join("demo/manifest.json");
            let original = std::fs::read_to_string(&manifest).unwrap();
            std::fs::write(&manifest, original.replace("\"max\":255", "\"max\":50")).unwrap();
            ui.plugins.rescan();
            assert_eq!(ui.plugin_editor.binding().unwrap(), saved);
            assert!(ui.plugin_editor.snapshot().starts_with("kept:demo/send"));
            assert!(!ui.plugin_editor.notice().is_empty());
            // The action disappears entirely while another one stays installed.
            std::fs::write(
                &manifest,
                original.replace("\"id\":\"send\"", "\"id\":\"other\""),
            )
            .unwrap();
            ui.plugins.rescan();
            assert_eq!(ui.plugin_editor.binding().unwrap(), saved);
            assert!(!ui.plugin_editor.notice().is_empty());
        })
    });
}

#[test]
fn a_rescan_keeps_the_users_edits_while_the_saved_binding_is_still_available() {
    run_gtk_test(|| {
        catalog_fixture("plugin-rescan-valid", |widgets, _layers, _requests| {
            let root = PluginRoot::new("rescan-valid");
            let ui = &widgets.binding_ui;
            ui.plugins.load(root.0.clone());
            ui.plugin_editor.populate(&demo_binding(42));
            ui.plugin_editor.set_text("channel", "edited");
            ui.plugins.rescan();
            let binding = ui.plugin_editor.binding().unwrap();
            assert_eq!(binding.settings["channel"], "edited");
            assert_eq!(binding.settings["level"], 42);
            assert!(!ui.plugin_editor.snapshot().starts_with("kept:"));
        })
    });
}

/// Rewrites the `demo` plugin's manifest so it declares `requirements`.
fn declare(root: &PluginRoot, requirements: &str) {
    let path = root.0.join("demo/manifest.json");
    let text = std::fs::read_to_string(&path).unwrap();
    let text = match text.find(",\"requirements\"") {
        Some(at) => format!("{}}}", &text[..at]),
        None => text,
    };
    let text = text.trim_end().trim_end_matches('}').to_owned();
    std::fs::write(path, format!("{text},\"requirements\":{requirements}}}")).unwrap();
}

fn missing_alerts(widgets: &MonitorWidgets) -> Vec<String> {
    all::<Label>(widgets)
        .into_iter()
        .filter(|label| label.accessible_role() == gtk::AccessibleRole::Alert)
        .map(|label| label.text().to_string())
        .filter(|text| text.starts_with("Missing: "))
        .collect()
}

#[test]
fn a_plugin_without_requirements_shows_no_requires_block() {
    run_gtk_test(|| {
        catalog_fixture("plugin-no-requires", |widgets, _layers, _requests| {
            let root = PluginRoot::new("no-requires");
            widgets.binding_ui.plugins.load(root.0.clone());
            assert!(!texts(widgets).iter().any(|text| text == "Requires"));
            assert!(missing_alerts(widgets).is_empty());
        })
    });
}

#[test]
fn the_requires_block_lists_requirements_and_flags_a_missing_required_command() {
    run_gtk_test(|| {
        catalog_fixture("plugin-requires", |widgets, _layers, _requests| {
            let root = PluginRoot::new("requires");
            declare(
                &root,
                r#"[{"kind":"command","name":"sdeck-absent-tool","version":">= 3.9","description":"Does the work"},
                    {"kind":"command","name":"sdeck-absent-extra","optional":true},
                    {"kind":"command","name":"sh"},
                    {"kind":"service","name":"bluetooth"},
                    {"kind":"file","name":"token","optional":true}]"#,
            );
            let hub = &widgets.binding_ui.plugins;
            hub.load(root.0.clone());
            let shown = texts(widgets);
            assert_eq!(shown.iter().filter(|t| *t == "Requires").count(), 1);
            for expected in [
                "command: sdeck-absent-tool >= 3.9",
                "command: sdeck-absent-extra (optional)",
                "command: sh",
                "service: bluetooth",
                "file: token (optional)",
            ] {
                assert!(shown.iter().any(|t| t == expected), "{expected}: {shown:?}");
            }
            let tip = all::<Label>(widgets)
                .into_iter()
                .find(|l| l.text() == "command: sdeck-absent-tool >= 3.9")
                .unwrap();
            assert_eq!(tip.tooltip_text().as_deref(), Some("Does the work"));
            // Only the required, missing command is an alert; the optional one is plain text.
            assert_eq!(missing_alerts(widgets), vec!["Missing: sdeck-absent-tool"]);
            assert!(shown.iter().any(|t| t == "Not found: sdeck-absent-extra"));
            // Missing requirements block neither enabling nor assigning.
            enable(widgets, "Enable Demo (v1.0.0)");
            assert!(hub
                .plugins()
                .iter()
                .any(|(m, enabled)| m.id == "demo" && *enabled));
            assert!(row(widgets, "Send message").is_sensitive());
            assert!(!root.ran());
        })
    });
}

#[test]
fn the_requires_block_is_rechecked_on_every_rescan_and_runs_nothing() {
    run_gtk_test(|| {
        catalog_fixture("plugin-requires-rescan", |widgets, _layers, _requests| {
            let root = PluginRoot::new("requires-rescan");
            declare(&root, r#"[{"kind":"command","name":"sdeck-absent-tool"}]"#);
            widgets.binding_ui.plugins.load(root.0.clone());
            assert_eq!(missing_alerts(widgets), vec!["Missing: sdeck-absent-tool"]);
            declare(&root, r#"[{"kind":"command","name":"sh"}]"#);
            update_plugins(widgets).emit_clicked();
            assert!(missing_alerts(widgets).is_empty());
            assert!(texts(widgets).iter().any(|t| t == "command: sh"));
            declare(&root, r#"[{"kind":"command","name":"sdeck-absent-other"}]"#);
            update_plugins(widgets).emit_clicked();
            assert_eq!(missing_alerts(widgets), vec!["Missing: sdeck-absent-other"]);
            declare(&root, "[]");
            update_plugins(widgets).emit_clicked();
            assert!(!texts(widgets).iter().any(|text| text == "Requires"));
            assert!(!root.ran());
        })
    });
}
