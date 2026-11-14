//! The button system of the approved artboards: flat buttons are frameless, destructive
//! buttons read as danger, icon-only buttons are 34px squares, and the page bar and the
//! catalog filter chips follow the same rules.
use super::ui_refresh_tests::{fixture, Fixture};
use super::*;

fn find(fixture: &Fixture, caption: &str) -> Button {
    let mut buttons = Vec::new();
    collect_layout_buttons(fixture.widgets.root.upcast_ref(), &mut buttons);
    buttons
        .into_iter()
        .find(|button| button_caption(button).as_deref() == Some(caption))
        .unwrap_or_else(|| panic!("missing production button: {caption}"))
}

/// Border-box size (GTK `width()`/`height()` report the content box only).
fn outer(widget: &impl IsA<gtk::Widget>) -> (i32, i32) {
    let widget = widget.as_ref();
    (
        widget.measure(Orientation::Horizontal, -1).0,
        widget.measure(Orientation::Vertical, -1).0,
    )
}

fn pixel(pixels: &[u8], width: i32, x: i32, y: i32) -> [u8; 3] {
    let at = (y * width + x) as usize * 4;
    // Cairo/GSK textures download as premultiplied BGRA on little-endian hosts.
    [pixels[at + 2], pixels[at + 1], pixels[at]]
}

fn render(fixture: &Fixture, widget: &impl IsA<gtk::Widget>) -> (Vec<u8>, i32, i32) {
    let widget = widget.as_ref();
    let pixels = rendered_pixels(&fixture.window, widget);
    (pixels, widget.width(), widget.height())
}

fn close(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 3)
}

#[test]
fn page_bar_navigation_is_flat_unlinked_and_34px() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("button-page-bar", |_| {}) else {
            return;
        };
        for caption in ["Previous", "Next", "Parent"] {
            let button = find(&fixture, caption);
            assert!(button.has_css_class("flat"), "{caption} is flat");
            assert!(button.has_css_class("icon-only"), "{caption} is icon-only");
            assert!(
                !button.parent().unwrap().has_css_class("linked"),
                "{caption} is not part of a linked group"
            );
            assert_eq!(outer(&button), (34, 34), "{caption}");
        }
        for caption in ["New page", "Rename page", "Delete page"] {
            let button = find(&fixture, caption);
            assert!(button.has_css_class("flat"), "{caption} is flat");
            assert_eq!(outer(&button).1, 34, "{caption} is 34px high");
        }
        assert!(find(&fixture, "Delete page").has_css_class("destructive-action"));
        let mut menus = Vec::new();
        collect_menu_buttons(fixture.widgets.root.upcast_ref(), &mut menus);
        let folders = menus
            .into_iter()
            .find(|menu| {
                menu.tooltip_text()
                    .is_some_and(|t| t.starts_with("Page folders"))
            })
            .expect("the Folders menu button");
        assert!(!folders.has_frame(), "Folders is flat");
        assert_eq!(outer(&folders).1, 34, "Folders is 34px high");
    });
}

fn collect_menu_buttons(widget: &gtk::Widget, found: &mut Vec<gtk::MenuButton>) {
    if let Some(menu) = widget.downcast_ref::<gtk::MenuButton>() {
        found.push(menu.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        child = current.next_sibling();
        collect_menu_buttons(&current, found);
    }
}

#[test]
fn flat_buttons_draw_no_border_while_default_buttons_do() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("button-flat", |_| {}) else {
            return;
        };
        let flat = find(&fixture, "Previous");
        let (pixels, width, height) = render(&fixture, &flat);
        assert!(
            close(
                pixel(&pixels, width, 0, height / 2),
                pixel(&pixels, width, 4, height / 2)
            ),
            "the flat button's edge matches its own fill: no 1px control edge"
        );
        let framed = find(&fixture, "Test action");
        assert!(!framed.has_css_class("flat"));
        let (pixels, width, height) = render(&fixture, &framed);
        assert!(
            !close(
                pixel(&pixels, width, 0, height / 2),
                pixel(&pixels, width, 4, height / 2)
            ),
            "a default button keeps its control edge"
        );
    });
}

