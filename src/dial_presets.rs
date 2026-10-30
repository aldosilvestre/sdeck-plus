//! Dial presets: draggable turn/press configurations for the four dials.
use crate::*;
use action_catalog::row_button;
use sdeck_plus::{
    desktop_actions::{DesktopAction as D, MediaCommand as M, ScrollDirection},
    device_control::DeviceAction,
    features::FeatureAction,
    organization::NavigationAction as N,
};

#[derive(Clone, Copy)]
enum DialEvent {
    Press,
    Clockwise,
    CounterClockwise,
}

impl DialEvent {
    fn trigger(self, dial: u8) -> Trigger {
        match self {
            Self::Press => Trigger::EncoderPress(dial),
            Self::Clockwise => Trigger::EncoderClockwise(dial),
            Self::CounterClockwise => Trigger::EncoderCounterClockwise(dial),
        }
    }
}

/// What to put on a dial; only the listed events are replaced.
struct Preset {
    label: &'static str,
    summary: &'static str,
    events: Vec<(DialEvent, Action)>,
}

/// The payload of a dragged preset: its index in [`presets`].
#[derive(Clone, Copy, glib::Boxed)]
#[boxed_type(name = "SdeckDialPreset")]
struct DialPreset(usize);

fn desktop(action: D) -> Action {
    Action::Feature(FeatureAction::Desktop(action))
}

fn presets() -> Vec<Preset> {
    use DialEvent::{Clockwise, CounterClockwise, Press};
    let scroll = |direction| {
        desktop(D::MouseScroll {
            direction,
            ticks: 1,
        })
    };
    vec![
        Preset {
            label: "Volume",
            summary: "Turn: volume · Press: mute",
            events: vec![
                (Clockwise, desktop(D::Media(M::VolumeUp))),
                (CounterClockwise, desktop(D::Media(M::VolumeDown))),
                (Press, desktop(D::Media(M::Mute))),
            ],
        },
        Preset {
            label: "Brightness",
            summary: "Turn: dim or brighten the device",
            events: vec![
                (
                    Clockwise,
                    Action::Feature(FeatureAction::Device(DeviceAction::AdjustBrightness(10))),
                ),
                (
                    CounterClockwise,
                    Action::Feature(FeatureAction::Device(DeviceAction::AdjustBrightness(-10))),
                ),
            ],
        },
        Preset {
            label: "Tracks",
            summary: "Turn: next or previous · Press: play / pause",
            events: vec![
                (Clockwise, desktop(D::Media(M::Next))),
                (CounterClockwise, desktop(D::Media(M::Previous))),
                (Press, desktop(D::Media(M::PlayPause))),
            ],
        },
        Preset {
            label: "Scroll",
            summary: "Turn: scroll the page down or up",
            events: vec![
                (Clockwise, scroll(ScrollDirection::Down)),
                (CounterClockwise, scroll(ScrollDirection::Up)),
            ],
        },
        Preset {
            label: "Pages",
            summary: "Turn: next or previous keypad page",
            events: vec![
                (
                    Clockwise,
                    Action::Feature(FeatureAction::Navigation(N::NextPage)),
                ),
                (
                    CounterClockwise,
                    Action::Feature(FeatureAction::Navigation(N::PreviousPage)),
                ),
            ],
        },
    ]
}

type Assign = Rc<dyn Fn(usize, usize) -> bool>;

