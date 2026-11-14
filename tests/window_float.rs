//! Public-API checks for the Hyprland floating request.

use sdeck_plus::window_float::{batch, command_socket, Area};
use std::ffi::OsStr;

#[test]
fn outside_hyprland_there_is_no_socket() {
    assert!(command_socket(Some(OsStr::new("/run/user/1000")), None).is_none());
}

#[test]
fn default_window_is_centered_on_a_full_hd_monitor() {
    let area = Area {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    assert!(batch("0x1", area, 1120, 700).ends_with("movewindowpixel exact 400 190,address:0x1"));
}
