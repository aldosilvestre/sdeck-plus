//! Shared touchscreen page controls on touch and dial inspectors.
use super::*;
use sdeck_plus::features::TouchscreenAction;

#[derive(Clone)]
pub(super) struct TouchscreenControls {
    pub(super) root: GtkBox,
    model: gtk::StringList,
    picker: gtk::DropDown,
    previous: Button,
    next: Button,
    create: Button,
    updating: Rc<Cell<bool>>,
}
impl TouchscreenControls {
    pub(super) fn new() -> Self {
        let root = GtkBox::new(Orientation::Horizontal, 6);
        root.set_tooltip_text(Some("Touchscreen pages include their dials; keypad pages and folders are unchanged. Edge indicators show available pages; assign swipe actions separately."));
        let model = gtk::StringList::new(&[]);
        let picker = gtk::DropDown::new(Some(model.clone()), gtk::Expression::NONE);
        picker.update_property(&[gtk::accessible::Property::Label("Touchscreen page")]);
        let previous = crate::ui::icon_button("go-previous-symbolic", "Previous touchscreen page");
        let next = crate::ui::icon_button("go-next-symbolic", "Next touchscreen page");
        let create = crate::ui::icon_button("list-add-symbolic", "New touchscreen page");
        root.append(&Label::new(Some("Touch page")));
        root.append(&picker);
        for button in [&previous, &next, &create] {
            root.append(button);
        }
        root.set_visible(false);
        Self {
            root,
            model,
            picker,
            previous,
            next,
            create,
            updating: Rc::new(Cell::new(false)),
        }
    }
    pub(super) fn bind(&self, runtime: &FeatureRuntime) {
        for (button, action) in [
            (&self.previous, TouchscreenAction::PreviousPage),
            (&self.next, TouchscreenAction::NextPage),
        ] {
            let runtime = runtime.clone();
            button.connect_clicked(move |_| runtime.navigate_touchscreen(action));
        }
        let callback = runtime.clone();
        self.create.connect_clicked(move |_| {
            let Some(profile) = callback.touchscreen_profile() else { return; };
            let result = callback.layers.borrow_mut().create_touchscreen_page(&profile);
            match result {
                Ok(_) => {
                    callback.refresh_selection();
                    callback.persist(callback.layers.borrow().clone());
                    callback.report("Touchscreen page created. Configure its dials, gestures and artwork independently.", false);
                }
                Err(error) => callback.report(&format!("Could not create touchscreen page: {error}"), true),
            }
        });
        let callback = runtime.clone();
        let updating = self.updating.clone();
        self.picker.connect_selected_notify(move |picker| {
            if updating.get() {
                return;
            }
            let Some(profile) = callback.touchscreen_profile() else {
                return;
            };
            let owner = callback
                .layers
                .borrow()
                .touchscreen_pages(&profile)
                .get(picker.selected() as usize)
                .cloned();
            if let Some(owner) = owner {
                // GTK can deliver coalesced selection notifications after the
                // model-refresh guard has cleared. Do not refresh an already
                // selected owner: replacing the model here would notify again.
                let already_selected = {
                    let layers = callback.layers.borrow();
                    layers.touchscreen_owner(layers.active()) == Some(owner.as_str())
                };
                if already_selected {
                    return;
                }
                if callback
                    .layers
                    .borrow_mut()
                    .select_touchscreen_page(&profile, &owner)
                {
                    callback.refresh_selection();
                    callback.persist(callback.layers.borrow().clone());
                }
            }
        });
        let callback = runtime.clone();
        runtime
            .ui
            .control_picker
            .connect_selected_notify(move |_| callback.refresh_touchscreen_controls());
    }
}
impl FeatureRuntime {
    pub(crate) fn touchscreen_controls(&self) -> &GtkBox {
        &self.touchscreen_controls.root
    }

    fn touchscreen_profile(&self) -> Option<String> {
        Some(
            self.organization
                .borrow()
                .as_ref()?
                .profile_for_key(self.layers.borrow().active()),
        )
    }
    pub(super) fn refresh_touchscreen_controls(&self) {
        let controls = &self.touchscreen_controls;
        let profile = self.touchscreen_profile();
        let layers = self.layers.borrow();
        let pages = profile
            .as_deref()
            .map(|profile| layers.touchscreen_pages(profile))
            .unwrap_or_default();
        let selected = pages
            .iter()
            .position(|owner| Some(owner.as_str()) == layers.touchscreen_owner(layers.active()))
            .unwrap_or(0);
        controls.updating.set(true);
        let labels = (1..=pages.len())
            .map(|number| format!("Page {number}"))
            .collect::<Vec<_>>();
        crate::replace_string_list(&controls.model, &labels);
        controls.picker.set_selected(selected as u32);
        controls.previous.set_sensitive(selected > 0);
        controls.next.set_sensitive(selected + 1 < pages.len());
        controls
            .create
            .set_sensitive(!pages.is_empty() && pages.len() < 64);
        controls.root.set_visible(
            matches!(
                self.ui.control_kind(),
                crate::ControlKind::Touch | crate::ControlKind::Encoder
            ) && !pages.is_empty(),
        );
        controls.updating.set(false);
    }
    pub(super) fn navigate_touchscreen(&self, action: TouchscreenAction) {
        let Some(profile) = self.touchscreen_profile() else {
            self.report("Touchscreen navigation is unavailable.", true);
            return;
        };
        let changed = self
            .layers
            .borrow_mut()
            .step_touchscreen_page(&profile, action == TouchscreenAction::NextPage);
        if changed {
            self.refresh_selection();
            self.persist(self.layers.borrow().clone());
            self.report(
                "Touchscreen page selected; keypad page and folder unchanged.",
                false,
            );
        } else {
            self.report("No matching touchscreen page in this profile.", false);
        }
    }
}
