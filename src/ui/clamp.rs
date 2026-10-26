//! Bounded reading measure: fills the available width up to a maximum, then
//! centers its child. GTK 4.8 has no native clamp outside libadwaita.
use gtk::{glib, graphene, gsk, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    pub struct Clamp {
        pub(super) child: RefCell<Option<gtk::Widget>>,
        pub(super) maximum: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Clamp {
        const NAME: &'static str = "SdeckClamp";
        type Type = super::Clamp;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Clamp {
        fn dispose(&self) {
            if let Some(child) = self.child.take() {
                child.unparent();
            }
        }
    }

    impl Clamp {
        fn child_width(&self, child: &gtk::Widget, available: i32) -> i32 {
            let minimum = child.measure(gtk::Orientation::Horizontal, -1).0;
            available.min(self.maximum.get()).max(minimum)
        }
    }

    impl WidgetImpl for Clamp {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = self.child.borrow().clone() else {
                return (0, 0, -1, -1);
            };
            if orientation == gtk::Orientation::Horizontal {
                let (minimum, natural, _, _) = child.measure(orientation, -1);
                return (
                    minimum,
                    natural.max(self.maximum.get()).max(minimum),
                    -1,
                    -1,
                );
            }
            let width = if for_size < 0 {
                -1
            } else {
                self.child_width(&child, for_size)
            };
            let (minimum, natural, _, _) = child.measure(orientation, width);
            (minimum, natural, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let Some(child) = self.child.borrow().clone() else {
                return;
            };
            let child_width = self.child_width(&child, width);
            let offset = ((width - child_width).max(0) / 2) as f32;
            let transform = gsk::Transform::new().translate(&graphene::Point::new(offset, 0.0));
            child.allocate(child_width, height, baseline, Some(transform));
        }
    }
}

glib::wrapper! {
    /// Gives its child the available width up to `maximum`, centered beyond it.
    pub struct Clamp(ObjectSubclass<imp::Clamp>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Clamp {
    pub(crate) fn new(child: &impl IsA<gtk::Widget>, maximum: i32) -> Self {
        let clamp: Self = glib::Object::new();
        clamp.imp().maximum.set(maximum);
        child.set_parent(&clamp);
        clamp.imp().child.replace(Some(child.clone().upcast()));
        clamp
    }
}
