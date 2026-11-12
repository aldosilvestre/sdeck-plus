use super::*;
use crate::plugin_add_ui::{self, AddPluginDialog, Phase};
use action_catalog_tests::{catalog_fixture, collect_catalog_buttons};
use gtk::glib::translate::IntoGlib;
use sdeck_plus::plugins::{
    install, stage, CancelToken, InstallError, InstallMode, PluginSource, StageError, StageOptions,
};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const HASH: &str = "0123456789abcdef0123456789abcdef01234567";
const URL: &str = "https://example.invalid/team/plugin.git";

/// A scratch tree outside the repository: projects to add, the plugins folder, the directory
/// staging happens in (so leftovers show) and fake git scripts.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("sdeck-add-ui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        for dir in ["plugins", "tmp", "src"] {
            std::fs::create_dir_all(path.join(dir)).unwrap();
        }
        Self(path)
    }

    fn plugins(&self) -> PathBuf {
        self.0.join("plugins")
    }

    fn tmp(&self) -> PathBuf {
        self.0.join("tmp")
    }

    fn options(&self) -> StageOptions {
        StageOptions::new()
            .temp_dir(self.tmp())
            .plugins_root(self.plugins())
    }

    /// Staging copies are deleted on a short-lived thread, so wait (bounded) for the signal.
    fn nothing_staged(&self) {
        let tmp = self.tmp();
        wait_until("every staging directory to be removed", || {
            std::fs::read_dir(&tmp).unwrap().next().is_none()
        });
    }

    fn installed_names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.plugins())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// A valid project: manifest, executable (leaves `ran.marker` if ever run) and a sibling file.
    fn project(&self, dir: &str, id: &str, version: &str) -> PathBuf {
        let path = self.0.join("src").join(dir);
        std::fs::create_dir_all(path.join("lib")).unwrap();
        let manifest = serde_json::json!({
            "id": id, "version": version, "protocol_version": 1, "name": "Fixture",
            "executable": "run",
            "actions": [{
                "id": "send", "name": "Send", "controls": ["key", "dial_press"],
                "settings": [
                    {"type": "text", "id": "channel", "label": "Channel", "required": true},
                    {"type": "boolean", "id": "loud", "label": "Loud"}
                ]
            }],
            "requirements": [
                {"kind": "command", "name": "sh"},
                {"kind": "command", "name": "sdeck-absent-tool"},
                {"kind": "service", "name": "bluetooth", "optional": true}
            ]
        });
        std::fs::write(path.join("manifest.json"), manifest.to_string()).unwrap();
        std::fs::write(path.join("run"), "#!/bin/sh\ntouch ran.marker\n").unwrap();
        std::fs::set_permissions(path.join("run"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(path.join("lib/helper.py"), "print('x')\n").unwrap();
        path
    }

    /// An executable `/bin/sh` script, warmed up so a racing fork cannot make `exec` fail with
    /// ETXTBSY.
    fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(
            &path,
            format!("#!/bin/sh\n[ \"$1\" = --sdeck-warm ] && exit 0\n{body}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        for _ in 0..200 {
            match std::process::Command::new(&path)
                .arg("--sdeck-warm")
                .status()
            {
                Err(error) if error.raw_os_error() == Some(26) => {
                    std::thread::yield_now();
                }
                other => {
                    other.unwrap();
                    break;
                }
            }
        }
        path
    }

    /// A fake git that "clones" `fixture` after `delay` seconds and reports `HASH`.
    fn git(&self, fixture: &Path, delay: u32) -> PathBuf {
        self.script(
            "git-fake",
            &format!(
                r#"for a; do dest=$a; done
case " $* " in
*" clone "*)
  sleep {delay}
  mkdir "$dest" && cp -R '{fixture}'/. "$dest"
  mkdir "$dest/.git" && echo ref > "$dest/.git/HEAD"
  ;;
*" rev-parse "*) echo '{HASH}' ;;
esac"#,
                fixture = fixture.display()
            ),
        )
    }

    fn pid_file(&self) -> PathBuf {
        self.0.join("child.pid")
    }

    /// A git that starts a grandchild, records its pid and waits for it.
    fn hanging_git(&self) -> PathBuf {
        self.script(
            "git-hang",
            &format!(
                "sleep 30 &\necho $! > '{}'\nwait",
                self.pid_file().display()
            ),
        )
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs the main loop until `done` holds; a wake-up source keeps it from blocking.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let wake = glib::timeout_add_local(Duration::from_millis(20), || glib::ControlFlow::Continue);
    let started = std::time::Instant::now();
    while !done() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "timed out waiting for {what}"
        );
        glib::MainContext::default().iteration(true);
    }
    wake.remove();
}

fn process_gone(pid: &str) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat.rsplit(") ").next().is_some_and(|s| s.starts_with('Z')),
    }
}

fn labels_in(widget: &gtk::Widget) -> Vec<Label> {
    fn walk(widget: &gtk::Widget, found: &mut Vec<Label>) {
        if let Some(label) = widget.downcast_ref::<Label>() {
            found.push(label.clone());
        }
        let mut child = widget.first_child();
        while let Some(next) = child {
            walk(&next, found);
            child = next.next_sibling();
        }
    }
    let mut found = Vec::new();
    walk(widget, &mut found);
    found
}

fn card_texts(dialog: &AddPluginDialog) -> Vec<String> {
    let mut labels = labels_in(dialog.card().upcast_ref());
    labels.extend(labels_in(dialog.pinned().upcast_ref()));
    labels
        .iter()
        .map(|label| label.text().to_string())
        .collect()
}

fn error_texts(dialog: &AddPluginDialog) -> Vec<String> {
    dialog
        .error_labels()
        .iter()
        .map(|label| label.text().to_string())
        .collect()
}

fn add_button(widgets: &MonitorWidgets) -> Button {
    let mut buttons = Vec::new();
    collect_catalog_buttons(widgets.root.upcast_ref(), &mut buttons);
    let found: Vec<Button> = buttons
        .into_iter()
        .filter(|b| b.tooltip_text().as_deref() == Some("Add plugin…"))
        .collect();
    assert_eq!(found.len(), 1, "exactly one add button");
    found.into_iter().next().unwrap()
}

