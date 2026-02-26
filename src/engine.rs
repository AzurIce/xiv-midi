use crate::error::{Error, Result};
use crate::keyboard::{Key, KeyboardController};
use crate::mapping::{Action, MappingConfig};
use crate::midi::{MidiEventType, MidiMessage};
use crossbeam_channel::{self as channel};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiInputPort};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Default minimum gap between consecutive note-on keypresses.
/// FF14 needs a small window to distinguish two keypresses.
const DEFAULT_MIN_NOTE_GAP: Duration = Duration::from_millis(3);

/// Delay after changing modifier keys to let them register.
const MODIFIER_SETTLE_DELAY: Duration = Duration::from_millis(3);

/// MIDI engine that processes MIDI events and triggers keyboard actions
pub struct MidiEngine<K: KeyboardController> {
    keyboard: Arc<Mutex<K>>,
    mapping: Arc<Mutex<MappingConfig>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ModifierState {
    shift: bool,
    ctrl: bool,
    alt: bool,
}

/// Tracks the currently playing note so we can auto-release before the next one.
struct NoteScheduler {
    /// The key currently held down (if any)
    current_key: Option<Key>,
    /// Modifier state currently applied
    current_modifiers: ModifierState,
    /// When the last note-on keypress was sent
    last_note_time: Instant,
    /// Minimum gap between consecutive note-on events
    min_note_gap: Duration,
}

impl NoteScheduler {
    fn new() -> Self {
        Self {
            current_key: None,
            current_modifiers: ModifierState::default(),
            last_note_time: Instant::now() - Duration::from_secs(1), // far in the past
            min_note_gap: DEFAULT_MIN_NOTE_GAP,
        }
    }

    /// Release the currently playing note (if any).
    fn release_current<K: KeyboardController>(&mut self, kb: &mut K) -> Result<()> {
        if let Some(key) = self.current_key.take() {
            kb.release(key)?;
        }
        Ok(())
    }

    /// Ensure the minimum gap since the last note-on has elapsed.
    fn wait_min_gap(&self) {
        let elapsed = self.last_note_time.elapsed();
        if elapsed < self.min_note_gap {
            thread::sleep(self.min_note_gap - elapsed);
        }
    }

    /// Set modifier keys to the desired state, only sending changes.
    fn set_modifiers<K: KeyboardController>(
        &mut self,
        desired: ModifierState,
        kb: &mut K,
    ) -> Result<()> {
        let cur = self.current_modifiers;
        let mut changed = false;

        if desired.shift != cur.shift {
            if desired.shift {
                kb.press(Key::Shift)?;
            } else {
                kb.release(Key::Shift)?;
            }
            changed = true;
        }

        if desired.ctrl != cur.ctrl {
            if desired.ctrl {
                kb.press(Key::Control)?;
            } else {
                kb.release(Key::Control)?;
            }
            changed = true;
        }

        if desired.alt != cur.alt {
            if desired.alt {
                kb.press(Key::Alt)?;
            } else {
                kb.release(Key::Alt)?;
            }
            changed = true;
        }

        self.current_modifiers = desired;

        // Only sleep if modifiers actually changed and at least one is active
        if changed && (desired.shift || desired.ctrl || desired.alt) {
            thread::sleep(MODIFIER_SETTLE_DELAY);
        }

        Ok(())
    }

    /// Play a new note: auto-release previous, enforce gap, set modifiers, press key.
    fn play_note<K: KeyboardController>(&mut self, actions: &[Action], kb: &mut K) -> Result<()> {
        // Pre-scan: extract the target modifier state and key from the action list
        // so we can do the smart release-before-press logic.
        let mut target_mods: Option<ModifierState> = None;
        let mut target_key: Option<Key> = None;

        for action in actions {
            match action {
                Action::SetModifiers { shift, ctrl, alt } => {
                    target_mods = Some(ModifierState {
                        shift: *shift,
                        ctrl: *ctrl,
                        alt: *alt,
                    });
                }
                Action::Press(key) => {
                    target_key = Some(*key);
                }
                _ => {}
            }
        }

        // If this is a note-on (has a Press action), do the smart scheduling
        if let Some(key) = target_key {
            // 1. Release the previous note first
            self.release_current(kb)?;

            // 2. Enforce minimum gap between note-on events
            self.wait_min_gap();

            // 3. Set modifiers
            if let Some(mods) = target_mods {
                self.set_modifiers(mods, kb)?;
            }

            // 4. Press the new key
            kb.press(key)?;
            self.current_key = Some(key);
            self.last_note_time = Instant::now();
        } else {
            // This is a note-off or other action sequence — execute normally
            self.execute_actions_raw(actions, kb)?;
        }

        Ok(())
    }

    /// Handle a note-off event.
    fn handle_note_off<K: KeyboardController>(
        &mut self,
        actions: &[Action],
        released_key: Option<Key>,
        kb: &mut K,
    ) -> Result<()> {
        // Only process the release if this note is still the current one.
        // If a newer note has already replaced it, skip the release to avoid
        // cutting off the new note.
        if let Some(rk) = released_key {
            if self.current_key == Some(rk) {
                self.execute_actions_raw(actions, kb)?;
                self.current_key = None;
            }
            // else: a different note is playing now, ignore this release
        } else {
            // No specific key identified, just run the actions
            self.execute_actions_raw(actions, kb)?;
        }

        Ok(())
    }

