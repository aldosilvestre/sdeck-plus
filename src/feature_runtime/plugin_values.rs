//! Values that plugin actions report for the controls they are bound to.
use crate::{
    plugin_runtime::{QueryRequest, QuerySender},
    ArtworkTarget, ArtworkUi,
};
use sdeck_plus::{
    features::FeatureAction,
    info::{Glyph, InfoSource, InfoValue},
    layers::{Action, Trigger},
    plugins::{PluginBinding, QueryOutcome},
};
use std::collections::{BTreeSet, HashMap};

/// What a key or touch section shows from a binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Shown {
    /// An explicit live-information choice.
    Info(InfoSource),
    /// The value a plugin action with a display reports.
    Plugin(PluginBinding),
}

/// In-memory answers of plugins; never stored in any configuration.
#[derive(Default)]
pub(crate) struct PluginValues {
    displays: BTreeSet<(String, String)>,
    values: HashMap<(String, ArtworkTarget), (PluginBinding, InfoValue)>,
    queries: Option<QuerySender>,
}

/// The triggers whose binding a touch section shows, in the order `zone_info_source` uses.
fn zone_triggers(zone: u8) -> [Trigger; 5] {
    [
        Trigger::EncoderPress(zone),
        Trigger::EncoderClockwise(zone),
        Trigger::EncoderCounterClockwise(zone),
        Trigger::ZoneTap(zone),
        Trigger::ZonePress(zone),
    ]
}

/// Shown until a plugin has answered.
fn placeholder() -> InfoValue {
    InfoValue::text(Glyph::None, "...")
}

impl ArtworkUi {
    /// Replaces the enabled plugin actions that declare a display; the answers kept so far
    /// are forgotten when the set changes. Returns whether it changed.
    pub(crate) fn set_plugin_displays(&self, displays: BTreeSet<(String, String)>) -> bool {
        let mut info = self.info.borrow_mut();
        if info.plugin.displays == displays {
            return false;
        }
        info.plugin.displays = displays;
        info.plugin.values.clear();
        true
    }

    pub(crate) fn set_plugin_queries(&self, queries: QuerySender) {
        self.info.borrow_mut().plugin.queries = Some(queries);
    }

    /// The plugin binding under `trigger`, when its action is enabled and declares a display.
    fn displaying_binding(&self, layer: &str, trigger: &Trigger) -> Option<PluginBinding> {
        let Some(Action::Feature(FeatureAction::Plugin(binding))) =
            self.layers.borrow().binding(layer, trigger)
        else {
            return None;
        };
        let key = (binding.plugin_id.clone(), binding.action_id.clone());
        self.info
            .borrow()
            .plugin
            .displays
            .contains(&key)
            .then_some(binding)
    }

    /// The plugin binding a touch section shows, unless live information covers the section.
    fn zone_plugin(&self, layer: &str, zone: u8) -> Option<PluginBinding> {
        if self.layers.borrow().zone_info_source(layer, zone).is_some() {
            return None;
        }
        zone_triggers(zone)
            .iter()
            .find_map(|trigger| self.displaying_binding(layer, trigger))
    }

    /// Keys and touch sections that show a plugin's value on `layer`. A control with an
    /// explicit live-information choice is not one of them: that choice wins.
    pub(crate) fn plugin_targets(&self, layer: &str) -> Vec<(ArtworkTarget, PluginBinding)> {
        if self.info.borrow().plugin.displays.is_empty() {
            return Vec::new();
        }
        let model = self.layers.borrow().model();
        let keys = model.key_numbers().filter_map(|key| {
            let trigger = Trigger::KeyPress(key);
            if self.layers.borrow().info_source(layer, &trigger).is_some() {
                return None;
            }
            Some((
                ArtworkTarget::Keypad(key),
                self.displaying_binding(layer, &trigger)?,
            ))
        });
        let zones = model.zone_numbers().filter_map(|zone| {
            Some((
                ArtworkTarget::EncoderZone(zone),
                self.zone_plugin(layer, zone)?,
            ))
        });
        keys.chain(zones).collect()
    }

