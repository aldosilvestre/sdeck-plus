//! Copy action templates onto keys without executing them or moving artwork.
use crate::*;
use feature_editor::choice::Choice;
use sdeck_plus::plugins::PluginControl;

#[derive(Clone, glib::Boxed)]
#[boxed_type(name = "SdeckCatalogAction")]
struct CatalogAction {
    picker: u32,
    feature: Option<Choice>,
    /// A plugin action (plugin id, action id); it always needs the form and an explicit save.
    plugin: Option<(String, String)>,
}

impl CatalogAction {
    fn ready(&self) -> bool {
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

/// What the user did to the catalog, kept across the rebuilds a model change
/// causes: the search text and which categories are open.
#[derive(Clone, Default)]
pub(crate) struct CatalogMemory {
    query: Rc<RefCell<String>>,
    /// The category chip in use; `None` is "All".
    category: Rc<RefCell<Option<String>>>,
    open: Rc<RefCell<std::collections::HashMap<String, bool>>>,
}

pub(crate) fn connect(
    keys: Rc<Vec<gtk::Widget>>,
    memory: &CatalogMemory,
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
    // One chip per category narrows the list to it; "All" lists everything.
    let chips = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .min_children_per_line(1)
        // Four to a line keeps the panel as narrow as before; the rest wrap below.
        .max_children_per_line(4)
        .column_spacing(4)
        .row_spacing(4)
        .build();
    chips.add_css_class("catalog-chips");
    let mut chip_buttons: Vec<gtk::ToggleButton> = Vec::new();
    for name in [
        "All",
        "Basic",
        "System",
        "Multimedia",
        "Organization",
        "Device",
        "OBS",
        "Plugins",
    ] {
        let chip = gtk::ToggleButton::with_label(name);
        chip.add_css_class("catalog-chip");
        if let Some(first) = chip_buttons.first() {
            chip.set_group(Some(first));
        }
        let chosen = memory.category.borrow().clone();
        chip.set_active(chosen.as_deref().map_or(name == "All", |c| c == name));
        chips.append(&chip);
        chip_buttons.push(chip);
    }
    // Plugins only get a chip while at least one is installed.
    let plugins_chip = chip_buttons[chip_buttons.len() - 1].clone();
    plugins_chip.set_visible(false);
    catalog.append(&chips);
    let list = GtkBox::new(Orientation::Vertical, 6);
    list.add_css_class("catalog-list");
    let assign: Assign = {
        let ui = ui.clone();
        let layers = layers.clone();
        let heading = heading.clone();
        let keys: Vec<_> = keys.iter().map(ObjectExt::downgrade).collect();
        // The first dial's widget follows the strip, after every key.
        let dial_offset = usize::from(ui.model.get().keys()) + 1;
        let save = save.downgrade();
        let confirm_ui = ui.clone();
        let confirm_keys = keys.clone();
        let apply: Assign = Rc::new(move |action, target| {
            if let Some((plugin, id)) = &action.plugin {
                let event = match target {
                    Target::Key(_) => EventKind::ButtonPress,
                    Target::Dial(_, event) => event,
                };
                let kind = match target {
                    Target::Key(_) => ControlKind::Button,
                    Target::Dial(..) => ControlKind::Encoder,
                };
                let control = plugin_ui::plugin_control(kind, event);
                if !ui.plugins.supports(plugin, id, control) {
                    ui.error(&format!(
                        "That plugin action cannot be assigned to a {}.",
                        plugin_ui::control_name(control).to_lowercase()
                    ));
                    return false;
                }
                ui.plugins.set_control(Some(control));
            }
            let (kind, index, widget) = match target {
                Target::Key(index) => (ControlKind::Button, index, index),
                Target::Dial(index, _) => (ControlKind::Encoder, index, dial_offset + index),
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
            if action.plugin.is_some() {
                ui.ensure_plugin_row();
            }
            ui.action_picker.set_selected(action.picker);
            if let Some(choice) = action.feature {
                ui.feature_editor.select_catalog_choice(choice);
            }
            if let Some((plugin, id)) = &action.plugin {
                ui.plugin_editor.select(plugin, id);
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
                    Target::Dial(index, _) => dial_offset + index,
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
                            apply(action.clone(), target);
                        }),
                    );
                    return true;
                }
            }
            apply(action, target)
        })
    };
    let mut sections = Vec::new();
    let wiring = Wiring {
        memory,
        ui,
        assign: &assign,
    };
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
            label.to_owned(),
            None,
            BindingIcon::for_action_type(picker),
            CatalogAction {
                picker,
                feature: None,
                plugin: None,
            },
        )
    })
    .collect();
    sections.push(Section::new(
        &list,
        "Basic",
        BindingIcon::for_action_type(1),
        basic,
        Extras::open(true),
        &wiring,
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
                    choice.label().to_owned(),
                    None,
                    choice.binding_icon(),
                    CatalogAction {
                        picker: 5,
                        feature: Some(choice),
                        plugin: None,
                    },
                )
            })
            .collect();
        let groups = if category == 1 {
            // Multimedia is long: media transport, the speakers, the microphone, then files.
            vec![
                (0, "Media"),
                (4, "Output (speakers, headphones)"),
                (8, "Input (microphone)"),
                (12, "Files"),
            ]
        } else {
            Vec::new()
        };
        sections.push(Section::new(
            &list,
            label,
            Choice::category_icon(category as u32),
            entries,
            Extras {
                groups,
                ..Extras::open(false)
            },
            &wiring,
        ));
    }
    // Every plugin is its own sidebar item below the built-in categories.
    let plugin_region = GtkBox::new(Orientation::Vertical, 6);
    list.append(&plugin_region);
    let plugin_sections: Rc<RefCell<Vec<Section>>> = Rc::default();
    // Reapplies search and chip; filled in once the section list is complete.
    let refresh_view: RefreshHook = Rc::default();
    let build_plugins = {
        let refresh_view = refresh_view.clone();
        let plugins_chip = plugins_chip.clone();
        let (region, built) = (plugin_region.downgrade(), plugin_sections.clone());
        let (memory, ui, assign) = (memory.clone(), ui.clone(), assign.clone());
        move || {
            let Some(region) = region.upgrade() else {
                return false;
            };
            populate_plugins(
                &region,
                &built,
                &Wiring {
                    memory: &memory,
                    ui: &ui,
                    assign: &assign,
                },
            );
            plugins_chip.set_visible(!built.borrow().is_empty());
            // A filter typed before the rebuild still applies to the new items.
            let refresh = refresh_view.borrow().clone();
            if let Some(refresh) = refresh {
                refresh();
            } else {
                let query = memory.query.borrow().to_lowercase();
                for section in built.borrow().iter() {
                    section.filter(&query);
                }
            }
            true
        }
    };
    build_plugins();
    ui.plugins.on_change(build_plugins);
    {
        let built = plugin_sections.clone();
        let (ui_hub, weak) = (ui.plugins.clone(), plugin_region.downgrade());
        ui.plugins.on_control(move || {
            if weak.upgrade().is_none() {
                return false;
            }
            for section in built.borrow().iter() {
                section.dim_unfit(ui_hub.control());
            }
            true
        });
    }
    let empty = ui::helper("No actions match your search.");
    empty.add_css_class("catalog-empty");
    empty.set_visible(false);
    list.append(&empty);
    let sections = Rc::new(sections);
    let refresh: Rc<dyn Fn()> = {
        let (sections, plugins) = (sections.clone(), plugin_sections.clone());
        let (query, category) = (memory.query.clone(), memory.category.clone());
        Rc::new(move || {
            let query = query.borrow().to_lowercase();
            let category = category.borrow().clone();
            let chosen = category.is_some();
            // Every section must be updated, so this cannot short-circuit.
            let mut found = false;
            for section in sections.iter() {
                let inside = category.as_deref().is_none_or(|name| name == section.title);
                found |= section.show(&query, inside, chosen);
            }
            let plugins_inside = category.as_deref().is_none_or(|name| name == "Plugins");
            for section in plugins.borrow().iter() {
                found |= section.show(&query, plugins_inside, chosen);
            }
            empty.set_visible(!found);
        })
    };
    *refresh_view.borrow_mut() = Some(refresh.clone());
    let remembered_query = memory.query.clone();
    let apply_search = refresh.clone();
    search.connect_search_changed(move |search| {
        *remembered_query.borrow_mut() = search.text().to_string();
        apply_search();
    });
    for chip in &chip_buttons {
        let (category, apply) = (memory.category.clone(), refresh.clone());
        chip.connect_toggled(move |chip| {
            if chip.is_active() {
                let name = chip.label().map(|name| name.to_string());
                *category.borrow_mut() = name.filter(|name| name != "All");
                apply();
            }
        });
    }
    // A rebuilt catalog shows the search the user had typed.
    let remembered = memory.query.borrow().clone();
    search.set_text(&remembered);
    refresh();
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(320)
        .max_content_height(640)
        .propagate_natural_height(true)
        .child(&list)
        .build();
    catalog.append(&scroll);
    let model = ui.model.get();
    for (index, key) in keys.iter().take(usize::from(model.keys())).enumerate() {
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
    for (index, dial) in keys
        .iter()
        .skip(usize::from(model.keys()) + 1)
        .take(usize::from(model.dials))
        .enumerate()
    {
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
                assign(action.clone(), Target::Dial(index, event));
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

/// Reapplies the search and the category chip to the whole catalog, once it is built.
type RefreshHook = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// What every category row is connected to.
struct Wiring<'a> {
    memory: &'a CatalogMemory,
    ui: &'a BindingUi,
    assign: &'a Assign,
}

/// What only some sections have: a remembered-state key and the widgets heading a plugin.
#[derive(Default)]
struct Extras<'a> {
    expanded: bool,
    preface: Option<&'a GtkBox>,
    key: Option<String>,
    /// Sub-headings of long categories: the row index each one precedes, and its text.
    groups: Vec<(usize, &'static str)>,
}

impl Extras<'_> {
    fn open(expanded: bool) -> Self {
        Self {
            expanded,
            ..Self::default()
        }
    }
}