fn open_dialog(widgets: &MonitorWidgets) -> AddPluginDialog {
    add_button(widgets).emit_clicked();
    plugin_add_ui::current(&widgets.binding_ui.plugins).expect("the add dialog opened")
}

/// A main window with a hub on `scratch` (temp plugins folder, injected staging options) and
/// the dialog opened through the real add button.
fn with_dialog(
    tag: &str,
    prepare: impl FnOnce(&Scratch) -> StageOptions,
    check: impl FnOnce(&MonitorWidgets, &Scratch, &AddPluginDialog),
) {
    catalog_fixture(&format!("add-{tag}"), |widgets, _layers, _requests| {
        let scratch = Scratch::new(tag);
        let options = prepare(&scratch);
        let hub = &widgets.binding_ui.plugins;
        hub.load(scratch.plugins());
        hub.use_stage_options(options);
        let dialog = open_dialog(widgets);
        check(widgets, &scratch, &dialog);
        dialog.window().close();
        assert!(
            !dialog.window().is_visible(),
            "the dialog closes at the end of every test"
        );
    });
}

/// Types `text` and presses Check plugin; waits for the verdict.
fn run_check(dialog: &AddPluginDialog, text: &str) {
    dialog.entry().set_text(text);
    dialog.check_button().emit_clicked();
    assert_eq!(dialog.state_phase(), Phase::Checking);
    wait_until("the check to finish", || {
        dialog.state_phase() != Phase::Checking
    });
}

fn install_caption(dialog: &AddPluginDialog) -> String {
    button_caption(&dialog.install_button())
        .expect("install caption")
        .to_string()
}

fn capture(dialog: &AddPluginDialog, name: &str) {
    let Some(directory) = std::env::var_os("STREAMDECK_CATALOG_QA_DIR") else {
        return;
    };
    let window = dialog.window();
    wait_for_frame(&window);
    std::fs::create_dir_all(&directory).unwrap();
    rendered_texture(&window, &window)
        .save_to_png(Path::new(&directory).join(format!("add-plugin-{name}.png")))
        .unwrap();
}

fn plugins_json(scratch: &Scratch) -> Option<String> {
    std::fs::read_to_string(scratch.plugins().join("plugins.json")).ok()
}

#[test]
fn the_add_button_sits_next_to_update_plugins_and_opens_one_dialog() {
    run_gtk_test(|| {
        catalog_fixture("add-button", |widgets, _layers, _requests| {
            let hub = &widgets.binding_ui.plugins;
            // No plugin folder and no plugin: the heading is still there and the add button lives
            // in the binding toolbar, right before Update plugins.
            let heading: Vec<Label> = labels_in(widgets.root.upcast_ref())
                .into_iter()
                .filter(|label| label.has_css_class("plugin-heading"))
                .collect();
            assert_eq!(heading.len(), 1, "the heading is always visible");
            let add = add_button(widgets);
            assert!(add.label().is_none(), "icon only");
            let image = add.child().and_downcast::<gtk::Image>().expect("icon");
            assert_eq!(image.icon_name().as_deref(), Some("list-add-symbolic"));
            assert_eq!(add.tooltip_text().as_deref(), Some("Add plugin…"));
            assert!(add.is_focusable() && add.is_sensitive());
            let row = add.parent().expect("toolbar row");
            assert_ne!(
                heading[0].parent().as_ref(),
                Some(&row),
                "the heading row holds only the label"
            );
            let reload = add.next_sibling().expect("Update plugins follows");
            assert_eq!(reload.tooltip_text().as_deref(), Some("Update plugins"));
            assert_eq!(row.last_child().as_ref(), Some(&reload));
            assert_eq!(heading[0].accessible_role(), gtk::AccessibleRole::Heading);
            let hint = labels_in(widgets.root.upcast_ref())
                .into_iter()
                .map(|label| label.text().to_string())
                .find(|text| text.starts_with("No plugins yet"))
                .expect("empty-state hint");
            assert_eq!(
                hint,
                "No plugins yet. Use the Add plugin button in the toolbar to add one."
            );
            let root = Scratch::new("hint");
            hub.load(root.plugins());
            let rooted = labels_in(widgets.root.upcast_ref())
                .into_iter()
                .map(|label| label.text().to_string())
                .find(|text| text.starts_with("No plugins yet"))
                .expect("hint with a root");
            assert_eq!(
                rooted,
                format!(
                    "No plugins yet. Use the Add plugin button in the toolbar, or copy a plugin folder into {} and use Update plugins.",
                    root.plugins().display()
                )
            );

            if let Some(directory) = std::env::var_os("STREAMDECK_CATALOG_QA_DIR") {
                let native = widgets.root.native().expect("native");
                wait_for_frame(&widgets.root);
                std::fs::create_dir_all(&directory).unwrap();
                rendered_texture(&native, &row)
                    .save_to_png(Path::new(&directory).join("add-plugin-toolbar.png"))
                    .unwrap();
            }
            // Loading rebuilt the sidebar, so the button is a new widget.
            let add = add_button(widgets);
            assert!(plugin_add_ui::current(hub).is_none());
            add.emit_clicked();
            let dialog = plugin_add_ui::current(hub).expect("dialog opened");
            add.emit_clicked();
            let again = plugin_add_ui::current(hub).expect("still open");
            assert_eq!(dialog.window(), again.window(), "one instance at a time");
            let titled = gtk::Window::list_toplevels()
                .into_iter()
                .filter_map(|window| window.downcast::<gtk::Window>().ok())
                .filter(|window| {
                    window.is_visible() && window.title().as_deref() == Some("Add plugin")
                })
                .count();
            assert_eq!(titled, 1);
            let window = dialog.window();
            assert!(window.is_modal());
            assert!(window.transient_for().is_some(), "transient for the app");
            assert_eq!(
                dialog.entry().placeholder_text().unwrap().as_str(),
                "Folder path or https:// repository address"
            );
            let focus = gtk::prelude::GtkWindowExt::focus(&window).expect("initial focus");
            assert!(
                focus == *dialog.entry().upcast_ref::<gtk::Widget>()
                    || focus.is_ancestor(&dialog.entry()),
                "the entry has the initial focus"
            );
            assert!(!dialog.install_button().is_sensitive());
            assert_eq!(install_caption(&dialog), "Install");
            let texts: Vec<String> = labels_in(window.upcast_ref())
                .iter()
                .map(|label| label.text().to_string())
                .collect();
            assert!(texts
                .iter()
                .any(|t| t.contains("Nothing runs until you enable it")));
            capture(&dialog, "idle");
            // Escape closes it, and a new press opens a fresh one.
            let controllers = window.observe_controllers();
            let escape = (0..controllers.n_items())
                .filter_map(|i| controllers.item(i))
                .find_map(|c| c.downcast::<gtk::EventControllerKey>().ok())
                .expect("key controller");
            let handled = escape.emit_by_name::<bool>(
                "key-pressed",
                &[
                    &gtk::gdk::Key::Escape.into_glib(),
                    &0u32,
                    &gtk::gdk::ModifierType::empty(),
                ],
            );
            assert!(handled && !window.is_visible(), "Escape closes the dialog");
            add.emit_clicked();
            let reopened = plugin_add_ui::current(hub).expect("reopened");
            assert_ne!(reopened.window(), window);
            reopened.window().close();
            // Browse opens a modal folder chooser.
            let browse = open_dialog(widgets);
            let chooser = browse.chooser();
            assert_eq!(chooser.action(), gtk::FileChooserAction::SelectFolder);
            assert!(chooser.is_modal());
            chooser.emit_by_name::<()>("response", &[&gtk::ResponseType::Cancel.into_glib()]);
            assert!(browse.browse_button().is_sensitive());
            browse.window().close();
        })
    });
}