#[test]
fn destructive_buttons_use_the_danger_style() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("button-danger", |_| {}) else {
            return;
        };
        let _ = &fixture;
        let danger = ui::button("user-trash-symbolic", "Delete profile");
        danger.add_css_class("destructive-action");
        let plain = ui::button("user-trash-symbolic", "Plain");
        let icon = ui::icon_button("go-previous-symbolic", "Icon");
        let row = GtkBox::new(Orientation::Horizontal, 20);
        row.set_valign(gtk::Align::Start);
        row.set_halign(gtk::Align::Start);
        row.append(&danger);
        row.append(&plain);
        row.append(&icon);
        let window = ApplicationWindow::builder()
            .default_width(400)
            .default_height(120)
            .build();
        install_style(&window);
        window.set_child(Some(&row));
        window.present();
        wait_for_frame(&window);
        assert_eq!(outer(&danger).1, 34);
        assert_eq!(outer(&plain).1, 34, "default buttons are 34px high");
        assert_eq!(outer(&icon), (34, 34), "icon-only buttons are 34px squares");
        let (pixels, width, height) = {
            let pixels = rendered_pixels(&window, &danger);
            (pixels, danger.width(), danger.height())
        };
        let edge = pixel(&pixels, width, 0, height / 2);
        assert!(
            edge[0] > edge[1].saturating_add(30) && edge[0] > edge[2].saturating_add(30),
            "danger border is the error colour at half strength: {edge:?}"
        );
        let fill = pixel(&pixels, width, 4, height / 2);
        assert!(fill[0] > fill[2] + 8, "danger fill is tinted: {fill:?}");
        let (pixels, width, height) = {
            let pixels = rendered_pixels(&window, &plain);
            (pixels, plain.width(), plain.height())
        };
        let edge = pixel(&pixels, width, 0, height / 2);
        assert!(
            edge[0].abs_diff(edge[1]) < 30,
            "default edge is neutral: {edge:?}"
        );
        window.close();
    });
}

/// Opt-in captures for visual review (dark and light); unset = no files written.
#[test]
fn button_system_captures_when_a_qa_directory_is_set() {
    let Some(directory) = std::env::var_os("STREAMDECK_BTN_QA_DIR") else {
        return;
    };
    let _ = directory;
    run_gtk_test(|| {
        let directory =
            std::path::PathBuf::from(std::env::var_os("STREAMDECK_BTN_QA_DIR").unwrap());
        std::fs::create_dir_all(&directory).unwrap();
        let Some(fixture) = fixture("button-captures", |_| {}) else {
            return;
        };
        crate::app_icon::register().unwrap();
        let settings = gtk::Settings::default().unwrap();
        let widgets = &fixture.widgets;
        widgets.binding_ui.mark_clean();
        key_button(&widgets.key_labels[0]).emit_clicked();
        for (suffix, theme, dark) in [("dark", "Adwaita-dark", true), ("light", "Adwaita", false)] {
            settings.set_gtk_theme_name(Some(theme));
            settings.set_gtk_application_prefer_dark_theme(dark);
            wait_for_frame(&widgets.root);
            let window: gtk::Widget = widgets.root.clone().upcast();
            super::documentation_screenshots::save(
                &directory,
                &window,
                &format!("main-window-{suffix}"),
            );
            let editor = &widgets._text_editor;
            editor.panel.set_autohide(false);
            editor.popup();
            editor.input.buffer().set_text("Play\nNow");
            super::documentation_screenshots::save(
                &directory,
                editor.panel.upcast_ref(),
                &format!("button-text-{suffix}"),
            );
            editor.panel.popdown();
            super::documentation_screenshots::menu(
                &directory,
                &widgets.obs_menu,
                &format!("obs-settings-{suffix}"),
            );
        }
    });
}
