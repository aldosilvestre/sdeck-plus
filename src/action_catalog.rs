//! Copy action templates onto keys without executing them or moving artwork.
use crate::*;
use feature_editor::choice::Choice;

#[derive(Clone, Copy, glib::Boxed)]
#[boxed_type(name = "SdeckCatalogAction")]
struct CatalogAction {
    picker: u32,
    feature: Option<Choice>,
}

impl CatalogAction {
    fn ready(self) -> bool {
        matches!(
            self.feature,
            Some(
                Choice::PlayPause
                    | Choice::Stop
                    | Choice::Next
                    | Choice::Previous
                    | Choice::VolumeUp
                    | Choice::VolumeDown
                    | Choice::Mute
                    | Choice::StopAudio
                    | Choice::NextPage
                    | Choice::PreviousPage
                    | Choice::ParentFolder
                    | Choice::CancelTimer
                    | Choice::Sleep
                    | Choice::Wake
                    | Choice::Recording
                    | Choice::Streaming
                    | Choice::StartRecording
                    | Choice::StopRecording
                    | Choice::PauseRecording
                    | Choice::ResumeRecording
                    | Choice::StartStreaming
                    | Choice::StopStreaming
            )
        )
    }
}

pub(crate) fn connect(
    keys: Rc<Vec<gtk::Widget>>,
    ui: &BindingUi,
    layers: &Rc<RefCell<LayerStore>>,
    heading: &Label,
    save: &Button,
) -> GtkBox {
    let catalog = GtkBox::new(Orientation::Vertical, 8);
    catalog.append(&ui::helper(
        "Drag an action onto a key or a dial, or click to assign it to the selected control.",
    ));
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search actions"));
    catalog.append(&search);
    let list = GtkBox::new(Orientation::Vertical, 6);
    list.add_css_class("catalog-list");
    let assign: Assign = {
        let ui = ui.clone();
        let layers = layers.clone();
        let heading = heading.clone();
        let keys: Vec<_> = keys.iter().map(ObjectExt::downgrade).collect();
        let save = save.downgrade();
        let confirm_ui = ui.clone();
        let confirm_keys = keys.clone();
        let apply: Assign = Rc::new(move |action, target| {
            let (kind, index, widget) = match target {
                Target::Key(index) => (ControlKind::Button, index, index),
                Target::Dial(index, _) => (ControlKind::Encoder, index, 9 + index),
            };
            let number = (index + 1) as u8;
            if ui.select_control(kind, Some(number), true).is_err() {
                return false;
            }
            if let Target::Dial(_, event) = target {
                select_string_or_first(&ui.event_model, &ui.event_picker, event_label(event));
            }
            let selected: Vec<_> = keys.iter().filter_map(glib::WeakRef::upgrade).collect();
            set_selected_control(&selected, widget);
            heading.set_text(&control_heading(kind, Some(number)));
            ui.load_selected_binding(&layers);
            ui.clear_action_fields();
            ui.action_picker.set_selected(action.picker);
            if let Some(choice) = action.feature {
                ui.feature_editor.select_catalog_choice(choice);
            }
            ui.update_action_visibility();
            if action.ready() {
                if let Some(save) = save.upgrade() {
                    save.emit_clicked();
                }
            } else {
                ui.status.set_text("Complete the action settings below, then Save binding. The previous action stays active until saved.");
                ui.status.add_css_class("feedback-warning");
            }
            true
        });
        Rc::new(move |action, target| {
            // Choosing an action replaces the draft already on that control and
            // event; only a draft elsewhere is at stake, so that one asks first.
            let number = |index: usize| Some((index + 1) as u8);
            let same_control = match target {
                Target::Key(index) => {
                    confirm_ui.control_kind() == ControlKind::Button
                        && confirm_ui.control_number() == number(index)
                }
                Target::Dial(index, event) => {
                    confirm_ui.control_kind() == ControlKind::Encoder
                        && confirm_ui.control_number() == number(index)
                        && confirm_ui.event_kind() == event
                }
            };
            if confirm_ui.is_dirty() && !same_control {
                let widget = match target {
                    Target::Key(index) => index,
                    Target::Dial(index, _) => 9 + index,
                };
                let anchor = confirm_keys
                    .get(widget)
                    .and_then(glib::WeakRef::upgrade)
                    .and_then(|control| control.downcast::<Button>().ok());
                if let Some(anchor) = anchor {
                    let apply = apply.clone();
                    confirm_discard(
                        &anchor,
                        Rc::new(move || {
                            apply(action, target);
                        }),
                    );
                    return true;
                }
            }
            apply(action, target)
        })
    };
    let mut sections = Vec::new();
    let basic: Vec<_> = [
        ("Go to profile or page", 0),
        ("Run command", 1),
        ("Open application", 2),
        ("Shortcut", 3),
        ("Send named event", 4),
    ]
    .into_iter()
    .map(|(label, picker)| {
        (
            label,
            BindingIcon::for_action_type(picker),
            CatalogAction {
                picker,
                feature: None,
            },
        )
    })
    .collect();
    sections.push(Section::new(
        &list,
        "Basic",
        BindingIcon::for_action_type(1),
        basic,
        true,
        ui,
        &assign,
    ));
    for (category, label) in ["System", "Multimedia", "Organization", "Device", "OBS"]
        .into_iter()
        .enumerate()
    {
        let entries: Vec<_> = Choice::ALL
            .into_iter()
            .filter(|choice| {
                choice.category() == category as u32
                    && !matches!(
                        choice,
                        Choice::NextTouchscreenPage | Choice::PreviousTouchscreenPage
                    )
            })
            .map(|choice| {
                (
                    choice.label(),
                    choice.binding_icon(),
                    CatalogAction {
                        picker: 5,
                        feature: Some(choice),
                    },
                )
            })
            .collect();
        sections.push(Section::new(
            &list,
            label,
            Choice::category_icon(category as u32),
            entries,
            false,
            ui,
            &assign,
        ));
    }
    let empty = ui::helper("No actions match your search.");
    empty.add_css_class("catalog-empty");
    empty.set_visible(false);
    list.append(&empty);
    search.connect_search_changed(move |search| {
        let query = search.text().to_lowercase();
        // Every section must be filtered, so this cannot short-circuit.
        let mut found = false;
        for section in &sections {
            found |= section.filter(&query);
        }
        empty.set_visible(!found);
    });
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(320)
        .max_content_height(640)
        .propagate_natural_height(true)
        .child(&list)
        .build();
    catalog.append(&scroll);
    for (index, key) in keys.iter().take(8).enumerate() {
        // A distinct boxed GType keeps COPY assignment separate from MOVE key swapping.
        let target = gtk::DropTarget::new(CatalogAction::static_type(), gtk::gdk::DragAction::COPY);
        let weak = key.downgrade();
        target.connect_enter(move |_, _, _| {
            if let Some(key) = weak.upgrade() {
                key.add_css_class("action-drop-target");
            }
            gtk::gdk::DragAction::COPY
        });
        let weak = key.downgrade();
        target.connect_leave(move |_| {
            if let Some(key) = weak.upgrade() {
                key.remove_css_class("action-drop-target");
            }
        });
        let weak = key.downgrade();
        let assign = assign.clone();
        target.connect_drop(move |_, value, _, _| {
            if let Some(key) = weak.upgrade() {
                key.remove_css_class("action-drop-target");
            }
            value
                .get::<CatalogAction>()
                .is_ok_and(|action| assign(action, Target::Key(index)))
        });
        key.add_controller(target);
    }
    for (index, dial) in keys.iter().skip(9).take(4).enumerate() {
        let target = gtk::DropTarget::new(CatalogAction::static_type(), gtk::gdk::DragAction::COPY);
        let weak = dial.downgrade();
        target.connect_enter(move |_, _, _| {
            if let Some(dial) = weak.upgrade() {
                dial.add_css_class("action-drop-target");
            }
            gtk::gdk::DragAction::COPY
        });
        let weak = dial.downgrade();
        target.connect_leave(move |_| {
            if let Some(dial) = weak.upgrade() {
                dial.remove_css_class("action-drop-target");
            }
        });
        let weak = dial.downgrade();
        let assign = assign.clone();
        target.connect_drop(move |_, value, _, _| {
            let Some(dial) = weak.upgrade() else {
                return false;
            };
            dial.remove_css_class("action-drop-target");
            let Ok(action) = value.get::<CatalogAction>() else {
                return false;
            };
            // A dial has three events, so ask which one this action belongs to.
            let assign = assign.clone();
            choose_event(&dial, move |event| {
                assign(action, Target::Dial(index, event));
            });
            true
        });
        dial.add_controller(target);
    }
    catalog
}

