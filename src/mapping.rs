use crate::keyboard::Key;
use crate::midi::MidiNote;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Action to perform when a MIDI event occurs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    /// Press a key
    Press(Key),
    /// Release a key
    Release(Key),
    /// Wait for a duration
    Delay(u64), // milliseconds
    /// Set modifiers for the following actions
    SetModifiers { shift: bool, ctrl: bool, alt: bool },
}

/// Mapping from a MIDI note to keyboard actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteMapping {
    /// Actions to perform when note is pressed
    pub on_press: Vec<Action>,
    /// Actions to perform when note is released
    pub on_release: Vec<Action>,
}

impl Default for NoteMapping {
    fn default() -> Self {
        Self {
            on_press: Vec::new(),
            on_release: Vec::new(),
        }
    }
}

/// MIDI to keyboard mapping configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingConfig {
    /// Channel to listen to (0-15, None = all channels)
    pub channel: Option<u8>,
    /// Note mappings
    pub mappings: HashMap<u8, NoteMapping>,
}

impl MappingConfig {
    pub fn new() -> Self {
        Self {
            channel: Some(0),
            mappings: HashMap::new(),
        }
    }

    /// Get mapping for a specific note
    pub fn get_mapping(&self, note: MidiNote) -> Option<&NoteMapping> {
        self.mappings.get(&note.value())
    }

    /// Add a mapping for a note
    pub fn add_mapping(&mut self, note: MidiNote, mapping: NoteMapping) {
        self.mappings.insert(note.value(), mapping);
    }

    /// Load from JSON file
    pub fn from_file(path: &std::path::Path) -> crate::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config = serde_json::from_str(&content)?;
        Ok(config)
    }

    /// Save to JSON file
    pub fn to_file(&self, path: &std::path::Path) -> crate::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

impl Default for MappingConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate default FFXIV mapping
/// Maps 3 octaves (C3-C6) to keyboard keys with modifiers:
/// - C3-B3: Ctrl + key
/// - C4-B4: key (no modifier)
/// - C5-B5: Shift + key
pub fn create_ffxiv_default_mapping() -> MappingConfig {
    let mut config = MappingConfig::new();

    // FFXIV performance keyboard layout:
    // Q 2 W 3 E R 5 T 6 Y 7 U I
    let keys = [
        Key::Q,
        Key::Num2,
        Key::W,
        Key::Num3,
        Key::E,
        Key::R,
        Key::Num5,
        Key::T,
        Key::Num6,
        Key::Y,
        Key::Num7,
        Key::U,
        Key::I,
    ];

    // C3 (MIDI 48) to C4 (MIDI 60) - with Ctrl
    for (i, key) in keys.iter().enumerate() {
        let note = MidiNote::new(48 + i as u8).unwrap();
        let mapping = NoteMapping {
            on_press: vec![
                Action::SetModifiers {
                    shift: false,
                    ctrl: true,
                    alt: false,
                },
                Action::Press(*key),
            ],
            on_release: vec![
                Action::Release(*key),
                Action::SetModifiers {
                    shift: false,
                    ctrl: false,
                    alt: false,
                },
            ],
        };
        config.add_mapping(note, mapping);
    }

    // C4 (MIDI 60) to C5 (MIDI 72) - no modifier
    for (i, key) in keys.iter().enumerate() {
        let note = MidiNote::new(60 + i as u8).unwrap();
        let mapping = NoteMapping {
            on_press: vec![Action::Press(*key)],
            on_release: vec![Action::Release(*key)],
        };
        config.add_mapping(note, mapping);
    }

    // C5 (MIDI 72) to C6 (MIDI 84) - with Shift
    for (i, key) in keys.iter().enumerate() {
        let note = MidiNote::new(72 + i as u8).unwrap();
        let mapping = NoteMapping {
            on_press: vec![
                Action::SetModifiers {
                    shift: true,
                    ctrl: false,
                    alt: false,
                },
                Action::Press(*key),
            ],
            on_release: vec![
                Action::Release(*key),
                Action::SetModifiers {
                    shift: false,
                    ctrl: false,
                    alt: false,
                },
            ],
        };
        config.add_mapping(note, mapping);
    }

    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_mapping() {
        let config = create_ffxiv_default_mapping();

        // Check C4 (middle C, note 60) maps to Q
        let note = MidiNote::new(60).unwrap();
        let mapping = config.get_mapping(note).unwrap();
        assert_eq!(mapping.on_press.len(), 1);

        // Check C3 (note 48) has Ctrl modifier
        let note = MidiNote::new(48).unwrap();
        let mapping = config.get_mapping(note).unwrap();
        assert!(mapping.on_press.len() >= 2);
    }
}