    /// Everything a key or touch section draws instead of its usual picture on `layer`.
    pub(crate) fn shown_targets(&self, layer: &str) -> Vec<(ArtworkTarget, Shown)> {
        let mut shown: Vec<_> = self
            .info_targets(layer)
            .into_iter()
            .map(|(target, source)| (target, Shown::Info(source)))
            .chain(
                self.plugin_targets(layer)
                    .into_iter()
                    .map(|(target, binding)| (target, Shown::Plugin(binding))),
            )
            .collect();
        shown.sort_by_key(|(target, _)| match target {
            ArtworkTarget::Keypad(key) => (0, *key),
            ArtworkTarget::EncoderZone(zone) => (1, *zone),
            ArtworkTarget::TouchStripBackground => (2, 0),
        });
        shown
    }

    /// What a touch section shows on `layer`, if anything.
    pub(crate) fn zone_shown(&self, layer: &str, zone: u8) -> Option<Shown> {
        match self.layers.borrow().zone_info_source(layer, zone) {
            Some(source) => Some(Shown::Info(source)),
            None => self.zone_plugin(layer, zone).map(Shown::Plugin),
        }
    }

    /// The picture value for one shown thing: the reading, or the plugin's latest answer
    /// (a placeholder until it has answered).
    pub(crate) fn shown_value(
        &self,
        layer: &str,
        target: ArtworkTarget,
        shown: &Shown,
    ) -> InfoValue {
        match shown {
            Shown::Info(source) => self.info_value(*source),
            Shown::Plugin(binding) => self.plugin_value(layer, target, binding),
        }
    }

    pub(crate) fn plugin_value(
        &self,
        layer: &str,
        target: ArtworkTarget,
        binding: &PluginBinding,
    ) -> InfoValue {
        self.info
            .borrow()
            .plugin
            .values
            .get(&(layer.to_owned(), target))
            .filter(|(answered, _)| answered == binding)
            .map_or_else(placeholder, |(_, value)| value.clone())
    }

    /// Stores a plugin's answer for one control and redraws just that control, in the window
    /// and on the device. An answer for a control that no longer shows that binding is stale
    /// and dropped, as is one the control's live information or a countdown covers.
    pub(crate) fn update_plugin_value(
        &self,
        layer: String,
        target: ArtworkTarget,
        binding: PluginBinding,
        result: Result<QueryOutcome, String>,
    ) {
        let current = self
            .plugin_targets(&layer)
            .iter()
            .any(|(shown, bound)| *shown == target && *bound == binding);
        if !current {
            return;
        }
        let value = match result {
            Ok(outcome) => InfoValue {
                level: outcome.level,
                detail: outcome.detail.unwrap_or_default(),
                ..InfoValue::text(Glyph::None, &outcome.headline)
            },
            Err(_) => InfoValue {
                inactive: true,
                ..InfoValue::text(Glyph::None, "--")
            },
        };
        let previous = self
            .info
            .borrow_mut()
            .plugin
            .values
            .insert((layer.clone(), target), (binding.clone(), value.clone()));
        if previous.is_some_and(|(bound, shown)| bound == binding && shown == value) {
            return;
        }
        if self.layers.borrow().active() != layer || self.countdown_shows(&layer, target) {
            return;
        }
        self.paint_info_preview(target, &value);
        let _ = self
            .output
            .send(self.shown_command(&layer, target, &Shown::Plugin(binding)));
    }

    /// Asks the plugins for the values of the controls on `layer`; with `plugin`, only the
    /// controls of that plugin. Answers arrive later as [`MonitorEvent::PluginValue`].
    pub(crate) fn request_plugin_values(&self, layer: &str, plugin: Option<&str>) {
        let Some(queries) = self.info.borrow().plugin.queries.clone() else {
            return;
        };
        for (target, binding) in self.plugin_targets(layer) {
            if plugin.is_some_and(|plugin| plugin != binding.plugin_id) {
                continue;
            }
            let request = QueryRequest {
                layer: layer.to_owned(),
                target,
                binding,
            };
            if let Err(error) = queries.submit(request) {
                eprintln!("A plugin value was not requested: {error:?}");
            }
        }
    }

    /// A plugin finished an action: what it shows may have changed, so ask again.
    pub(crate) fn plugin_finished(&self, plugin: &str) {
        let layer = self.layers.borrow().active().to_owned();
        self.request_plugin_values(&layer, Some(plugin));
    }
}
