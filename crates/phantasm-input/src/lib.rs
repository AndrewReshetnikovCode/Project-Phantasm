use std::collections::HashMap;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputEvent {
    pub frame: u64,
    pub action: String,
    pub pressed: bool,
}

pub struct InputSystem {
    action_map: HashMap<String, Vec<KeyCode>>,
    just_pressed: HashMap<String, bool>,
    recording: Vec<InputEvent>,
    is_recording: bool,
}

impl InputSystem {
    pub fn new() -> Self {
        let mut system = Self {
            action_map: HashMap::new(),
            just_pressed: HashMap::new(),
            recording: Vec::new(),
            is_recording: false,
        };

        system.bind("move_up", vec![KeyCode::Char('w'), KeyCode::Up]);
        system.bind("move_down", vec![KeyCode::Char('s'), KeyCode::Down]);
        system.bind("move_left", vec![KeyCode::Char('a'), KeyCode::Left]);
        system.bind("move_right", vec![KeyCode::Char('d'), KeyCode::Right]);
        system.bind("quit", vec![KeyCode::Char('q'), KeyCode::Esc]);
        system.bind("interact", vec![KeyCode::Char('e'), KeyCode::Enter]);
        system
    }

    pub fn bind(&mut self, action: &str, keys: Vec<KeyCode>) {
        self.action_map.insert(action.to_string(), keys);
        self.just_pressed.insert(action.to_string(), false);
    }

    pub fn poll(&mut self, frame: u64) -> bool {
        for val in self.just_pressed.values_mut() {
            *val = false;
        }

        while event::poll(Duration::from_millis(0)).unwrap_or(false) {
            if let Ok(Event::Key(key_event)) = event::read() {
                if key_event.kind == KeyEventKind::Press {
                    self.handle_key(key_event, frame);
                }
            }
        }

        self.is_action_pressed("quit")
    }

    fn handle_key(&mut self, event: KeyEvent, frame: u64) {
        for (action, keys) in &self.action_map {
            if keys.contains(&event.code) {
                self.just_pressed.insert(action.clone(), true);
                if self.is_recording {
                    self.recording.push(InputEvent {
                        frame,
                        action: action.clone(),
                        pressed: true,
                    });
                }
            }
        }
    }

    pub fn is_action_pressed(&self, action: &str) -> bool {
        *self.just_pressed.get(action).unwrap_or(&false)
    }

    pub fn pressed_actions(&self) -> Vec<String> {
        self.just_pressed
            .iter()
            .filter(|(_, &v)| v)
            .map(|(k, _)| k.clone())
            .collect()
    }

    pub fn start_recording(&mut self) {
        self.recording.clear();
        self.is_recording = true;
    }

    pub fn stop_recording(&mut self) -> Vec<InputEvent> {
        self.is_recording = false;
        std::mem::take(&mut self.recording)
    }

    pub fn recording_to_json(&self) -> String {
        serde_json::to_string_pretty(&self.recording).unwrap_or_default()
    }
}

impl Default for InputSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bindings_exist() {
        let input = InputSystem::new();
        assert!(input.action_map.contains_key("move_up"));
        assert!(input.action_map.contains_key("quit"));
    }

    #[test]
    fn custom_binding() {
        let mut input = InputSystem::new();
        input.bind("attack", vec![KeyCode::Char('x')]);
        assert!(input.action_map.contains_key("attack"));
    }

    #[test]
    fn pressed_actions_empty_initially() {
        let input = InputSystem::new();
        assert!(input.pressed_actions().is_empty());
    }
}