#[test]
fn a_folder_is_checked_previewed_and_installed_disabled() {
    run_gtk_test(|| {
        with_dialog(
            "folder",
            |scratch| scratch.options(),
            |widgets, scratch, dialog| {
                let project = scratch.project("proj", "newplug", "2.0.0");
                let canonical = std::fs::canonicalize(&project).unwrap();
                dialog.entry().set_text(&project.to_string_lossy());
                dialog.check_button().emit_clicked();
                assert_eq!(dialog.state_phase(), Phase::Checking);
                assert!(dialog.spinner_spinning(), "a spinner while checking");
                assert!(dialog.cancel_button().is_visible());
                assert!(!dialog.install_button().is_sensitive());
                wait_until("the preview", || dialog.state_phase() != Phase::Checking);
                assert_eq!(
                    dialog.state_phase(),
                    Phase::Preview,
                    "{:?}",
                    error_texts(dialog)
                );
                assert!(!dialog.spinner_spinning());
                let texts = card_texts(dialog);
                let has = |needle: &str| texts.iter().any(|t| t == needle);
                assert!(has("Name: Fixture") && has("Id: newplug") && has("Version: 2.0.0"));
                assert!(
                    has(&format!("Source: {}", canonical.display())),
                    "{texts:?}"
                );
                assert!(
                    texts.iter().any(|t| t.starts_with("Files: 3 (")),
                    "{texts:?}"
                );
                assert!(has("Send: Key · Dial press"), "{texts:?}");
                assert!(texts
                    .iter()
                    .any(|t| t.starts_with("Settings: Channel (text, required)")));
                assert!(has("command: sh") && has("command: sdeck-absent-tool"));
                let missing: Vec<Label> = labels_in(dialog.card().upcast_ref())
                    .into_iter()
                    .filter(|l| l.text().starts_with("Missing:"))
                    .collect();
                assert_eq!(missing.len(), 1, "only the absent command is flagged");
                assert_eq!(missing[0].text(), "Missing: sdeck-absent-tool");
                assert_eq!(missing[0].accessible_role(), gtk::AccessibleRole::Alert);
                assert!(
                    texts
                        .iter()
                        .any(|t| t.contains("runs with your user's privileges")
                            && t.contains("Read its code")),
                    "{texts:?}"
                );
                assert!(dialog.install_button().is_sensitive());
                assert_eq!(install_caption(dialog), "Install");
                assert!(
                    scratch.installed_names().is_empty(),
                    "nothing before Install"
                );
                capture(dialog, "preview");

                dialog.install_button().emit_clicked();
                assert_eq!(dialog.state_phase(), Phase::Installing);
                assert!(!dialog.install_button().is_sensitive());
                let hub = &widgets.binding_ui.plugins;
                wait_until("the install", || !dialog.window().is_visible());
                let (manifest, enabled) = hub.manifest("newplug").expect("a sidebar item");
                assert_eq!(manifest.version, "2.0.0");
                assert!(!enabled, "installed disabled");
                assert!(plugins_json(scratch).is_none(), "nothing was enabled");
                assert!(!scratch.plugins().join("newplug/ran.marker").exists());
                assert!(scratch.plugins().join("newplug/lib/helper.py").is_file());
                let status = widgets.binding_ui.status.text();
                assert_eq!(
                    status.as_str(),
                    "Installed Fixture 2.0.0. It is disabled: enable it in the Plugins section."
                );
                assert!(
                    labels_in(widgets.root.upcast_ref())
                        .iter()
                        .any(|l| l.text() == "Fixture (disabled)"),
                    "its own sidebar item"
                );
                scratch.nothing_staged();
            },
        )
    });
}

#[test]
fn editing_the_entry_invalidates_the_preview() {
    run_gtk_test(|| {
        with_dialog(
            "edit",
            |scratch| scratch.options(),
            |_widgets, scratch, dialog| {
                let project = scratch.project("proj", "editme", "1.0.0");
                let text = project.to_string_lossy().into_owned();
                run_check(dialog, &text);
                assert_eq!(dialog.state_phase(), Phase::Preview);
                assert_eq!(dialog.candidate_text().as_deref(), Some(text.as_str()));
                dialog.entry().set_text(&format!("{text}/"));
                assert_eq!(dialog.state_phase(), Phase::Idle);
                assert!(!dialog.install_button().is_sensitive());
                assert!(card_texts(dialog).is_empty(), "the preview is gone");
                assert!(dialog.candidate_text().is_none());
                scratch.nothing_staged();
                // Setting the same text again does not bring it back: check again.
                dialog.entry().set_text(&text);
                assert!(!dialog.install_button().is_sensitive());
                dialog.entry().set_text("");
                dialog.install_button().emit_clicked();
                assert!(scratch.installed_names().is_empty());
            },
        )
    });
}

