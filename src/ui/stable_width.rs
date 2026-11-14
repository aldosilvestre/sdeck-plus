//! A container whose horizontal size request ignores the height it is given.
//!
//! A column of wrapping labels and lists reports a wider minimum when it is measured for a
//! short height (it would have to grow wider to fit). A horizontal box measures its children
//! for the height it allocates, so such a column could take one of two stable widths. This
//! wrapper always answers the horizontal request for an unconstrained height; the vertical
//! request still follows the width it is given.
use gtk::{glib, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    pub struct StableWidth {
        pub(super) child: RefCell<Option<gtk::Widget>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for StableWidth {
        const NAME: &'static str = "SdeckStableWidth";
        type Type = super::StableWidth;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for StableWidth {
        fn dispose(&self) {
            if let Some(child) = self.child.take() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for StableWidth {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = self.child.borrow().clone() else {
                return (0, 0, -1, -1);
            };
            let for_size = if orientation == gtk::Orientation::Horizontal {
                -1
            } else {
                for_size
            };
            let (minimum, natural, _, _) = child.measure(orientation, for_size);
            (minimum, natural, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.child.borrow().as_ref() {
                child.allocate(width, height, baseline, None);
            }
        }
    }
}

glib::wrapper! {
    /// Passes its allocation to its child; see the module documentation.
    pub struct StableWidth(ObjectSubclass<imp::StableWidth>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl StableWidth {
    pub(crate) fn new(child: &impl IsA<gtk::Widget>) -> Self {
        let wrapper: Self = glib::Object::new();
        child.set_parent(&wrapper);
        wrapper.imp().child.replace(Some(child.clone().upcast()));
        wrapper
    }
}
