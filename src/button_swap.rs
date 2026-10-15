use crate::*;

struct KeyDrag {
    key: u8,
    owner: ChooserOwner,
    session: Rc<()>,
}

pub(crate) fn connect(
    keys: &[gtk::Widget],
    artwork: &ArtworkUi,
    runtime: &feature_runtime::FeatureRuntime,
    root: &GtkBox,
) {
    runtime.attach_key_swap_root(root.upcast_ref());
    let session = Rc::new(());
    for (number, key) in (1u8..=8).zip(keys) {
        let source = gtk::DragSource::builder()
            .actions(gtk::gdk::DragAction::MOVE)
            .build();
        let images = artwork.clone();
        let source_session = session.clone();
        source.connect_prepare(move |_, _, _| {
            let mut owner = images.capture_owner();
            owner.target = ArtworkTarget::Keypad(number);
            let value = glib::BoxedAnyObject::new(KeyDrag {
                key: number,
                owner,
                session: source_session.clone(),
            })
            .to_value();
            Some(gtk::gdk::ContentProvider::for_value(&value))
        });
        let weak = key.downgrade();
        source.connect_drag_begin(move |source, _| {
            if let Some(key) = weak.upgrade() {
                source.set_icon(Some(&gtk::WidgetPaintable::new(Some(&key))), 30, 30);
                key.add_css_class("swap-source");
            }
        });
        let weak = key.downgrade();
        source.connect_drag_end(move |_, _, _| {
            if let Some(key) = weak.upgrade() {
                key.remove_css_class("swap-source");
            }
        });
        key.add_controller(source);
        let target = gtk::DropTarget::new(
            glib::BoxedAnyObject::static_type(),
            gtk::gdk::DragAction::MOVE,
        );
        let weak = key.downgrade();
        target.connect_enter(move |_, _, _| {
            if let Some(key) = weak.upgrade() {
                key.add_css_class("swap-target");
            }
            gtk::gdk::DragAction::MOVE
        });
        let weak = key.downgrade();
        target.connect_leave(move |_| {
            if let Some(key) = weak.upgrade() {
                key.remove_css_class("swap-target");
            }
        });
        let images = artwork.clone();
        let runtime = runtime.clone();
        let target_session = session.clone();
        let weak = key.downgrade();
        target.connect_drop(move |_, value, _, _| {
            if let Some(key) = weak.upgrade() {
                key.remove_css_class("swap-target");
            }
            let Ok(object) = value.get::<glib::BoxedAnyObject>() else {
                return false;
            };
            let Ok(drag) = object.try_borrow::<KeyDrag>() else {
                return false;
            };
            let valid_owner = images
                .chooser_owners
                .borrow()
                .get(&drag.owner.name)
                .is_some_and(|owner| Rc::ptr_eq(owner, &drag.owner.incarnation));
            Rc::ptr_eq(&target_session, &drag.session)
                && valid_owner
                && runtime.swap_keys(&drag.owner.name, drag.key, number)
        });
        key.add_controller(target);
    }
}
