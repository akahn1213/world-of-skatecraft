//! Xbox packets from the same XInput/evdev mechanism as the pinned Skate 3 bridge.

mod platform;

/// Raw XInput conventions: positive Y is up, triggers are 0..=255.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XboxState {
    pub buttons: u16,
    pub triggers: [u8; 2],
    pub left: [i16; 2],
    pub right: [i16; 2],
}

#[derive(Default)]
pub struct ControllerTransport {
    #[cfg(windows)]
    capabilities: [platform::CapabilityCache; 4],
}

impl ControllerTransport {
    /// Select the first successful slot, exactly as skate-host's InputFrame does.
    /// Every control comes from this one packet; errors never retain held input.
    pub fn poll(&mut self) -> Option<(usize, XboxState)> {
        first_connected(std::array::from_fn(|slot| {
            #[cfg(windows)]
            return platform::poll(slot, &mut self.capabilities[slot]);
            #[cfg(not(windows))]
            platform::poll(slot)
        }))
    }
}

fn first_connected(
    samples: [Result<XboxState, platform::DeviceError>; 4],
) -> Option<(usize, XboxState)> {
    samples
        .into_iter()
        .enumerate()
        .find_map(|(slot, sample)| sample.ok().map(|state| (slot, state)))
}

/// Tarnished's reference stick deadzones and the former Bevy trigger hysteresis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CombatFrame {
    pub buttons: u16,
    pub pressed: u16,
    pub movement: [f32; 2],
    pub look: [f32; 2],
    pub heavy_attack: bool,
}

#[derive(Default)]
pub struct CombatInput {
    slot: Option<usize>,
    buttons: u16,
    heavy_attack: bool,
}

impl CombatInput {
    pub fn sample(&mut self, packet: Option<(usize, XboxState)>) -> CombatFrame {
        let Some((slot, state)) = packet else {
            *self = Self::default();
            return CombatFrame::default();
        };
        let changed = self.slot.is_some_and(|previous| previous != slot);
        let pressed = if changed {
            0
        } else {
            state.buttons & !self.buttons
        };
        if changed {
            self.heavy_attack = false;
        }
        let trigger = f32::from(state.triggers[1]) / 255.0;
        if trigger >= 0.75 {
            self.heavy_attack = true;
        } else if trigger <= 0.65 {
            self.heavy_attack = false;
        }
        self.slot = Some(slot);
        self.buttons = state.buttons;
        CombatFrame {
            buttons: state.buttons,
            pressed,
            // Pinned ER sim/data.rs: STICK_DEADZONE = 0.1; camera.rs: length > 0.15.
            movement: stick(state.left, 0.1),
            look: stick(state.right, 0.15),
            heavy_attack: self.heavy_attack,
        }
    }
}

fn stick(raw: [i16; 2], deadzone: f32) -> [f32; 2] {
    let normalized = raw.map(|axis| f32::from(axis) / 32768.0);
    if normalized[0].hypot(normalized[1]) <= deadzone {
        [0.0; 2]
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_keeps_buttons_sticks_and_triggers_in_one_packet() {
        let state = XboxState {
            buttons: 0x0200,
            triggers: [0, 255],
            left: [1234, -5678],
            right: [-2345, 6789],
        };
        assert_eq!(
            first_connected([
                Err(platform::DeviceError::Disconnected),
                Ok(state),
                Ok(XboxState::default()),
                Err(platform::DeviceError::Disconnected),
            ]),
            Some((1, state))
        );
        assert_eq!(
            first_connected(std::array::from_fn(|_| Err(
                platform::DeviceError::Disconnected
            ))),
            None
        );
    }

    #[test]
    fn sticks_preserve_direction_and_reference_radial_deadzones() {
        assert_eq!(stick([0, 0], 0.1), [0.0; 2]);
        assert_eq!(stick([3000, 0], 0.1), [0.0; 2]);
        assert_eq!(stick([0, 4900], 0.15), [0.0; 2]);
        // A diagonal outside the radial deadzone must not be filtered per axis.
        assert_ne!(stick([2500, 2500], 0.1), [0.0; 2]);
        let mut input = CombatInput::default();
        let frame = input.sample(Some((
            0,
            XboxState {
                left: [i16::MIN, i16::MAX],
                right: [i16::MAX, i16::MIN],
                ..Default::default()
            },
        )));
        assert_eq!(frame.movement, [-1.0, 32767.0 / 32768.0]);
        assert_eq!(frame.look, [32767.0 / 32768.0, -1.0]);
    }

    #[test]
    fn trigger_hysteresis_and_disconnect_clear_all_held_input() {
        let mut input = CombatInput::default();
        let sample = |trigger| {
            Some((
                0,
                XboxState {
                    buttons: 0x0210,
                    left: [32767, 32767],
                    right: [32767, 32767],
                    triggers: [255, trigger],
                },
            ))
        };
        assert!(!input.sample(sample(191)).heavy_attack);
        assert!(input.sample(sample(192)).heavy_attack);
        assert!(input.sample(sample(166)).heavy_attack);
        assert!(!input.sample(sample(165)).heavy_attack);
        assert!(input.sample(sample(255)).heavy_attack);
        assert_eq!(input.sample(None), CombatFrame::default());
        assert!(!input.sample(sample(166)).heavy_attack);
        assert_eq!(input.sample(sample(166)).pressed, 0);
    }

    #[test]
    fn pad_handoff_does_not_generate_toggle_or_retain_trigger_latch() {
        let mut input = CombatInput::default();
        let first = input.sample(Some((
            0,
            XboxState {
                buttons: 0x0010,
                triggers: [0, 255],
                ..Default::default()
            },
        )));
        assert_eq!(first.pressed, 0x0010);
        let replacement = input.sample(Some((
            1,
            XboxState {
                buttons: 0x0090,
                triggers: [0, 180],
                left: [16000, 0],
                ..Default::default()
            },
        )));
        assert_eq!(replacement.pressed, 0);
        assert!(!replacement.heavy_attack);
        assert_eq!(replacement.movement, [16000.0 / 32768.0, 0.0]);
    }
}
