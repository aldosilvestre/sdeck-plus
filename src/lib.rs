//! In-process API for observing Stream Deck + HID input.

pub mod asset_links;
pub mod binding_editor;
pub mod binding_icons;
pub mod button_text;
pub mod config_bundle;
pub mod desktop_actions;
pub mod device_control;
pub mod event_api;
pub mod features;
pub mod image_output;
pub mod info;
pub mod info_render;
pub mod keyboard;
pub mod layers;
pub mod local_artwork;
pub mod obs;
pub mod organization;
pub mod protocol;

pub mod app_focus;
#[cfg(test)]
mod feature_persistence_tests;
#[cfg(test)]
mod touchscreen_navigation_tests;
#[cfg(test)]
mod touchscreen_tests;