#[test]
fn invalid_sources_show_errors_and_install_nothing() {
    run_gtk_test(|| {
        with_dialog(
            "invalid",
            |scratch| scratch.options(),
            |_widgets, scratch, dialog| {
                let file = scratch.0.join("src/a-file");
                std::fs::write(&file, "x").unwrap();
                let empty = scratch.0.join("src/no-manifest");
                std::fs::create_dir_all(&empty).unwrap();
                let hostile = scratch.project("hostile", "hostile", "1.0.0");
                let manifest = std::fs::read_to_string(hostile.join("manifest.json")).unwrap();
                std::fs::write(
                    hostile.join("manifest.json"),
                    manifest.replace("\"executable\":\"run\"", "\"executable\":\"../outside\""),
                )
                .unwrap();
                let big = scratch.project("big", "big", "1.0.0");
                for n in 0..260 {
                    std::fs::write(big.join(format!("lib/f{n}.txt")), "x").unwrap();
                }
                let linked = scratch.project("linked", "linked", "1.0.0");
                std::os::unix::fs::symlink("/etc/hostname", linked.join("link")).unwrap();
                let inside = scratch.plugins();
                let cases: Vec<(String, &str)> = vec![
                    (String::new(), "enter a folder path"),
                    ("   ".into(), "enter a folder path"),
                    ("relative/path".into(), "absolute path"),
                    ("http://example.invalid/x.git".into(), "only https://"),
                    ("ssh://git@example.invalid/x.git".into(), "only https://"),
                    ("git@example.invalid:x/y.git".into(), "only https://"),
                    (file.to_string_lossy().into(), "not a folder"),
                    (empty.to_string_lossy().into(), "plugin format"),
                    (hostile.to_string_lossy().into(), "plugin format"),
                    (big.to_string_lossy().into(), "plugin format"),
                    (linked.to_string_lossy().into(), "plugin format"),
                    (inside.to_string_lossy().into(), ""),
                ];
                for (text, expect) in cases {
                    run_check(dialog, &text);
                    let errors = error_texts(dialog);
                    assert_eq!(dialog.state_phase(), Phase::Invalid, "{text:?}: {errors:?}");
                    assert!(!errors.is_empty(), "{text:?}");
                    assert!(
                        errors.iter().any(|e| e.to_lowercase().contains(expect)),
                        "{text:?}: {errors:?}"
                    );
                    for label in dialog.error_labels() {
                        assert_eq!(label.accessible_role(), gtk::AccessibleRole::Alert);
                        let raw = label.text();
                        assert!(
                            !raw.contains("Invalid(")
                                && !raw.contains("Source(")
                                && !raw.contains('{'),
                            "no debug output: {raw}"
                        );
                    }
                    assert!(!dialog.install_button().is_sensitive(), "{text:?}");
                    assert!(card_texts(dialog).is_empty());
                    assert!(scratch.installed_names().is_empty(), "{text:?}");
                    scratch.nothing_staged();
                    if text == "relative/path" {
                        capture(dialog, "error");
                    }
                }
            },
        )
    });
}

fn installed(scratch: &Scratch, dir: &str, id: &str, version: &str) {
    let project = scratch.project(dir, id, version);
    let source = PluginSource::Folder(std::fs::canonicalize(project).unwrap());
    let staged = stage(&source, &scratch.options(), &CancelToken::new()).unwrap();
    install(&staged, &scratch.plugins(), InstallMode::New).unwrap();
}

#[test]
fn an_installed_id_offers_replace_which_disables_the_plugin_only_when_pressed() {
    run_gtk_test(|| {
        with_dialog(
            "replace",
            |scratch| scratch.options(),
            |widgets, scratch, dialog| {
                installed(scratch, "old", "dup", "1.0.0");
                let hub = &widgets.binding_ui.plugins;
                hub.rescan();
                hub.set_enabled("dup", true).unwrap();
                assert!(plugins_json(scratch).unwrap().contains("dup"));
                let project = scratch.project("new", "dup", "2.0.0");
                run_check(dialog, &project.to_string_lossy());
                assert_eq!(dialog.state_phase(), Phase::Replace);
                assert_eq!(install_caption(dialog), "Replace");
                assert!(dialog.install_button().is_sensitive());
                let texts = card_texts(dialog);
                assert!(
                    texts
                        .iter()
                        .any(|t| t.contains("already installed (version 1.0.0)")
                            && t.contains("disables it until you enable it again")),
                    "{texts:?}"
                );
                // Checking alone changed nothing: still enabled, old version in place.
                assert!(plugins_json(scratch).unwrap().contains("dup"));
                assert_eq!(hub.manifest("dup").unwrap().0.version, "1.0.0");
                assert!(hub.manifest("dup").unwrap().1);
                capture(dialog, "replace");

                dialog.install_button().emit_clicked();
                wait_until("the replace", || !dialog.window().is_visible());
                let (manifest, enabled) = hub.manifest("dup").unwrap();
                assert_eq!(manifest.version, "2.0.0");
                assert!(!enabled, "replace disables until enabled again");
                assert!(!plugins_json(scratch).unwrap_or_default().contains("dup"));
                let status = widgets.binding_ui.status.text();
                assert!(
                    status.contains("Installed Fixture 2.0.0")
                        && status.contains("was disabled")
                        && status.contains("enable it again"),
                    "{status}"
                );
                scratch.nothing_staged();
            },
        )
    });
}

#[test]
fn a_race_that_installs_the_id_first_flips_to_replace_instead_of_failing_silently() {
    run_gtk_test(|| {
        with_dialog(
            "race",
            |scratch| scratch.options(),
            |widgets, scratch, dialog| {
                let project = scratch.project("new", "racy", "2.0.0");
                run_check(dialog, &project.to_string_lossy());
                assert_eq!(dialog.state_phase(), Phase::Preview);
                installed(scratch, "other", "racy", "1.5.0");
                dialog.install_button().emit_clicked();
                wait_until("the verdict", || dialog.state_phase() != Phase::Installing);
                assert_eq!(dialog.state_phase(), Phase::Replace);
                assert_eq!(install_caption(dialog), "Replace");
                assert!(!error_texts(dialog).is_empty(), "the flip is explained");
                assert!(
                    card_texts(dialog)
                        .iter()
                        .any(|t| t.contains("(version 1.5.0)")),
                    "{:?}",
                    card_texts(dialog)
                );
                let hub = &widgets.binding_ui.plugins;
                hub.rescan();
                assert_eq!(
                    hub.manifest("racy").unwrap().0.version,
                    "1.5.0",
                    "untouched"
                );
                dialog.install_button().emit_clicked();
                wait_until("the replace", || !dialog.window().is_visible());
                assert_eq!(hub.manifest("racy").unwrap().0.version, "2.0.0");
                scratch.nothing_staged();
            },
        )
    });
}

