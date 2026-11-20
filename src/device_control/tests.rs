use super::*;
use crate::device_model::{MINI, PLUS, PLUS_XL};

#[test]
fn feature_report_matches_plus_protocol_when_brightness_is_set() {
    // Given
    let mut state = DesiredState::default();
    // When
    state.apply(&DeviceAction::Brightness(73)).unwrap();
    // Then: report ID, command, percentage, and 29 zero bytes, on + and + XL.
    for model in [&PLUS, &PLUS_XL] {
        assert_eq!(
            state.report(model.family),
            [
                0x03, 0x08, 73, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0,
            ],
            "{}",
            model.id
        );
    }
}

#[test]
fn legacy_mini_intent_uses_the_mini_brightness_report() {
    // Given
    let mut state = DesiredState::default();
    let mini = |level: u8| {
        let mut report = [0u8; 32];
        report[..6].copy_from_slice(&[0x05, 0x55, 0xAA, 0xD1, 0x01, level]);
        report
    };
    assert_eq!(state.report(MINI.family), mini(100));
    // When / Then: the same intent, in the Mini's own bytes.
    state.apply(&DeviceAction::Brightness(73)).unwrap();
    assert_eq!(state.report(MINI.family), mini(73));
    state.apply(&DeviceAction::Sleep).unwrap();
    assert_eq!(state.report(MINI.family), mini(0));
    state.apply(&DeviceAction::Wake).unwrap();
    assert_eq!(state.report(MINI.family), mini(73));
}

#[test]
fn adjustment_clamps_when_steps_cross_percentage_bounds() {
    // Given
    for (initial, step, expected) in [(95, 127, 100), (5, -128, 0), (40, -10, 30)] {
        let mut state = DesiredState::default();
        state.apply(&DeviceAction::Brightness(initial)).unwrap();
        // When
        state.apply(&DeviceAction::AdjustBrightness(step)).unwrap();
        // Then
        assert_eq!(state.report(PLUS.family)[2], expected);
    }
}

#[test]
fn wake_restores_when_sleep_is_repeated_and_brightness_changes_asleep() {
    // Given
    let mut state = DesiredState::default();
    state.apply(&DeviceAction::Brightness(42)).unwrap();
    state.apply(&DeviceAction::Sleep).unwrap();
    state.apply(&DeviceAction::Sleep).unwrap();
    state.apply(&DeviceAction::AdjustBrightness(8)).unwrap();
    assert_eq!(state.report(PLUS.family)[2], 0);
    // When
    state.apply(&DeviceAction::Wake).unwrap();
    // Then
    assert_eq!(state.report(PLUS.family)[2], 50);
}

#[test]
fn zero_is_restored_when_waking_from_explicit_zero_brightness() {
    // Given
    let mut state = DesiredState::default();
    state.apply(&DeviceAction::Brightness(0)).unwrap();
    state.apply(&DeviceAction::Sleep).unwrap();
    // When
    state.apply(&DeviceAction::Wake).unwrap();
    // Then
    assert_eq!(state.report(PLUS.family)[2], 0);
}

#[test]
fn invalid_absolute_brightness_preserves_previous_intent() {
    // Given
    let mut state = DesiredState::default();
    state.apply(&DeviceAction::Brightness(23)).unwrap();
    // When
    let error = state.apply(&DeviceAction::Brightness(101)).unwrap_err();
    // Then
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(state.report(PLUS.family)[2], 23);
    assert_eq!(state.action, Some(DeviceAction::Brightness(23)));
}

#[test]
fn relative_adjustment_uses_baseline_when_no_absolute_level_was_requested() {
    // Given
    let mut state = DesiredState::default();
    assert!(state.action.is_none());
    // When
    state.apply(&DeviceAction::AdjustBrightness(-5)).unwrap();
    // Then
    assert_eq!(state.report(PLUS.family)[2], 95);
}

#[test]
fn actions_round_trip_when_serialized() {
    // Given
    for action in [
        DeviceAction::Brightness(60),
        DeviceAction::AdjustBrightness(-10),
        DeviceAction::Sleep,
        DeviceAction::Wake,
    ] {
        // When
        let encoded = serde_json::to_vec(&action).unwrap();
        let decoded: DeviceAction = serde_json::from_slice(&encoded).unwrap();
        // Then
        assert_eq!(decoded, action);
        assert_eq!(action.clone(), action);
        assert!(action.validate().is_ok());
    }
}