    /// Execute actions without the smart scheduling (raw passthrough).
    fn execute_actions_raw<K: KeyboardController>(
        &mut self,
        actions: &[Action],
        kb: &mut K,
    ) -> Result<()> {
        for action in actions {
            match action {
                Action::Press(key) => {
                    kb.press(*key)?;
                }
                Action::Release(key) => {
                    kb.release(*key)?;
                }
                Action::Delay(ms) => {
                    thread::sleep(Duration::from_millis(*ms));
                }
                Action::SetModifiers { shift, ctrl, alt } => {
                    let desired = ModifierState {
                        shift: *shift,
                        ctrl: *ctrl,
                        alt: *alt,
                    };
                    self.set_modifiers(desired, kb)?;
                }
            }
        }
        Ok(())
    }
}

/// Internal event sent through the channel from the MIDI callback to the processing thread.
struct MidiEvent {
    message: MidiMessage,
}

impl<K: KeyboardController + 'static> MidiEngine<K> {
    pub fn new(keyboard: K, mapping: MappingConfig) -> Self {
        Self {
            keyboard: Arc::new(Mutex::new(keyboard)),
            mapping: Arc::new(Mutex::new(mapping)),
        }
    }

    /// Get a shared reference to the mapping config.
    /// This can be used to modify settings (like octave_transpose) at runtime.
    pub fn mapping(&self) -> Arc<Mutex<MappingConfig>> {
        Arc::clone(&self.mapping)
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
        self.connect_with_callback(device_name, |_| {})
    }

    /// Connect to a MIDI device by name with a callback for MIDI events
    pub fn connect_with_callback<F>(
        &self,
        device_name: &str,
        callback: F,
    ) -> Result<MidiInputConnection<()>>
    where
        F: Fn(MidiMessage) + Send + 'static,
    {
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
            .ok_or_else(|| Error::Mapping(format!("Device '{}' not found", device_name)))?;

        self.connect_port_with_callback(port, callback)
    }

    /// Connect to a MIDI device by port
    pub fn connect_port(&self, port: MidiInputPort) -> Result<MidiInputConnection<()>> {
        self.connect_port_with_callback(port, |_| {})
    }

    /// Connect to a MIDI device by port with a callback for MIDI events
    pub fn connect_port_with_callback<F>(
        &self,
        port: MidiInputPort,
        callback: F,
    ) -> Result<MidiInputConnection<()>>
    where
        F: Fn(MidiMessage) + Send + 'static,
    {
        let midi_in = MidiInput::new("xiv-midi")?;

        let keyboard = Arc::clone(&self.keyboard);
        let mapping = Arc::clone(&self.mapping);

        // Create a bounded channel — small buffer to avoid latency buildup.
        // If the processing thread can't keep up, we'd rather drop old events
        // than accumulate latency.
        let (tx, rx) = channel::bounded::<MidiEvent>(64);

        // Spawn the processing thread with the NoteScheduler
        thread::spawn(move || {
            let mut scheduler = NoteScheduler::new();

            while let Ok(event) = rx.recv() {
                let msg = event.message;

                // Look up mapping
                let mapping_guard = mapping.lock().unwrap();
                if let Some(channel) = mapping_guard.channel {
                    if msg.channel != channel {
                        continue;
                    }
                }

                let note_mapping = match mapping_guard.get_mapping_transposed(msg.note) {
                    Some((_transposed_note, m)) => m.clone(),
                    None => {
                        tracing::debug!("No mapping for note {}", msg.note);
                        continue;
                    }
                };
                drop(mapping_guard);

                let mut kb = keyboard.lock().unwrap();

                let result = match msg.event_type {
                    MidiEventType::NoteOn => scheduler.play_note(&note_mapping.on_press, &mut *kb),
                    MidiEventType::NoteOff => {
                        // Figure out which key this note maps to for smart release
                        let released_key = note_mapping.on_press.iter().find_map(|a| {
                            if let Action::Press(k) = a {
                                Some(*k)
                            } else {
                                None
                            }
                        });
                        scheduler.handle_note_off(&note_mapping.on_release, released_key, &mut *kb)
                    }
                };

                if let Err(e) = result {
                    tracing::error!("Error handling MIDI event: {}", e);
                }
            }

            tracing::info!("MIDI processing thread exiting");
        });

        // Connect midir — the callback just forwards events through the channel
        let connection = midi_in.connect(
            &port,
            "xiv-midi-input",
            move |_timestamp, data, _| match MidiMessage::parse(data) {
                Ok(msg) => {
                    callback(msg.clone());

                    // Non-blocking send: if the channel is full, drop the event
                    // to avoid latency buildup
                    if let Err(e) = tx.try_send(MidiEvent { message: msg }) {
                        tracing::warn!("MIDI event dropped (channel full): {}", e);
                    }
                }
                Err(e) => {
                    tracing::error!("Error parsing MIDI message: {}", e);
                }
            },
            (),
        )?;

        tracing::info!("Connected to MIDI device");
        Ok(connection)
    }

    /// Release all keys
    pub fn release_all(&self) -> Result<()> {
        self.keyboard.lock().unwrap().release_all()
    }
}
