//! The key names and aliases reference is a table with aligned columns.
use super::action_catalog_tests::catalog_fixture;
use super::*;

fn find_all(widget: &gtk::Widget, class: &str, found: &mut Vec<gtk::Widget>) {
    if widget.has_css_class(class) {
        found.push(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        find_all(&current, class, found);
        child = current.next_sibling();
    }
}

fn label_text(widget: &gtk::Widget) -> Option<String> {
    widget
        .downcast_ref::<Label>()
        .map(|label| label.text().to_string())
}

#[test]
fn key_names_and_aliases_are_a_table_with_a_header_and_aligned_columns() {
    run_gtk_test(|| {
        catalog_fixture("shortcut-table", |widgets, _layers, _requests| {
            let ui = &widgets.binding_ui;
            ui.action_picker.set_selected(3);
            ui.update_action_visibility();
            assert!(ui
                .action_guidance
                .emit_by_name::<bool>("activate-link", &[&"shortcut-reference"]));
            let dialog = current_dialog();
            let mut tables = Vec::new();
            find_all(dialog.upcast_ref(), "reference-table", &mut tables);
            assert_eq!(tables.len(), 1, "one aliases table");
            let table = &tables[0];
            let mut heads = Vec::new();
            find_all(table, "reference-head", &mut heads);
            let head = heads.first().expect("header row");
            let titles: Vec<_> = {
                let mut cells = Vec::new();
                let mut child = head.first_child();
                while let Some(cell) = child {
                    cells.extend(label_text(&cell));
                    child = cell.next_sibling();
                }
                cells
            };
            assert_eq!(titles, ["Key", "Also accepted as"]);
            let mut rows = Vec::new();
            find_all(table, "reference-row", &mut rows);
            let expected = sdeck_plus::keyboard::supported_key_names()
                .filter(|name| {
                    sdeck_plus::keyboard::supported_key_aliases().any(|(_, target)| target == *name)
                })
                .count();
            assert_eq!(rows.len(), expected, "one row per aliased key");
            let mut starts = Vec::new();
            for row in &rows {
                let key = row.first_child().expect("key cell");
                let aliases = key.next_sibling().expect("aliases cell");
                assert!(key.has_css_class("reference-key"));
                assert!(aliases.has_css_class("reference-aliases"));
                starts.push(aliases.compute_bounds(table).unwrap().x());
            }
            assert!(
                starts
                    .windows(2)
                    .all(|pair| (pair[0] - pair[1]).abs() <= 0.5),
                "aliases start in one column: {starts:?}"
            );
            if let Some(directory) = std::env::var_os("STREAMDECK_CATALOG_QA_DIR") {
                wait_for_frame(&dialog);
                // The dialog is its own toplevel: render it with its own renderer.
                let (width, height) = (dialog.width(), dialog.height());
                let snapshot = gtk::Snapshot::new();
                gtk::WidgetPaintable::new(Some(&dialog)).snapshot(
                    &snapshot,
                    f64::from(width),
                    f64::from(height),
                );
                let texture = dialog.renderer().unwrap().render_texture(
                    snapshot.to_node().unwrap(),
                    Some(&gtk::graphene::Rect::new(
                        0.0,
                        0.0,
                        width as f32,
                        height as f32,
                    )),
                );
                texture
                    .save_to_png(
                        std::path::Path::new(&directory).join("shortcut-reference-dialog.png"),
                    )
                    .unwrap();
            }
            dialog.response(ResponseType::Close);
        })
    });
}