#[test]
fn a_repository_is_fetched_by_git_off_the_gtk_thread_and_shows_its_commit() {
    run_gtk_test(|| {
        with_dialog(
            "repo",
            |scratch| {
                let project = scratch.project("remote", "remoteplug", "3.1.0");
                scratch.options().git_program(scratch.git(&project, 1))
            },
            |widgets, scratch, dialog| {
                dialog.entry().set_text(URL);
                dialog.check_button().emit_clicked();
                assert_eq!(dialog.state_phase(), Phase::Checking);
                // The main loop keeps running while the fake git sleeps.
                let ticks = Rc::new(Cell::new(0u32));
                let counter = ticks.clone();
                let tick = glib::timeout_add_local(Duration::from_millis(25), move || {
                    counter.set(counter.get() + 1);
                    glib::ControlFlow::Continue
                });
                wait_until("the clone", || dialog.state_phase() != Phase::Checking);
                tick.remove();
                assert!(
                    ticks.get() >= 5,
                    "the GTK thread stayed responsive: {}",
                    ticks.get()
                );
                assert_eq!(
                    dialog.state_phase(),
                    Phase::Preview,
                    "{:?}",
                    error_texts(dialog)
                );
                let texts = card_texts(dialog);
                assert!(
                    texts
                        .iter()
                        .any(|t| t == &format!("Source: {URL} (commit 0123456789)")),
                    "{texts:?}"
                );
                dialog.install_button().emit_clicked();
                wait_until("the install", || !dialog.window().is_visible());
                let hub = &widgets.binding_ui.plugins;
                assert!(!hub.manifest("remoteplug").unwrap().1);
                assert!(!scratch.plugins().join("remoteplug/.git").exists());
                scratch.nothing_staged();
            },
        )
    });
}

#[test]
fn git_problems_are_explained_in_plain_words() {
    run_gtk_test(|| {
        with_dialog(
            "gitmissing",
            |scratch| scratch.options().git_program(scratch.0.join("no-such-git")),
            |_widgets, scratch, dialog| {
                run_check(dialog, URL);
                assert_eq!(dialog.state_phase(), Phase::Invalid);
                let errors = error_texts(dialog);
                assert_eq!(errors.len(), 1, "{errors:?}");
                assert!(
                    errors[0].contains("need git") && errors[0].contains("choose a folder"),
                    "{errors:?}"
                );
                assert!(!dialog.install_button().is_sensitive());
                scratch.nothing_staged();
            },
        );
        with_dialog(
            "gitfail",
            |scratch| {
                let git = scratch.script(
                    "git-fail",
                    &format!("echo 'fatal: repository {URL} not found' >&2\nexit 128"),
                );
                scratch.options().git_program(git)
            },
            |_widgets, scratch, dialog| {
                run_check(dialog, URL);
                assert_eq!(dialog.state_phase(), Phase::Invalid);
                let errors = error_texts(dialog).join("\n");
                assert!(errors.contains("exit status 128"), "{errors}");
                assert!(errors.contains("Git said: fatal: repository"), "{errors}");
                assert!(!errors.contains("GitFailed"), "{errors}");
                assert!(
                    !errors.contains("example.invalid"),
                    "address stays out: {errors}"
                );
                scratch.nothing_staged();
            },
        );
    });
}

#[test]
fn cancel_and_closing_stop_a_running_clone_and_leave_nothing_behind() {
    run_gtk_test(|| {
        // The Cancel button keeps the dialog open.
        with_dialog(
            "cancel",
            |scratch| scratch.options().git_program(scratch.hanging_git()),
            |_widgets, scratch, dialog| {
                dialog.entry().set_text(URL);
                dialog.check_button().emit_clicked();
                wait_until("the clone to start", || {
                    std::fs::read_to_string(scratch.pid_file())
                        .is_ok_and(|text| !text.trim().is_empty())
                });
                dialog.cancel_button().emit_clicked();
                wait_until("the cancel", || dialog.state_phase() != Phase::Checking);
                assert_eq!(dialog.state_phase(), Phase::Idle);
                assert!(
                    dialog.error_labels().is_empty(),
                    "cancelling is not an error"
                );
                assert!(dialog.window().is_visible());
                let pid = std::fs::read_to_string(scratch.pid_file()).unwrap();
                wait_until("the grandchild to die", || process_gone(pid.trim()));
                scratch.nothing_staged();
            },
        );
        // Closing the dialog cancels, kills the process group, joins and cleans up.
        with_dialog(
            "close",
            |scratch| scratch.options().git_program(scratch.hanging_git()),
            |widgets, scratch, dialog| {
                dialog.entry().set_text(URL);
                dialog.check_button().emit_clicked();
                wait_until("the clone to start", || {
                    std::fs::read_to_string(scratch.pid_file())
                        .is_ok_and(|text| !text.trim().is_empty())
                });
                let pid = std::fs::read_to_string(scratch.pid_file()).unwrap();
                let started = std::time::Instant::now();
                dialog.window().close();
                assert!(
                    started.elapsed() < Duration::from_millis(500),
                    "closing never waits for the worker: {:?}",
                    started.elapsed()
                );
                wait_until("the grandchild to die", || process_gone(pid.trim()));
                wait_until("the staging directory to go", || {
                    std::fs::read_dir(scratch.tmp()).unwrap().next().is_none()
                });
                scratch.nothing_staged();
                assert!(widgets.binding_ui.plugins.try_begin_install().is_some());
            },
        );
    });
}

