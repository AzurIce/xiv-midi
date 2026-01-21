use crate::error::{Error, Result};

/// MIDI note number (0-127)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MidiNote(u8);

impl MidiNote {
    pub fn new(note: u8) -> Result<Self> {
        if note > 127 {
            return Err(Error::InvalidMidiMessage(format!(
                "Note number {} out of range (0-127)",
                note
            )));
        }
        Ok(Self(note))
    }

    pub fn value(&self) -> u8 {
        self.0
    }

    /// Get the octave number (-1 to 9)
    pub fn octave(&self) -> i8 {
        (self.0 as i8 / 12) - 1
    }

    /// Get the note name (C, C#, D, etc.)
    pub fn name(&self) -> &'static str {
        const NAMES: [&str; 12] = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        NAMES[self.0 as usize % 12]
    }

    /// Get full note name with octave (e.g., "C4", "A#3")
    pub fn full_name(&self) -> String {
        format!("{}{}", self.name(), self.octave())
    }
}

impl std::fmt::Display for MidiNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.full_name())
    }
}

/// MIDI event type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiEventType {
    NoteOn,
    NoteOff,
}

/// Parsed MIDI message
#[derive(Debug, Clone, PartialEq)]
pub struct MidiMessage {
    pub event_type: MidiEventType,
    pub channel: u8,
    pub note: MidiNote,
    pub velocity: u8,
}

impl MidiMessage {
    /// Parse a raw MIDI message
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 3 {
            return Err(Error::InvalidMidiMessage(format!(
                "Message too short: {} bytes",
                data.len()
            )));
        }

        let status = data[0];
        let message_type = status & 0xF0;
        let channel = status & 0x0F;

        match message_type {
            0x80 => {
                // Note Off
                Ok(Self {
                    event_type: MidiEventType::NoteOff,
                    channel,
                    note: MidiNote::new(data[1])?,
                    velocity: data[2],
                })
            }
            0x90 => {
                // Note On (or Note Off if velocity is 0)
                let velocity = data[2];
                let event_type = if velocity == 0 {
                    MidiEventType::NoteOff
                } else {
                    MidiEventType::NoteOn
                };
                Ok(Self {
                    event_type,
                    channel,
                    note: MidiNote::new(data[1])?,
                    velocity,
                })
            }
            _ => Err(Error::InvalidMidiMessage(format!(
                "Unsupported message type: 0x{:02X}",
                message_type
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_midi_note() {
        let c4 = MidiNote::new(60).unwrap();
        assert_eq!(c4.octave(), 4);
        assert_eq!(c4.name(), "C");
        assert_eq!(c4.full_name(), "C4");
    }

    #[test]
    fn test_midi_message_parse() {
        // Note On C4 with velocity 64
        let msg = MidiMessage::parse(&[0x90, 60, 64]).unwrap();
        assert_eq!(msg.event_type, MidiEventType::NoteOn);
        assert_eq!(msg.channel, 0);
        assert_eq!(msg.note.value(), 60);
        assert_eq!(msg.velocity, 64);

        // Note Off C4
        let msg = MidiMessage::parse(&[0x80, 60, 0]).unwrap();
        assert_eq!(msg.event_type, MidiEventType::NoteOff);

        // Note On with velocity 0 (treated as Note Off)
        let msg = MidiMessage::parse(&[0x90, 60, 0]).unwrap();
        assert_eq!(msg.event_type, MidiEventType::NoteOff);
    }
}
