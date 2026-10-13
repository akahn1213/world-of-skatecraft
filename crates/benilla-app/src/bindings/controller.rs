//! Fork controller support: vanilla has keyboard bindings, but no native D-pad bindings.
//! Resolve D-pad presses as number keys through the same command table, with independent
//! latches so releasing a controller button cannot release a physical keyboard key.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use std::sync::Mutex;

pub(super) const NUMBERS: [(u16, KeyCode, char); 4] = [
    (0x0004, KeyCode::Digit1, '1'), // Left
    (0x0001, KeyCode::Digit2, '2'), // Up
    (0x0008, KeyCode::Digit3, '3'), // Right
    (0x0002, KeyCode::Digit4, '4'), // Down
];

#[derive(Resource, Default)]
pub(super) struct Dpad {
    pub held: u16,
    pub pressed: u16,
}

pub(super) fn poll(
    mut transport: Local<Mutex<benilla_controller::ControllerTransport>>,
    mut input: Local<benilla_controller::CombatInput>,
    mut dpad: ResMut<Dpad>,
    window: Query<&Window, With<PrimaryWindow>>,
    skate: Option<Res<crate::skate::SkateDrive>>,
) {
    // Sample even while unfocused, so a held direction doesn't become a new press on return.
    // The shared native adapter supports Windows XInput and Linux evdev, like Tarnished.
    let frame = input.sample(transport.get_mut().unwrap().poll());
    let focused = window.single().is_ok_and(|window| window.focused);
    // While skating, LB+D-pad belongs to the engine's session markers.
    let skate_markers = skate.is_some_and(|drive| drive.active) && frame.buttons & 0x0100 != 0;
    *dpad = if focused && !skate_markers {
        Dpad {
            held: frame.buttons & 0x000f,
            pressed: frame.pressed & 0x000f,
        }
    } else {
        Dpad::default()
    };
}
