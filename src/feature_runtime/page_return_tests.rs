use super::*;

impl FeatureRuntime {
    pub(crate) fn assert_page_return(&self) {
        let original = self.layers.borrow().active().to_owned();
        let command = || Action::Command("true".into());
        let id = || String::from("return-test");
        for removed in [false, true] {
            let mut target = String::new();
            self.change_organization(|organization, layers| {
                target = organization.create_named_page(layers, "Return destination")?;
                Ok(())
            })
            .unwrap();
            self.execute(id(), Action::SwitchLayer(original.clone()));
            let after = |seconds| {
                Some(PageReturn {
                    layer: target.clone(),
                    seconds,
                })
            };

            self.execute_with_return(id(), command(), after(60));
            assert!(self.page_return_active());
            assert_eq!(self.layers.borrow().active(), original);
            self.execute(id(), Action::Feature(FeatureAction::CancelTimer));
            assert!(!self.page_return_active());

            // A missing page reports an error and keeps the pending return.
            self.execute_with_return(id(), command(), after(60));
            let pending = self.page_return.borrow().as_ref().unwrap().as_raw();
            let missing = Some(PageReturn {
                layer: "Missing page".into(),
                seconds: 1,
            });
            self.execute_with_return(id(), command(), missing);
            assert_eq!(
                self.page_return.borrow().as_ref().unwrap().as_raw(),
                pending
            );

            let loop_ = glib::MainLoop::new(None, false);
            let completed = Rc::new(Cell::new(false));
            let armed = Rc::new(Cell::new(false));
            let callback = self.clone();
            let completion_loop = loop_.clone();
            let completion_flag = completed.clone();
            let ready = armed.clone();
            let signal = self.status.connect_label_notify(move |_| {
                if ready.get() && !callback.page_return_active() {
                    completion_flag.set(true);
                    completion_loop.quit();
                }
            });
            let timed_out = Rc::new(Cell::new(false));
            let timeout_flag = timed_out.clone();
            let timeout_loop = loop_.clone();
            let timeout = glib::timeout_add_local_once(Duration::from_secs(4), move || {
                timeout_flag.set(true);
                timeout_loop.quit();
            });
            self.execute_with_return(id(), command(), after(1));
            armed.set(true);
            assert_ne!(
                self.page_return.borrow().as_ref().unwrap().as_raw(),
                pending
            );

            if removed {
                self.change_organization(|organization, layers| {
                    assert!(layers.select(&target));
                    organization.remove_page(layers)?;
                    assert!(layers.select(&original));
                    Ok(())
                })
                .unwrap();
            } else {
                // Entering a folder while waiting must not change the destination.
                self.change_organization(|organization, layers| {
                    organization.create_folder(layers, "Return wait folder")?;
                    Ok(())
                })
                .unwrap();
                assert_ne!(self.layers.borrow().active(), original);
            }
            loop_.run();
            self.status.disconnect(signal);
            if !timed_out.get() {
                timeout.remove();
            }
            assert!(completed.get(), "page return did not complete");
            assert!(!self.page_return_active(), "a return must not repeat");
            if removed {
                assert_eq!(self.layers.borrow().active(), original);
                assert!(self.status.has_css_class("feedback-error"));
            } else {
                assert_eq!(self.layers.borrow().active(), target);
                assert!(self.status.has_css_class("feedback-success"));
                self.change_organization(|organization, layers| {
                    organization.remove_page(layers)?;
                    assert!(layers.select(&original));
                    organization.remove_folder(layers, "Return wait folder")
                })
                .unwrap();
            }
        }
        // The option is independent of the countdown, even for a Timer action.
        self.execute_with_return(
            id(),
            Action::Feature(FeatureAction::Timer { seconds: 60 }),
            Some(PageReturn {
                layer: original.clone(),
                seconds: 60,
            }),
        );
        assert!(self.timer_active() && self.page_return_active());
        self.cancel_timer();
        assert!(!self.timer_active() && !self.page_return_active());
        // None is inert even if an old return option reaches the runtime.
        self.execute_with_return(
            id(),
            Action::None,
            Some(PageReturn {
                layer: original.clone(),
                seconds: 60,
            }),
        );
        assert!(!self.page_return_active());
        assert_eq!(self.layers.borrow().active(), original);
    }

    pub(crate) fn assert_brightness_info(&self) {
        let shown = || {
            self.artwork
                .info
                .borrow()
                .get(&InfoSource::Brightness)
                .map(|value| (value.level, value.headline.clone(), value.inactive))
        };
        self.track_brightness(&DeviceAction::Brightness(40));
        assert_eq!(shown(), Some((Some(40), "40%".into(), false)));
        self.track_brightness(&DeviceAction::AdjustBrightness(-50));
        assert_eq!(shown(), Some((Some(0), "0%".into(), false)));
        self.track_brightness(&DeviceAction::AdjustBrightness(30));
        self.track_brightness(&DeviceAction::Sleep);
        assert_eq!(shown(), Some((Some(0), "OFF".into(), true)));
        self.track_brightness(&DeviceAction::Wake);
        assert_eq!(shown(), Some((Some(30), "30%".into(), false)));
    }
}
