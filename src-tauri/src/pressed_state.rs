use crate::{
    protocol_v2::{CriticalButton, CriticalEvent},
    shared_input::{InputCommand, MouseButton},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeySource {
    ScanCode { scan_code: u16, extended: bool },
    VirtualKey(u16),
}

impl KeySource {
    fn from_event(key_code: u16, scan_code: u16, extended: bool) -> Self {
        if scan_code == 0 {
            Self::VirtualKey(key_code)
        } else {
            Self::ScanCode {
                scan_code,
                extended,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HeldKey {
    source: KeySource,
    target_key_code: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HeldButton {
    button: MouseButton,
    x: i32,
    y: i32,
}

#[derive(Clone, Default)]
pub struct PressedState {
    keys: Vec<HeldKey>,
    buttons: Vec<HeldButton>,
}

impl PressedState {
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty() && self.buttons.is_empty()
    }

    pub fn drag_button(&self) -> Option<MouseButton> {
        [MouseButton::Left, MouseButton::Right, MouseButton::Middle]
            .into_iter()
            .find(|button| self.buttons.iter().any(|held| held.button == *button))
    }

    pub fn update_pointer(&mut self, x: i32, y: i32) {
        for held in &mut self.buttons {
            held.x = x;
            held.y = y;
        }
    }

    pub fn apply(&mut self, event: &CriticalEvent) -> Vec<InputCommand> {
        match *event {
            CriticalEvent::Key {
                key_code,
                scan_code,
                extended,
                down,
            } => self.apply_key(key_code, scan_code, extended, down),
            CriticalEvent::Button {
                button, down, x, y, ..
            } => self.apply_button(button.into(), down, x, y),
            CriticalEvent::Scroll {
                delta_x, delta_y, ..
            } => vec![InputCommand::Scroll { delta_x, delta_y }],
        }
    }

    fn apply_key(
        &mut self,
        key_code: u16,
        scan_code: u16,
        extended: bool,
        down: bool,
    ) -> Vec<InputCommand> {
        let source = KeySource::from_event(key_code, scan_code, extended);
        if down {
            if self.keys.iter().any(|held| held.source == source) {
                return vec![];
            }
            let target_was_held = self
                .keys
                .iter()
                .any(|held| held.target_key_code == key_code);
            self.keys.push(HeldKey {
                source,
                target_key_code: key_code,
            });
            if target_was_held {
                vec![]
            } else {
                vec![InputCommand::Key {
                    key_code,
                    down: true,
                }]
            }
        } else {
            let Some(index) = self.keys.iter().position(|held| held.source == source) else {
                return vec![];
            };
            let held = self.keys.remove(index);
            if self
                .keys
                .iter()
                .any(|other| other.target_key_code == held.target_key_code)
            {
                vec![]
            } else {
                vec![InputCommand::Key {
                    key_code: held.target_key_code,
                    down: false,
                }]
            }
        }
    }

    fn apply_button(
        &mut self,
        button: MouseButton,
        down: bool,
        x: i32,
        y: i32,
    ) -> Vec<InputCommand> {
        if down {
            if let Some(held) = self.buttons.iter_mut().find(|held| held.button == button) {
                held.x = x;
                held.y = y;
                return vec![];
            }
            self.buttons.push(HeldButton { button, x, y });
        } else if let Some(index) = self.buttons.iter().position(|held| held.button == button) {
            self.buttons.remove(index);
        } else {
            return vec![];
        }
        vec![InputCommand::MouseButton { button, down, x, y }]
    }

    pub fn release_all(&mut self) -> Vec<InputCommand> {
        let mut commands = Vec::new();
        while let Some(held) = self.keys.pop() {
            if !self
                .keys
                .iter()
                .any(|other| other.target_key_code == held.target_key_code)
            {
                commands.push(InputCommand::Key {
                    key_code: held.target_key_code,
                    down: false,
                });
            }
        }
        for held in self.buttons.drain(..).rev() {
            commands.push(InputCommand::MouseButton {
                button: held.button,
                down: false,
                x: held.x,
                y: held.y,
            });
        }
        commands
    }
}

impl From<CriticalButton> for MouseButton {
    fn from(button: CriticalButton) -> Self {
        match button {
            CriticalButton::Left => Self::Left,
            CriticalButton::Right => Self::Right,
            CriticalButton::Middle => Self::Middle,
            CriticalButton::Back => Self::Back,
            CriticalButton::Forward => Self::Forward,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key_code: u16, scan_code: u16, down: bool) -> CriticalEvent {
        CriticalEvent::Key {
            key_code,
            scan_code,
            extended: false,
            down,
        }
    }

    #[test]
    fn a19_key_up_uses_mapping_frozen_at_key_down() {
        let mut state = PressedState::default();
        assert_eq!(
            state.apply(&key(0x11, 30, true)),
            vec![InputCommand::Key {
                key_code: 0x11,
                down: true
            }]
        );
        assert_eq!(
            state.apply(&key(0x5b, 30, false)),
            vec![InputCommand::Key {
                key_code: 0x11,
                down: false
            }]
        );
    }

    #[test]
    fn a20_repeat_and_two_sources_do_not_release_target_early() {
        let mut state = PressedState::default();
        assert_eq!(state.apply(&key(0x11, 29, true)).len(), 1);
        assert!(state.apply(&key(0x11, 29, true)).is_empty());
        assert!(state.apply(&key(0x11, 30, true)).is_empty());
        assert!(state.apply(&key(0x11, 29, false)).is_empty());
        assert_eq!(
            state.apply(&key(0x11, 30, false)),
            vec![InputCommand::Key {
                key_code: 0x11,
                down: false
            }]
        );
        assert!(state.release_all().is_empty());
    }

    #[test]
    fn release_all_emits_each_target_once_and_uses_button_position() {
        let mut state = PressedState::default();
        state.apply(&key(0x11, 29, true));
        state.apply(&key(0x11, 30, true));
        state.apply(&CriticalEvent::Button {
            button: CriticalButton::Left,
            down: true,
            x: 700,
            y: 400,
            motion_sequence: 1,
        });
        assert_eq!(
            state.release_all(),
            vec![
                InputCommand::Key {
                    key_code: 0x11,
                    down: false,
                },
                InputCommand::MouseButton {
                    button: MouseButton::Left,
                    down: false,
                    x: 700,
                    y: 400,
                },
            ]
        );
        assert!(state.release_all().is_empty());
    }

    #[test]
    fn motion_tracks_drag_button_and_updates_release_position() {
        let mut state = PressedState::default();
        state.apply(&CriticalEvent::Button {
            button: CriticalButton::Left,
            down: true,
            x: 10,
            y: 20,
            motion_sequence: 1,
        });
        assert_eq!(state.drag_button(), Some(MouseButton::Left));
        state.update_pointer(80, 90);
        assert_eq!(
            state.release_all(),
            vec![InputCommand::MouseButton {
                button: MouseButton::Left,
                down: false,
                x: 80,
                y: 90,
            }]
        );
        assert_eq!(state.drag_button(), None);
    }
}