#[test]
fn installs_are_serialised() {
    run_gtk_test(|| {
        with_dialog(
            "serial",
            |scratch| scratch.options(),
            |widgets, scratch, dialog| {
                let hub = &widgets.binding_ui.plugins;
                let project = scratch.project("proj", "serial", "1.0.0");
                run_check(dialog, &project.to_string_lossy());
                assert_eq!(dialog.state_phase(), Phase::Preview);
                let slot = hub.try_begin_install().expect("free slot");
                assert!(hub.try_begin_install().is_none(), "one install at a time");
                dialog.install_button().emit_clicked();
                assert_eq!(dialog.state_phase(), Phase::Preview, "refused");
                assert!(error_texts(dialog)[0].contains("already running"));
                assert!(scratch.installed_names().is_empty());
                drop(slot);
                dialog.install_button().emit_clicked();
                // A second press while this one runs is refused by the busy state.
                dialog.install_button().emit_clicked();
                wait_until("the install", || !dialog.window().is_visible());
                assert_eq!(scratch.installed_names(), ["serial"]);
                assert!(hub.try_begin_install().is_some(), "the slot is free again");
            },
        )
    });
}

#[test]
fn every_error_variant_has_a_clear_sentence() {
    use std::time::Duration;
    let stage = [
        StageError::Source("only https:// addresses are accepted".into()),
        StageError::Invalid(vec![
            "manifest.json: bad".into(),
            "run: not executable".into(),
        ]),
        StageError::GitMissing("cannot start `git`: not found".into()),
        StageError::GitFailed {
            status: Some(128),
            diagnostics: "fatal: nope\n\x1b[31mred\x1b[0m".into(),
        },
        StageError::GitFailed {
            status: None,
            diagnostics: String::new(),
        },
        StageError::DeadlineExceeded(Duration::from_secs(60)),
        StageError::TooLarge { limit: 32 << 20 },
        StageError::BadCommit("odd output".into()),
        StageError::Io("disk full".into()),
    ];
    for error in &stage {
        let lines = plugin_add_ui::stage_messages(error);
        assert!(!lines.is_empty(), "{error:?}");
        for line in &lines {
            assert!(line.chars().next().unwrap().is_uppercase(), "{line}");
            assert!(line.ends_with(['.', ':']), "{line}");
            assert!(
                !line.contains("GitFailed") && !line.contains("Invalid(") && !line.contains('\x1b'),
                "{line}"
            );
        }
    }
    assert!(plugin_add_ui::stage_messages(&StageError::Cancelled).is_empty());
    let many = StageError::Invalid((0..20).map(|n| format!("problem {n}")).collect());
    let lines = plugin_add_ui::stage_messages(&many);
    assert_eq!(lines.len(), 10, "header, 8 problems, and a count");
    assert_eq!(lines.last().unwrap(), "And 12 more problems.");
    let install = [
        InstallError::AlreadyInstalled {
            id: "x".into(),
            installed_version: None,
        },
        InstallError::InvalidState("plugins.json: bad".into()),
        InstallError::Io("cannot install the plugin: denied".into()),
    ];
    for error in &install {
        let lines = plugin_add_ui::install_messages(error);
        assert_eq!(lines.len(), 1, "{error:?}");
        assert!(lines[0].ends_with('.'), "{}", lines[0]);
        assert!(!lines[0].contains("AlreadyInstalled") && !lines[0].contains("Io("));
    }
}

#[test]
fn closing_during_an_install_lets_it_finish_then_rescans_and_reports() {
    run_gtk_test(|| {
        with_dialog(
            "closeinstall",
            |scratch| scratch.options(),
            |widgets, scratch, dialog| {
                let hub = &widgets.binding_ui.plugins;
                let project = scratch.project("proj", "late", "1.0.0");
                run_check(dialog, &project.to_string_lossy());
                assert_eq!(dialog.state_phase(), Phase::Preview);
                hub.set_install_delay(Duration::from_millis(600));
                dialog.install_button().emit_clicked();
                assert_eq!(dialog.state_phase(), Phase::Installing);
                let started = std::time::Instant::now();
                dialog.window().close();
                assert!(
                    started.elapsed() < Duration::from_millis(300),
                    "closing never waits: {:?}",
                    started.elapsed()
                );
                assert!(hub.manifest("late").is_none(), "not installed yet");
                assert!(
                    hub.try_begin_install().is_none(),
                    "the install still holds the slot"
                );
                wait_until("the detached install to be reported", || {
                    hub.manifest("late").is_some()
                });
                assert!(!hub.manifest("late").unwrap().1, "installed disabled");
                assert_eq!(
                    widgets.binding_ui.status.text().as_str(),
                    "Installed Fixture 1.0.0. It is disabled: enable it in the Plugins section."
                );
                assert!(plugins_json(scratch).is_none());
                assert!(!scratch.plugins().join("late/ran.marker").exists());
                scratch.nothing_staged();
                assert!(hub.try_begin_install().is_some(), "the slot is free again");
            },
        )
    });
}

fn press_escape(window: &gtk::Window) -> bool {
    let controllers = window.observe_controllers();
    let escape = (0..controllers.n_items())
        .filter_map(|i| controllers.item(i))
        .find_map(|c| c.downcast::<gtk::EventControllerKey>().ok())
        .expect("key controller");
    escape.emit_by_name::<bool>(
        "key-pressed",
        &[
            &gtk::gdk::Key::Escape.into_glib(),
            &0u32,
            &gtk::gdk::ModifierType::empty(),
        ],
    )
}

fn pump_until_freed(weak: &glib::WeakRef<gtk::Window>) {
    wait_until("the dialog window to be freed", || weak.upgrade().is_none());
}

#[test]
fn closing_with_a_preview_releases_the_staged_copy_and_frees_the_window() {
    run_gtk_test(|| {
        catalog_fixture("add-prel", |widgets, _l, _r| {
            let scratch = Scratch::new("prel");
            let hub = &widgets.binding_ui.plugins;
            hub.load(scratch.plugins());
            hub.use_stage_options(scratch.options());
            let project = scratch.project("proj", "rel", "1.0.0");
            let dialog = open_dialog(widgets);
            run_check(&dialog, &project.to_string_lossy());
            assert_eq!(dialog.state_phase(), Phase::Preview);
            assert!(std::fs::read_dir(scratch.tmp()).unwrap().next().is_some());
            let window = dialog.window();
            drop(dialog);
            let weak = window.downgrade();
            window.close();
            drop(window);
            scratch.nothing_staged();
            pump_until_freed(&weak);
            assert!(hub.add_dialog().is_none(), "the state is freed");
        });
    });
}