/// Build the Dials tab and make the four dial previews accept its presets.
pub(crate) fn connect(
    selected: Rc<Vec<gtk::Widget>>,
    ui: &BindingUi,
    layers: &Rc<RefCell<LayerStore>>,
    heading: &Label,
    runtime: &feature_runtime::FeatureRuntime,
    artwork: &ArtworkUi,
) -> GtkBox {
    let presets = Rc::new(presets());
    let assign: Assign = {
        let ui = ui.clone();
        let layers = layers.clone();
        let heading = heading.clone();
        let runtime = runtime.clone();
        let artwork = artwork.clone();
        let presets = presets.clone();
        let widgets: Vec<_> = selected.iter().map(ObjectExt::downgrade).collect();
        let guard_ui = ui.clone();
        let guard_widgets = widgets.clone();
        let apply: Assign = Rc::new(move |preset, dial| {
            let Some(preset) = presets.get(preset) else {
                return false;
            };
            let number = (dial + 1) as u8;
            let Some(layer) = dropdown_value(&ui.layer_model, ui.layer_picker.selected()) else {
                ui.error("Select a profile or page.");
                return false;
            };
            // Bind on a copy so a failure never leaves a half-applied preset.
            let mut next = layers.borrow().clone();
            for (event, action) in &preset.events {
                if let Err(error) = next.bind(&layer, event.trigger(number), action.clone()) {
                    ui.error(&format!("Could not assign {}: {error}", preset.label));
                    return false;
                }
            }
            *layers.borrow_mut() = next.clone();
            if ui
                .select_control(ControlKind::Encoder, Some(number), true)
                .is_err()
            {
                return false;
            }
            let controls: Vec<_> = widgets.iter().filter_map(glib::WeakRef::upgrade).collect();
            set_selected_control(&controls, 9 + dial);
            heading.set_text(&control_heading(ControlKind::Encoder, Some(number)));
            ui.load_selected_binding(&layers);
            if runtime.save_binding(next) {
                ui.status.set_text(&format!(
                    "Dial {number} set to {}; saving to disk…",
                    preset.label
                ));
            } else {
                ui.error("Could not save binding: saving worker is unavailable.");
            }
            ui.refresh_summaries(&layers);
            artwork.refresh(true);
            true
        });
        Rc::new(move |preset, dial| {
            // A draft on this very dial is replaced; one elsewhere asks first.
            let same_dial = guard_ui.control_kind() == ControlKind::Encoder
                && guard_ui.control_number() == Some((dial + 1) as u8);
            if guard_ui.is_dirty() && !same_dial {
                let anchor = guard_widgets
                    .get(9 + dial)
                    .and_then(glib::WeakRef::upgrade)
                    .and_then(|control| control.downcast::<Button>().ok());
                if let Some(anchor) = anchor {
                    let apply = apply.clone();
                    confirm_discard(
                        &anchor,
                        Rc::new(move || {
                            apply(preset, dial);
                        }),
                    );
                    return true;
                }
            }
            apply(preset, dial)
        })
    };

    let page = GtkBox::new(Orientation::Vertical, 8);
    page.append(&ui::helper(
        "Drag a preset onto a dial, or select a dial and click one. A preset replaces only the events it lists.",
    ));
    let list = GtkBox::new(Orientation::Vertical, 2);
    list.add_css_class("catalog-rows");
    for (index, preset) in presets.iter().enumerate() {
        let icon = preset
            .events
            .first()
            .map_or(BindingIcon::Generic, |(_, action)| {
                BindingIcon::for_action(action)
            });
        let row = row_button(
            preset.label,
            Some(preset.summary),
            icon,
            &format!(
                "Drag {} onto a dial, or click to assign to the selected dial.",
                preset.label
            ),
        );
        let source = gtk::DragSource::builder()
            .actions(gtk::gdk::DragAction::COPY)
            .build();
        source.connect_prepare(move |_, _, _| {
            Some(gtk::gdk::ContentProvider::for_value(
                &DialPreset(index).to_value(),
            ))
        });
        let weak = row.downgrade();
        source.connect_drag_begin(move |source, _| {
            if let Some(row) = weak.upgrade() {
                source.set_icon(Some(&gtk::WidgetPaintable::new(Some(&row))), 16, 16);
            }
        });
        row.add_controller(source);
        let ui = ui.clone();
        let assign = assign.clone();
        row.connect_clicked(
            move |_| match ui.control_number().and_then(|n| n.checked_sub(1)) {
                Some(dial) if ui.control_kind() == ControlKind::Encoder && dial < 4 => {
                    assign(index, usize::from(dial));
                }
                _ => ui.error("Select a dial before assigning a preset."),
            },
        );
        list.append(&row);
    }
    page.append(&list);

    for (dial, control) in selected.iter().skip(9).take(4).enumerate() {
        // A distinct boxed type keeps presets apart from keys and key swapping.
        let target = gtk::DropTarget::new(DialPreset::static_type(), gtk::gdk::DragAction::COPY);
        let weak = control.downgrade();
        target.connect_enter(move |_, _, _| {
            if let Some(control) = weak.upgrade() {
                control.add_css_class("action-drop-target");
            }
            gtk::gdk::DragAction::COPY
        });
        let weak = control.downgrade();
        target.connect_leave(move |_| {
            if let Some(control) = weak.upgrade() {
                control.remove_css_class("action-drop-target");
            }
        });
        let weak = control.downgrade();
        let assign = assign.clone();
        target.connect_drop(move |_, value, _, _| {
            if let Some(control) = weak.upgrade() {
                control.remove_css_class("action-drop-target");
            }
            value
                .get::<DialPreset>()
                .is_ok_and(|preset| assign(preset.0, dial))
        });
        control.add_controller(target);
    }
    page
}
