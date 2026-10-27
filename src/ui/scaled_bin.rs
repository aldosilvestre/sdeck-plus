//! Proportional presentation scaling for fixed-geometry content.
//!
//! The child keeps its own coordinates and natural size; this container only
//! applies a uniform scale transform so the hardware preview can use spare
//! room. GTK applies the same transform to rendering and pointer picking.
use gtk::{glib, graphene, gsk, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;
    use std::cell::{Cell, RefCell};

    pub struct ScaledBin {
        pub(super) child: RefCell<Option<gtk::Widget>>,
        pub(super) max_scale: Cell<f64>,
        pub(super) scale: Cell<f64>,
    }

    impl Default for ScaledBin {
        fn default() -> Self {
            Self {
                child: RefCell::new(None),
                max_scale: Cell::new(1.0),
                scale: Cell::new(1.0),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ScaledBin {
        const NAME: &'static str = "SdeckScaledBin";
        type Type = super::ScaledBin;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for ScaledBin {
        fn dispose(&self) {
            if let Some(child) = self.child.take() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for ScaledBin {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = self.child.borrow().clone() else {
                return (0, 0, -1, -1);
            };
            let (width, height) = natural_size(&child);
            let max_scale = self.max_scale.get();
            match orientation {
                // The minimum is always the unscaled content, so spare room is
                // optional and never forces scrolling.
                // Natural width stays unscaled: FlowBox decides wrapping from
                // natural widths, and spare width arrives through hexpand.
                gtk::Orientation::Horizontal => (width, width, -1, -1),
                _ => {
                    let natural = if for_size > 0 && width > 0 {
                        let scale = (f64::from(for_size) / f64::from(width)).clamp(1.0, max_scale);
                        scaled(height, scale)
                    } else {
                        height
                    };
                    (height, natural, -1, -1)
                }
            }
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let Some(child) = self.child.borrow().clone() else {
                return;
            };
            let (natural_width, natural_height) = natural_size(&child);
            if natural_width <= 0 || natural_height <= 0 {
                return;
            }
            let scale = (f64::from(width) / f64::from(natural_width))
                .min(f64::from(height) / f64::from(natural_height))
                .clamp(1.0, self.max_scale.get());
            self.scale.set(scale);
            // Center horizontally, keep the top edge aligned with neighbours.
            let offset = (f64::from(width) - f64::from(natural_width) * scale).max(0.0) / 2.0;
            let transform = gsk::Transform::new()
                .translate(&graphene::Point::new(offset.round() as f32, 0.0))
                .scale(scale as f32, scale as f32);
            child.allocate(natural_width, natural_height, -1, Some(transform));
        }
    }

    fn natural_size(child: &gtk::Widget) -> (i32, i32) {
        let width = child.measure(gtk::Orientation::Horizontal, -1).1;
        let height = child.measure(gtk::Orientation::Vertical, width).1;
        (width, height)
    }

    fn scaled(value: i32, scale: f64) -> i32 {
        (f64::from(value) * scale).floor() as i32
    }
}

glib::wrapper! {
    /// Uniformly scales one child between 1x and `max_scale`, never below its
    /// natural size, preserving the child's internal geometry.
    pub struct ScaledBin(ObjectSubclass<imp::ScaledBin>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ScaledBin {
    pub(crate) fn new(child: &impl IsA<gtk::Widget>, max_scale: f64) -> Self {
        let bin: Self = glib::Object::new();
        let imp = bin.imp();
        imp.max_scale.set(max_scale.max(1.0));
        child.set_parent(&bin);
        imp.child.replace(Some(child.clone().upcast()));
        bin
    }

    /// The scale applied at the last allocation (1.0 before allocation).
    #[cfg(test)]
    pub(crate) fn scale(&self) -> f64 {
        self.imp().scale.get()
    }
}