#[test]
fn closing_an_idle_dialog_frees_its_window() {
    run_gtk_test(|| {
        catalog_fixture("add-idle", |widgets, _l, _r| {
            let scratch = Scratch::new("idle");
            widgets.binding_ui.plugins.load(scratch.plugins());
            let dialog = open_dialog(widgets);
            let window = dialog.window();
            drop(dialog);
            let weak = window.downgrade();
            window.close();
            drop(window);
            pump_until_freed(&weak);
        });
    });
}

#[test]
fn escape_with_a_preview_releases_the_staged_copy() {
    run_gtk_test(|| {
        with_dialog(
            "escprev",
            |scratch| scratch.options(),
            |_w, scratch, dialog| {
                let project = scratch.project("proj", "esc", "1.0.0");
                run_check(dialog, &project.to_string_lossy());
                assert_eq!(dialog.state_phase(), Phase::Preview);
                assert!(press_escape(&dialog.window()));
                assert!(!dialog.window().is_visible());
                scratch.nothing_staged();
            },
        )
    });
}

#[test]
fn a_failed_replace_then_close_releases_the_staged_copy() {
    run_gtk_test(|| {
        with_dialog(
            "prep",
            |s| s.options(),
            |w, scratch, dialog| {
                let hub = &w.binding_ui.plugins;
                installed(scratch, "old", "dup", "1.0.0");
                hub.rescan();
                std::fs::write(scratch.plugins().join("plugins.json"), "{not json").unwrap();
                let before = std::fs::read(scratch.plugins().join("dup/manifest.json")).unwrap();
                let project = scratch.project("new", "dup", "2.0.0");
                run_check(dialog, &project.to_string_lossy());
                assert_eq!(dialog.state_phase(), Phase::Replace);
                dialog.install_button().emit_clicked();
                dialog.install_button().emit_clicked();
                wait_until("the verdict", || dialog.state_phase() != Phase::Installing);
                assert!(error_texts(dialog)
                    .iter()
                    .any(|e| e.contains("plugins.json")));
                assert_eq!(
                    std::fs::read(scratch.plugins().join("dup/manifest.json")).unwrap(),
                    before,
                    "untouched"
                );
                assert_eq!(
                    std::fs::read_to_string(scratch.plugins().join("plugins.json")).unwrap(),
                    "{not json"
                );
                dialog.window().close();
                scratch.nothing_staged();
            },
        )
    });
}

#[test]
fn the_installed_bytes_are_the_previewed_ones() {
    run_gtk_test(|| {
        with_dialog(
            "toctou",
            |s| s.options(),
            |_w, scratch, dialog| {
                let project = scratch.project("proj", "tt", "1.0.0");
                run_check(dialog, &project.to_string_lossy());
                assert_eq!(dialog.state_phase(), Phase::Preview);
                std::fs::write(project.join("run"), "#!/bin/sh\necho EVIL\n").unwrap();
                std::fs::write(project.join("lib/new.py"), "evil").unwrap();
                dialog.install_button().emit_clicked();
                wait_until("the install", || !dialog.window().is_visible());
                let run = std::fs::read_to_string(scratch.plugins().join("tt/run")).unwrap();
                assert!(run.contains("ran.marker") && !run.contains("EVIL"));
                assert!(!scratch.plugins().join("tt/lib/new.py").exists());
            },
        )
    });
}

#[test]
fn a_stale_check_or_an_edit_never_leaves_install_enabled() {
    run_gtk_test(|| {
        with_dialog(
            "pstale",
            |scratch| {
                let project = scratch.project("remote", "stale", "1.0.0");
                scratch.options().git_program(scratch.git(&project, 1))
            },
            |_w, scratch, dialog| {
                dialog.entry().set_text(URL);
                dialog.check_button().emit_clicked();
                dialog.entry().set_text("https://example.invalid/other.git");
                wait_until("the check", || dialog.state_phase() != Phase::Checking);
                assert_ne!(dialog.state_phase(), Phase::Preview);
                assert!(!dialog.install_button().is_sensitive());
                dialog.check_button().emit_clicked();
                dialog.check_button().emit_clicked();
                wait_until("the second check", || {
                    dialog.state_phase() != Phase::Checking
                });
                for edited in [
                    "https://example.invalid/other.git/",
                    " https://example.invalid/other.git",
                ] {
                    dialog.entry().set_text(edited);
                    assert!(!dialog.install_button().is_sensitive(), "{edited:?}");
                }
                scratch.nothing_staged();
            },
        )
    });
}

#[test]
fn hostile_git_output_is_shown_literally_without_invisible_or_control_characters() {
    run_gtk_test(|| {
        with_dialog(
            "pstderr",
            |scratch| {
                let git = scratch.script(
                    "git-evil",
                    "printf 'fatal: \\033[31mred\\033[0m \\342\\200\\256RLO \\342\\200\\213ZW <b>bold</b> &amp; <span foreground=\"red\">x</span>\\n' >&2\nhead -c 5000 /dev/zero | tr '\\0' 'A' >&2\nprintf '\\nline3 \\r\\n\\007bell\\nline4\\n' >&2\nexit 128",
                );
                scratch.options().git_program(git)
            },
            |_w, scratch, dialog| {
                run_check(dialog, URL);
                for label in dialog.error_labels() {
                    assert!(!label.uses_markup());
                    assert_eq!(label.accessible_role(), gtk::AccessibleRole::Alert);
                }
                let joined = error_texts(dialog).join("\n");
                assert!(
                    joined.contains("<b>bold</b>"),
                    "markup shown literally: {joined}"
                );
                assert!(joined.contains("RLO") && joined.contains("ZW"), "{joined}");
                assert!(
                    !joined.chars().any(|c| c.is_control() && c != '\n'),
                    "{joined}"
                );
                assert!(
                    !joined.chars().any(|c| matches!(c, '\u{202e}' | '\u{200b}')),
                    "bidi and zero-width characters are stripped: {joined:?}"
                );
                for line in error_texts(dialog) {
                    assert!(line.chars().count() < 600);
                }
                scratch.nothing_staged();
            },
        )
    });
}

