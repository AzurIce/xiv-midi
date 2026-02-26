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
    /// Whether to transpose out-of-range notes by octaves to fit within the mapped range
    #[serde(default)]
    pub octave_transpose: bool,
}

impl MappingConfig {
    pub fn new() -> Self {
        Self {
            channel: Some(0),
            mappings: HashMap::new(),
            octave_transpose: false,
        }
    }

    /// Get mapping for a specific note
    pub fn get_mapping(&self, note: MidiNote) -> Option<&NoteMapping> {
        self.mappings.get(&note.value())
    }

    /// Get mapping for a note, with octave transposition if enabled.
    /// If the note has no direct mapping and `octave_transpose` is true,
    /// shifts the note up/down by octaves until a mapping is found.
    pub fn get_mapping_transposed(&self, note: MidiNote) -> Option<(MidiNote, &NoteMapping)> {
        // Direct lookup first
        if let Some(m) = self.mappings.get(&note.value()) {
            return Some((note, m));
        }

        if !self.octave_transpose {
            return None;
        }

        // Find the range of mapped notes
        let min_mapped = *self.mappings.keys().min()?;
        let max_mapped = *self.mappings.keys().max()?;

        let mut candidate = note.value();

        // Try shifting toward the mapped range
        if candidate < min_mapped {
            // Shift up by octaves
            while candidate + 12 <= 127 {
                candidate += 12;
                if let Some(m) = self.mappings.get(&candidate) {
                    return MidiNote::new(candidate).ok().map(|n| (n, m));
                }
            }
        } else if candidate > max_mapped {
            // Shift down by octaves
            while candidate >= 12 {
                candidate -= 12;
                if let Some(m) = self.mappings.get(&candidate) {
                    return MidiNote::new(candidate).ok().map(|n| (n, m));
                }
            }
        } else {
            // Note is within the overall range but has no mapping at this octave.
            // Try the nearest octave shifts (down first, then up).
            let mut down = note.value();
            let mut up = note.value();
            loop {
                let can_down = down >= 12;
                let can_up = up + 12 <= 127;
                if !can_down && !can_up {
                    break;
                }
                if can_down {
                    down -= 12;
                    if let Some(m) = self.mappings.get(&down) {
                        return MidiNote::new(down).ok().map(|n| (n, m));
                    }
                }
                if can_up {
                    up += 12;
                    if let Some(m) = self.mappings.get(&up) {
                        return MidiNote::new(up).ok().map(|n| (n, m));
                    }
                }
            }
        }

        None
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
