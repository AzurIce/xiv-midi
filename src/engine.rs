use crate::error::{Error, Result};
use crate::keyboard::{Key, KeyboardController};
use crate::mapping::{Action, MappingConfig};
use crate::midi::{MidiEventType, MidiMessage};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiInputPort};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// MIDI engine that processes MIDI events and triggers keyboard actions
pub struct MidiEngine<K: KeyboardController> {
    keyboard: Arc<Mutex<K>>,
    mapping: Arc<Mutex<MappingConfig>>,
    current_modifiers: Arc<Mutex<ModifierState>>,
}

#[derive(Debug, Clone, Copy, Default)]
struct ModifierState {
    shift: bool,
    ctrl: bool,
    alt: bool,
}

impl<K: KeyboardController + 'static> MidiEngine<K> {
    pub fn new(keyboard: K, mapping: MappingConfig) -> Self {
        Self {
            keyboard: Arc::new(Mutex::new(keyboard)),
            mapping: Arc::new(Mutex::new(mapping)),
            current_modifiers: Arc::new(Mutex::new(ModifierState::default())),
        }
    }

    /// List available MIDI input devices
    pub fn list_devices() -> Result<Vec<String>> {
        let midi_in = MidiInput::new("xiv-midi-probe")?;
        let mut devices = Vec::new();

        for port in midi_in.ports() {
            if let Ok(name) = midi_in.port_name(&port) {
                devices.push(name);
            }
        }

        Ok(devices)
    }

    /// Connect to a MIDI device by name
    pub fn connect(&self, device_name: &str) -> Result<MidiInputConnection<()>> {
        let mut midi_in = MidiInput::new("xiv-midi")?;
        midi_in.ignore(Ignore::None);

        // Find the port with matching name
        let port = midi_in
            .ports()
            .into_iter()
            .find(|p| {
                midi_in
                    .port_name(p)
                    .map(|name| name == device_name)
                    .unwrap_or(false)
            })
            .ok_or_else(|| {
                Error::Mapping(format!("Device '{}' not found", device_name))
            })?;

        self.connect_port(port)
    }

    /// Connect to a MIDI device by port
    pub fn connect_port(&self, port: MidiInputPort) -> Result<MidiInputConnection<()>> {
        let mut midi_in = MidiInput::new("xiv-midi")?;

        let keyboard = Arc::clone(&self.keyboard);
        let mapping = Arc::clone(&self.mapping);
        let modifiers = Arc::clone(&self.current_modifiers);

        let connection = midi_in.connect(
            &port,
            "xiv-midi-input",
            move |_timestamp, data, _| {
                if let Err(e) = Self::handle_midi_event(
                    data,
                    &keyboard,
                    &mapping,
                    &modifiers,
                ) {
                    tracing::error!("Error handling MIDI event: {}", e);
                }
            },
            (),
        )?;

        tracing::info!("Connected to MIDI device");
        Ok(connection)
    }

    /// Handle a MIDI event
    fn handle_midi_event(
        data: &[u8],
        keyboard: &Arc<Mutex<K>>,
        mapping: &Arc<Mutex<MappingConfig>>,
        modifiers: &Arc<Mutex<ModifierState>>,
    ) -> Result<()> {
        // Parse MIDI message
        let msg = MidiMessage::parse(data)?;

        tracing::debug!(
            "MIDI event: {:?} {} (vel: {})",
            msg.event_type,
            msg.note,
            msg.velocity
        );

        // Check if we should process this channel
        let mapping = mapping.lock().unwrap();
        if let Some(channel) = mapping.channel {
            if msg.channel != channel {
                return Ok(());
            }
        }

        // Get mapping for this note
        let note_mapping = match mapping.get_mapping(msg.note) {
            Some(m) => m.clone(),
            None => {
                tracing::debug!("No mapping for note {}", msg.note);
                return Ok(());
            }
        };
        drop(mapping);

        // Execute actions based on event type
        let actions = match msg.event_type {
            MidiEventType::NoteOn => &note_mapping.on_press,
            MidiEventType::NoteOff => &note_mapping.on_release,
        };

        Self::execute_actions(actions, keyboard, modifiers)?;

        Ok(())
    }

    /// Execute a sequence of actions
    fn execute_actions(
        actions: &[Action],
        keyboard: &Arc<Mutex<K>>,
        modifiers: &Arc<Mutex<ModifierState>>,
    ) -> Result<()> {
        for action in actions {
            match action {
                Action::Press(key) => {
                    keyboard.lock().unwrap().press(*key)?;
                }
                Action::Release(key) => {
                    keyboard.lock().unwrap().release(*key)?;
                }
                Action::Delay(ms) => {
                    thread::sleep(Duration::from_millis(*ms));
                }
                Action::SetModifiers { shift, ctrl, alt } => {
                    let mut mods = modifiers.lock().unwrap();
                    let mut kb = keyboard.lock().unwrap();

                    // Update Shift
                    if *shift != mods.shift {
                        if *shift {
                            kb.press(Key::Shift)?;
                        } else {
                            kb.release(Key::Shift)?;
                        }
                        mods.shift = *shift;
                    }

                    // Update Ctrl
                    if *ctrl != mods.ctrl {
                        if *ctrl {
                            kb.press(Key::Control)?;
                        } else {
                            kb.release(Key::Control)?;
                        }
                        mods.ctrl = *ctrl;
                    }

                    // Update Alt
                    if *alt != mods.alt {
                        if *alt {
                            kb.press(Key::Alt)?;
                        } else {
                            kb.release(Key::Alt)?;
                        }
                        mods.alt = *alt;
                    }

                    // Small delay to let modifiers register
                    if *shift || *ctrl || *alt {
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            }
        }

        Ok(())
    }

    /// Release all keys
    pub fn release_all(&self) -> Result<()> {
        self.keyboard.lock().unwrap().release_all()
    }
}