#[test]
fn manifest_text_is_literal_and_stripped_of_invisible_characters() {
    run_gtk_test(|| {
        with_dialog(
            "pmarkup",
            |s| s.options(),
            |_w, scratch, dialog| {
                let project = scratch.project("proj", "mk", "1.0.0");
                let mut m: serde_json::Value = serde_json::from_str(
                    &std::fs::read_to_string(project.join("manifest.json")).unwrap(),
                )
                .unwrap();
                m["name"] = "<b>N</b>\u{202e}evil\u{200b}&amp;".into();
                m["actions"][0]["name"] = "<i>act</i> & more".into();
                m["actions"][0]["settings"][0]["label"] = "<u>Chan</u>".into();
                std::fs::write(project.join("manifest.json"), m.to_string()).unwrap();
                run_check(dialog, &project.to_string_lossy());
                assert_eq!(
                    dialog.state_phase(),
                    Phase::Preview,
                    "{:?}",
                    error_texts(dialog)
                );
                let mut all = labels_in(dialog.card().upcast_ref());
                all.extend(labels_in(dialog.pinned().upcast_ref()));
                for label in &all {
                    assert!(!label.uses_markup(), "{}", label.text());
                }
                let texts = card_texts(dialog);
                assert!(
                    texts.iter().any(|t| t.contains("<b>N</b>evil&amp;")),
                    "{texts:?}"
                );
                assert!(
                    texts
                        .iter()
                        .all(|t| !t.contains('\u{202e}') && !t.contains('\u{200b}')),
                    "{texts:?}"
                );
            },
        )
    });
}

#[test]
fn closing_the_main_window_during_a_check_cancels_it_and_cleans_up() {
    run_gtk_test(|| {
        with_dialog(
            "parentgone",
            |scratch| scratch.options().git_program(scratch.hanging_git()),
            |_w, scratch, dialog| {
                dialog.entry().set_text(URL);
                dialog.check_button().emit_clicked();
                wait_until("the clone to start", || {
                    std::fs::read_to_string(scratch.pid_file())
                        .is_ok_and(|text| !text.trim().is_empty())
                });
                let pid = std::fs::read_to_string(scratch.pid_file()).unwrap();
                let parent = dialog.window().transient_for().expect("parent");
                let started = std::time::Instant::now();
                parent.close();
                assert!(started.elapsed() < Duration::from_millis(500));
                wait_until("git's grandchild to die", || process_gone(pid.trim()));
                scratch.nothing_staged();
                assert!(!dialog.window().is_visible());
            },
        )
    });
}

#[test]
fn an_install_that_fails_after_the_dialog_closed_is_reported_on_the_status_line() {
    run_gtk_test(|| {
        with_dialog(
            "detachfail",
            |s| s.options(),
            |w, scratch, dialog| {
                let hub = &w.binding_ui.plugins;
                installed(scratch, "old", "dup", "1.0.0");
                hub.rescan();
                std::fs::write(scratch.plugins().join("plugins.json"), "{not json").unwrap();
                let project = scratch.project("new", "dup", "2.0.0");
                run_check(dialog, &project.to_string_lossy());
                assert_eq!(dialog.state_phase(), Phase::Replace);
                hub.set_install_delay(Duration::from_millis(300));
                dialog.install_button().emit_clicked();
                dialog.window().close();
                wait_until("the failure to be reported", || {
                    w.binding_ui.status.text().contains("failed")
                });
                let status = w.binding_ui.status.text();
                assert!(status.starts_with("Installing Fixture failed:"), "{status}");
                assert!(status.contains("plugins.json"), "{status}");
                assert!(w.binding_ui.status.has_css_class("feedback-error"));
                assert!(hub.try_begin_install().is_some());
                scratch.nothing_staged();
            },
        )
    });
}

#[test]
fn repeated_refused_installs_show_one_message() {
    run_gtk_test(|| {
        with_dialog(
            "refuse",
            |s| s.options(),
            |w, scratch, dialog| {
                let hub = &w.binding_ui.plugins;
                let project = scratch.project("proj", "ref", "1.0.0");
                run_check(dialog, &project.to_string_lossy());
                let slot = hub.try_begin_install().expect("slot");
                for _ in 0..3 {
                    dialog.install_button().emit_clicked();
                }
                assert_eq!(error_texts(dialog).len(), 1, "{:?}", error_texts(dialog));
                drop(slot);
            },
        )
    });
}

#[test]
fn failures_say_what_phase_they_happened_in() {
    run_gtk_test(|| {
        with_dialog(
            "phases",
            |s| s.options(),
            |w, scratch, dialog| {
                let hub = &w.binding_ui.plugins;
                let project = scratch.project("proj", "ph", "1.0.0");
                let text = project.to_string_lossy().into_owned();
                // A worker that panics during the check.
                hub.set_worker_panics(true);
                dialog.entry().set_text(&text);
                dialog.check_button().emit_clicked();
                wait_until("the verdict", || dialog.state_phase() != Phase::Checking);
                hub.set_worker_panics(false);
                assert_eq!(
                    error_texts(dialog),
                    ["The plugin check stopped unexpectedly. Nothing was installed."]
                );
                // A thread that cannot start during the check.
                hub.set_spawn_fails(true);
                dialog.check_button().emit_clicked();
                hub.set_spawn_fails(false);
                assert_eq!(dialog.state_phase(), Phase::Invalid);
                assert!(error_texts(dialog)[0].starts_with("Could not start the check"));
                // A worker that panics during the install.
                run_check(dialog, &text);
                assert_eq!(dialog.state_phase(), Phase::Preview);
                hub.set_worker_panics(true);
                dialog.install_button().emit_clicked();
                wait_until("the verdict", || dialog.state_phase() != Phase::Installing);
                hub.set_worker_panics(false);
                let errors = error_texts(dialog).join(" ");
                assert!(
                    errors.contains("installation stopped unexpectedly"),
                    "{errors}"
                );
                assert!(errors.contains("Check the plugins folder"), "{errors}");
                assert!(!errors.contains("Nothing was installed"), "{errors}");
                assert!(hub.try_begin_install().is_some(), "the slot is free");
                // A thread that cannot start during the install.
                run_check(dialog, &text);
                hub.set_spawn_fails(true);
                dialog.install_button().emit_clicked();
                hub.set_spawn_fails(false);
                let errors = error_texts(dialog).join(" ");
                assert!(
                    errors.starts_with("Could not start the installation"),
                    "{errors}"
                );
                assert!(!errors.contains("check"), "{errors}");
                assert!(scratch.installed_names().is_empty());
                scratch.nothing_staged();
            },
        )
    });
}