/// One collapsible category: a header toggle and its action rows.
struct Section {
    title: String,
    /// Group sub-headings; they only make sense while the whole category is listed.
    headings: Vec<Label>,
    container: GtkBox,
    header: Button,
    revealer: gtk::Revealer,
    chevron: gtk::Image,
    rows: Vec<(Button, String)>,
    /// Plugin rows only: what each row needs, to dim the ones the selected control cannot take.
    fit: Vec<Option<(bool, Vec<PluginControl>)>>,
    expanded: Rc<Cell<bool>>,
}

impl Section {
    fn new(
        list: &GtkBox,
        title: &str,
        icon: BindingIcon,
        entries: Vec<(String, Option<String>, BindingIcon, CatalogAction)>,
        extras: Extras,
        wiring: &Wiring,
    ) -> Self {
        let Wiring { memory, ui, assign } = wiring;
        let Extras {
            expanded,
            preface,
            key,
            groups,
        } = extras;
        // Remembered open state is keyed apart from the title, which can show a status.
        let key = key.unwrap_or_else(|| title.to_owned());
        let title = title.to_owned();
        let expanded = memory.open.borrow().get(&key).copied().unwrap_or(expanded);
        let container = GtkBox::new(Orientation::Vertical, 2);
        container.add_css_class("catalog-section");
        let header = Button::new();
        header.add_css_class("flat");
        header.add_css_class("catalog-category");
        header.set_tooltip_text(Some(&format!("Show or hide {title} actions")));
        let line = GtkBox::new(Orientation::Horizontal, 8);
        line.append(&binding_icon::binding_icon(icon));
        let name = Label::new(Some(&title));
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
        if let Some(preface) = preface {
            body.append(preface);
        }
        let mut headings = Vec::new();
        let mut rows = Vec::new();
        for (index, (label, subtitle, icon, action)) in entries.into_iter().enumerate() {
            if let Some((_, text)) = groups.iter().find(|(at, _)| *at == index) {
                let heading = Label::new(Some(text));
                heading.set_xalign(0.0);
                heading.add_css_class("catalog-group");
                body.append(&heading);
                headings.push(heading);
            }
            rows.push(add_row(
                &body,
                label,
                subtitle.as_deref(),
                icon,
                action,
                ui,
                (*assign).clone(),
            ));
        }
        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .child(&body)
            .build();
        container.append(&revealer);
        list.append(&container);
        let section = Self {
            title: title.clone(),
            headings,
            container,
            header,
            revealer,
            chevron,
            rows,
            fit: Vec::new(),
            expanded: Rc::new(Cell::new(expanded)),
        };
        section.apply(expanded);
        let revealer = section.revealer.downgrade();
        let chevron = section.chevron.downgrade();
        let state = section.expanded.clone();
        let remembered = memory.open.clone();
        section.header.connect_clicked(move |_| {
            let open = !state.get();
            state.set(open);
            remembered.borrow_mut().insert(key.clone(), open);
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

    /// Dim plugin rows that are disabled or do not fit the control being edited.
    fn dim_unfit(&self, control: Option<PluginControl>) {
        for ((row, label), fit) in self.rows.iter().zip(&self.fit) {
            let Some((enabled, controls)) = fit else {
                continue;
            };
            let fits = control.is_none_or(|control| controls.contains(&control));
            row.set_sensitive(*enabled && fits);
            row.set_tooltip_text(Some(&if !enabled {
                "Enable this plugin to assign its actions.".to_owned()
            } else if !fits {
                "This action does not support the selected control.".to_owned()
            } else {
                format!(
                    "Drag {label} onto a key or dial, or click to assign to the selected control."
                )
            }));
        }
    }

    /// Applies the search and the category chip: a section outside the chosen category is
    /// hidden, and the chosen one opens so its actions are reachable at once.
    fn show(&self, query: &str, in_category: bool, chosen: bool) -> bool {
        let matches = self.filter(query);
        if !in_category {
            self.container.set_visible(false);
            return false;
        }
        if chosen && query.is_empty() {
            self.apply(true);
        }
        matches
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
        for heading in &self.headings {
            heading.set_visible(query.is_empty());
        }
        if query.is_empty() {
            self.apply(self.expanded.get());
        } else {
            self.apply(true);
        }
        self.header.set_sensitive(query.is_empty());
        matches > 0
    }
}

/// Rebuilds the plugin sidebar items: a "Plugins" heading, then one collapsible item per plugin.
fn populate_plugins(region: &GtkBox, built: &RefCell<Vec<Section>>, wiring: &Wiring) {
    while let Some(child) = region.first_child() {
        region.remove(&child);
    }
    built.borrow_mut().clear();
    let hub = &wiring.ui.plugins;
    region.append(&plugin_ui::heading(hub));
    let fail: Rc<dyn Fn(&str)> = {
        let ui = wiring.ui.clone();
        Rc::new(move |message| ui.error(message))
    };
    for (manifest, enabled) in hub.plugins() {
        let preface = plugin_ui::preface(hub, &manifest, enabled, fail.clone());
        let entries = manifest
            .actions
            .iter()
            .map(|action| {
                let controls: Vec<_> = action
                    .controls
                    .iter()
                    .map(|control| plugin_ui::control_name(*control))
                    .collect();
                (
                    action.name.clone(),
                    Some(controls.join(" · ")),
                    BindingIcon::More,
                    CatalogAction {
                        picker: plugin_ui::PLUGIN_PICKER,
                        feature: None,
                        plugin: Some((manifest.id.clone(), action.id.clone())),
                    },
                )
            })
            .collect();
        let title = if enabled {
            manifest.name.clone()
        } else {
            format!("{} (disabled)", manifest.name)
        };
        let mut section = Section::new(
            region,
            &title,
            BindingIcon::More,
            entries,
            Extras {
                expanded: false,
                preface: Some(&preface),
                key: Some(format!("plugin:{}", manifest.id)),
                groups: Vec::new(),
            },
            wiring,
        );
        section.fit = manifest
            .actions
            .iter()
            .map(|action| Some((enabled, action.controls.clone())))
            .collect();
        section.dim_unfit(hub.control());
        built.borrow_mut().push(section);
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
    label: String,
    subtitle: Option<&str>,
    icon: BindingIcon,
    action: CatalogAction,
    ui: &BindingUi,
    assign: Assign,
) -> (Button, String) {
    let row = row_button(
        &label,
        subtitle,
        icon,
        &format!("Drag {label} onto a key or dial, or click to assign to the selected control."),
    );
    let source = gtk::DragSource::builder()
        .actions(gtk::gdk::DragAction::COPY)
        .build();
    let dragged = action.clone();
    source.connect_prepare(move |_, _, _| {
        Some(gtk::gdk::ContentProvider::for_value(&dragged.to_value()))
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
        let model = ui.model.get();
        match (ui.control_kind(), number) {
            (ControlKind::Button, Some(index)) if index < usize::from(model.keys()) => {
                assign(action.clone(), Target::Key(index));
            }
            (ControlKind::Encoder, Some(index)) if index < usize::from(model.dials) => {
                assign(action.clone(), Target::Dial(index, ui.event_kind()));
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
