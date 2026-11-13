use gtk::{gio, glib, prelude::*, Box as GtkBox, Button, Label, Orientation};
use sdeck_plus::desktop_actions::audio_output::{list_inputs, list_outputs, AudioOutput};
use sdeck_plus::desktop_actions::DesktopAction;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

/// Which kind of audio device a picker chooses.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Output,
    Input,
}

impl Kind {
    fn field(self) -> &'static str {
        match self {
            Self::Output => "Output device",
            Self::Input => "Input device",
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::Output => "output",
            Self::Input => "input",
        }
    }
}

#[derive(Clone)]
pub(super) struct OutputPicker(Rc<Widgets>);

struct Widgets {
    kind: Kind,
    root: GtkBox,
    picker: gtk::DropDown,
    model: gtk::StringList,
    refresh: Button,
    status: Label,
    outputs: RefCell<Vec<AudioOutput>>,
    names: RefCell<Vec<String>>,
    /// None selects rotation; Some selects a stable device name.
    selected: RefCell<Option<String>>,
    updating: Cell<bool>,
}

impl OutputPicker {
    pub(super) fn new(kind: Kind) -> Self {
        let model = gtk::StringList::new(&[]);
        let picker = gtk::DropDown::new(Some(model.clone()), gtk::Expression::NONE);
        crate::bound_layer_picker(&picker);
        picker.set_hexpand(true);
        let refresh = crate::ui::button("view-refresh-symbolic", "Refresh");
        let controls = GtkBox::new(Orientation::Horizontal, 8);
        controls.append(&picker);
        controls.append(&refresh);
        let (root, label) = super::field(kind.field(), &controls);
        label.set_mnemonic_widget(Some(&picker));
        picker.update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
        let status = crate::ui::helper("");
        root.append(&status);
        let this = Self(Rc::new(Widgets {
            kind,
            root,
            picker,
            model,
            refresh,
            status,
            outputs: RefCell::default(),
            names: RefCell::default(),
            selected: RefCell::default(),
            updating: Cell::new(false),
        }));
        let weak = Rc::downgrade(&this.0);
        this.0.picker.connect_selected_notify(move |picker| {
            if let Some(widgets) = weak.upgrade() {
                if !widgets.updating.get() {
                    *widgets.selected.borrow_mut() = picker
                        .selected()
                        .checked_sub(1)
                        .and_then(|index| widgets.names.borrow().get(index as usize).cloned());
                }
            }
        });
        let weak = Rc::downgrade(&this.0);
        this.0.refresh.connect_clicked(move |_| {
            if let Some(widgets) = weak.upgrade() {
                Self(widgets).refresh();
            }
        });
        let weak = Rc::downgrade(&this.0);
        this.0.root.connect_map(move |_| {
            if let Some(widgets) = weak.upgrade() {
                Self(widgets).refresh();
            }
        });
        this
    }

    pub(super) fn root(&self) -> &GtkBox {
        &self.0.root
    }

    pub(super) fn action(&self) -> DesktopAction {
        let selected = self.0.selected.borrow().clone();
        match (self.0.kind, selected) {
            (Kind::Output, Some(name)) => DesktopAction::SetAudioOutput { name },
            (Kind::Output, None) => DesktopAction::CycleAudioOutput,
            (Kind::Input, Some(name)) => DesktopAction::SetAudioInput { name },
            (Kind::Input, None) => DesktopAction::CycleAudioInput,
        }
    }

    pub(super) fn select(&self, name: Option<&str>) {
        *self.0.selected.borrow_mut() = name.map(str::to_owned);
        self.rebuild();
    }

    fn rebuild(&self) {
        let selected = self.0.selected.borrow().clone();
        let outputs = self.0.outputs.borrow();
        let mut names: Vec<_> = outputs.iter().map(|output| output.name.clone()).collect();
        let mut labels: Vec<_> = outputs
            .iter()
            .map(|output| output.description.clone())
            .collect();
        if let Some(name) = &selected {
            if !names.contains(name) {
                names.push(name.clone());
                labels.push(format!("{name} (not connected)"));
            }
        }
        let index = selected
            .as_ref()
            .and_then(|name| names.iter().position(|item| item == name));
        labels.insert(0, "Rotate".into());
        self.0.updating.set(true);
        *self.0.names.borrow_mut() = names;
        crate::replace_string_list(&self.0.model, &labels);
        self.0
            .picker
            .set_selected(index.map_or(0, |index| index as u32 + 1));
        *self.0.selected.borrow_mut() = selected;
        self.0.updating.set(false);
    }

    pub(super) fn set_outputs(&self, outputs: Vec<AudioOutput>) {
        let empty = outputs.is_empty();
        *self.0.outputs.borrow_mut() = outputs;
        self.rebuild();
        let noun = self.0.kind.noun();
        self.0.status.set_text(&if empty {
            format!("No {noun}s connected. Connect a device and refresh.")
        } else {
            format!("Rotate selects the next connected {noun} on each press and returns to the first after the last.")
        });
    }

    fn refresh(&self) {
        if !self.0.refresh.is_sensitive() {
            return;
        }
        self.0.refresh.set_sensitive(false);
        let kind = self.0.kind;
        self.0
            .status
            .set_text(&format!("Loading audio {}s...", kind.noun()));
        let weak = Rc::downgrade(&self.0);
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || match kind {
                Kind::Output => list_outputs(),
                Kind::Input => list_inputs(),
            })
            .await;
            let Some(widgets) = weak.upgrade() else {
                return;
            };
            let this = Self(widgets);
            this.0.refresh.set_sensitive(true);
            match result {
                Ok(Ok(outputs)) => this.set_outputs(outputs),
                Ok(Err(error)) => this
                    .0
                    .status
                    .set_text(&format!("Could not list audio {}s: {error}", kind.noun())),
                Err(_) => this.0.status.set_text(&format!(
                    "Could not list audio {}s: worker failed.",
                    kind.noun()
                )),
            }
        });
    }

    #[cfg(test)]
    pub(super) fn select_rotation(&self) {
        self.0.picker.set_selected(0);
    }

    #[cfg(test)]
    pub(super) fn wait_until_loaded(&self) {
        if self.0.refresh.is_sensitive() {
            return;
        }
        let loop_ = glib::MainLoop::new(None, false);
        let done = loop_.clone();
        let signal = self.0.refresh.connect_sensitive_notify(move |button| {
            if button.is_sensitive() {
                done.quit();
            }
        });
        let expired = Rc::new(Cell::new(false));
        let flag = expired.clone();
        let done = loop_.clone();
        let timeout = glib::timeout_add_local_once(std::time::Duration::from_secs(5), move || {
            flag.set(true);
            done.quit();
        });
        loop_.run();
        self.0.refresh.disconnect(signal);
        if !expired.get() {
            timeout.remove();
        }
        assert!(!expired.get(), "audio discovery did not finish");
    }
}