/// Where a catalog action is assigned: a key, or one event of a dial.
#[derive(Clone, Copy)]
enum Target {
    Key(usize),
    Dial(usize, EventKind),
}

type Assign = Rc<dyn Fn(CatalogAction, Target) -> bool>;

/// One collapsible category: a header toggle and its action rows.
struct Section {
    container: GtkBox,
    header: Button,
    revealer: gtk::Revealer,
    chevron: gtk::Image,
    rows: Vec<(Button, &'static str)>,
    expanded: Rc<Cell<bool>>,
}

impl Section {
    fn new(
        list: &GtkBox,
        title: &'static str,
        icon: BindingIcon,
        entries: Vec<(&'static str, BindingIcon, CatalogAction)>,
        expanded: bool,
        ui: &BindingUi,
        assign: &Assign,
    ) -> Self {
        let container = GtkBox::new(Orientation::Vertical, 2);
        container.add_css_class("catalog-section");
        let header = Button::new();
        header.add_css_class("flat");
        header.add_css_class("catalog-category");
        header.set_tooltip_text(Some(&format!("Show or hide {title} actions")));
        let line = GtkBox::new(Orientation::Horizontal, 8);
        line.append(&binding_icon::binding_icon(icon));
        let name = Label::new(Some(title));
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.add_css_class("catalog-category-name");
        line.append(&name);
        let count = Label::new(Some(&entries.len().to_string()));
        count.add_css_class("catalog-count");
        line.append(&count);
        let chevron = gtk::Image::from_icon_name("pan-end-symbolic");
        line.append(&chevron);
        header.set_child(Some(&line));
        container.append(&header);
        let body = GtkBox::new(Orientation::Vertical, 2);
        body.add_css_class("catalog-rows");
        let rows: Vec<_> = entries
            .into_iter()
            .map(|(label, icon, action)| add_row(&body, label, icon, action, ui, assign.clone()))
            .collect();
        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .child(&body)
            .build();
        container.append(&revealer);
        list.append(&container);
        let section = Self {
            container,
            header,
            revealer,
            chevron,
            rows,
            expanded: Rc::new(Cell::new(expanded)),
        };
        section.apply(expanded);
        let revealer = section.revealer.downgrade();
        let chevron = section.chevron.downgrade();
        let state = section.expanded.clone();
        section.header.connect_clicked(move |_| {
            let open = !state.get();
            state.set(open);
            if let (Some(revealer), Some(chevron)) = (revealer.upgrade(), chevron.upgrade()) {
                revealer.set_reveal_child(open);
                chevron.set_icon_name(Some(chevron_name(open)));
            }
        });
        section
    }

    fn apply(&self, open: bool) {
        self.revealer.set_reveal_child(open);
        self.chevron.set_icon_name(Some(chevron_name(open)));
    }

    /// Show only matching rows; an empty query restores the user's own open/closed state.
    fn filter(&self, query: &str) -> bool {
        let mut matches = 0;
        for (row, label) in &self.rows {
            let shown = query.is_empty() || label.to_lowercase().contains(query);
            row.set_visible(shown);
            matches += usize::from(shown);
        }
        self.container.set_visible(matches > 0);
        if query.is_empty() {
            self.apply(self.expanded.get());
        } else {
            self.apply(true);
        }
        self.header.set_sensitive(query.is_empty());
        matches > 0
    }
}

fn chevron_name(open: bool) -> &'static str {
    if open {
        "pan-down-symbolic"
    } else {
        "pan-end-symbolic"
    }
}

fn add_row(
    list: &GtkBox,
    label: &'static str,
    icon: BindingIcon,
    action: CatalogAction,
    ui: &BindingUi,
    assign: Assign,
) -> (Button, &'static str) {
    let row = row_button(
        label,
        None,
        icon,
        &format!("Drag {label} onto a key or dial, or click to assign to the selected control."),
    );
    let source = gtk::DragSource::builder()
        .actions(gtk::gdk::DragAction::COPY)
        .build();
    source.connect_prepare(move |_, _, _| {
        Some(gtk::gdk::ContentProvider::for_value(&action.to_value()))
    });
    let weak = row.downgrade();
    source.connect_drag_begin(move |source, _| {
        if let Some(row) = weak.upgrade() {
            source.set_icon(Some(&gtk::WidgetPaintable::new(Some(&row))), 16, 16);
        }
    });
    row.add_controller(source);
    let ui = ui.clone();
    row.connect_clicked(move |_| {
        let number = ui
            .control_number()
            .and_then(|n| n.checked_sub(1))
            .map(usize::from);
        match (ui.control_kind(), number) {
            (ControlKind::Button, Some(index)) if index < 8 => {
                assign(action, Target::Key(index));
            }
            (ControlKind::Encoder, Some(index)) if index < 4 => {
                assign(action, Target::Dial(index, ui.event_kind()));
            }
            _ => ui.error("Select a key or a dial before assigning an action."),
        }
    });
    list.append(&row);
    (row, label)
}

/// A frameless, draggable catalog row: icon tile, title, optional subtitle and grip.
pub(crate) fn row_button(
    title: &str,
    subtitle: Option<&str>,
    icon: BindingIcon,
    tooltip: &str,
) -> Button {
    let row = Button::new();
    row.add_css_class("flat");
    row.add_css_class("catalog-action");
    row.set_cursor_from_name(Some("grab"));
    row.set_tooltip_text(Some(tooltip));
    let content = GtkBox::new(Orientation::Horizontal, 10);
    let tile = GtkBox::new(Orientation::Horizontal, 0);
    tile.add_css_class("catalog-icon");
    tile.set_valign(gtk::Align::Center);
    tile.append(&binding_icon::binding_icon(icon));
    content.append(&tile);
    let text = GtkBox::new(Orientation::Vertical, 1);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let caption = Label::new(Some(title));
    caption.set_xalign(0.0);
    caption.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&caption);
    if let Some(subtitle) = subtitle {
        let detail = Label::new(Some(subtitle));
        detail.set_xalign(0.0);
        detail.add_css_class("catalog-subtitle");
        detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&detail);
    }
    content.append(&text);
    let grip = gtk::Image::from_icon_name("list-drag-handle-symbolic");
    grip.add_css_class("catalog-grip");
    content.append(&grip);
    row.set_child(Some(&content));
    row
}

/// Offer the dial's three events next to it and report the one chosen.
fn choose_event(anchor: &gtk::Widget, pick: impl Fn(EventKind) + Clone + 'static) {
    let popover = gtk::Popover::new();
    popover.set_parent(anchor);
    popover.add_css_class("dial-event-choice");
    let choices = GtkBox::new(Orientation::Vertical, 2);
    for (label, event) in [
        ("Turn clockwise", EventKind::EncoderClockwise),
        ("Turn counterclockwise", EventKind::EncoderCounterClockwise),
        ("Press", EventKind::EncoderPress),
    ] {
        let button = Button::with_label(label);
        button.add_css_class("flat");
        let weak = popover.downgrade();
        let pick = pick.clone();
        button.connect_clicked(move |_| {
            if let Some(popover) = weak.upgrade() {
                popover.popdown();
            }
            pick(event);
        });
        choices.append(&button);
    }
    popover.set_child(Some(&choices));
    // A popover parented to a widget must be unparented once it closes.
    popover.connect_closed(|popover| popover.unparent());
    popover.popup();
}
