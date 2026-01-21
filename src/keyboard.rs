use crate::error::{Error, Result};
use enigo::{
    Direction, Enigo, Key as EnigoKey, Keyboard as EnigoKeyboard, Settings,
};
use std::collections::HashMap;

/// Keyboard key representation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Key {
    // Letter keys
    A, B, C, D, E, F, G, H, I, J, K, L, M,
    N, O, P, Q, R, S, T, U, V, W, X, Y, Z,

    // Number keys
    Num0, Num1, Num2, Num3, Num4,
    Num5, Num6, Num7, Num8, Num9,

    // Function keys
    F1, F2, F3, F4, F5, F6,
    F7, F8, F9, F10, F11, F12,

    // Modifier keys
    Shift,
    Control,
    Alt,
    Meta,

    // Special keys
    Space,
    Enter,
    Escape,
    Tab,
    Backspace,

    // Arrow keys
    Up,
    Down,
    Left,
    Right,
}

impl Key {
    fn to_enigo_key(self) -> EnigoKey {
        match self {
            Key::A => EnigoKey::Unicode('a'),
            Key::B => EnigoKey::Unicode('b'),
            Key::C => EnigoKey::Unicode('c'),
            Key::D => EnigoKey::Unicode('d'),
            Key::E => EnigoKey::Unicode('e'),
            Key::F => EnigoKey::Unicode('f'),
            Key::G => EnigoKey::Unicode('g'),
            Key::H => EnigoKey::Unicode('h'),
            Key::I => EnigoKey::Unicode('i'),
            Key::J => EnigoKey::Unicode('j'),
            Key::K => EnigoKey::Unicode('k'),
            Key::L => EnigoKey::Unicode('l'),
            Key::M => EnigoKey::Unicode('m'),
            Key::N => EnigoKey::Unicode('n'),
            Key::O => EnigoKey::Unicode('o'),
            Key::P => EnigoKey::Unicode('p'),
            Key::Q => EnigoKey::Unicode('q'),
            Key::R => EnigoKey::Unicode('r'),
            Key::S => EnigoKey::Unicode('s'),
            Key::T => EnigoKey::Unicode('t'),
            Key::U => EnigoKey::Unicode('u'),
            Key::V => EnigoKey::Unicode('v'),
            Key::W => EnigoKey::Unicode('w'),
            Key::X => EnigoKey::Unicode('x'),
            Key::Y => EnigoKey::Unicode('y'),
            Key::Z => EnigoKey::Unicode('z'),
            Key::Num0 => EnigoKey::Unicode('0'),
            Key::Num1 => EnigoKey::Unicode('1'),
            Key::Num2 => EnigoKey::Unicode('2'),
            Key::Num3 => EnigoKey::Unicode('3'),
            Key::Num4 => EnigoKey::Unicode('4'),
            Key::Num5 => EnigoKey::Unicode('5'),
            Key::Num6 => EnigoKey::Unicode('6'),
            Key::Num7 => EnigoKey::Unicode('7'),
            Key::Num8 => EnigoKey::Unicode('8'),
            Key::Num9 => EnigoKey::Unicode('9'),
            Key::F1 => EnigoKey::F1,
            Key::F2 => EnigoKey::F2,
            Key::F3 => EnigoKey::F3,
            Key::F4 => EnigoKey::F4,
            Key::F5 => EnigoKey::F5,
            Key::F6 => EnigoKey::F6,
            Key::F7 => EnigoKey::F7,
            Key::F8 => EnigoKey::F8,
            Key::F9 => EnigoKey::F9,
            Key::F10 => EnigoKey::F10,
            Key::F11 => EnigoKey::F11,
            Key::F12 => EnigoKey::F12,
            Key::Shift => EnigoKey::Shift,
            Key::Control => EnigoKey::Control,
            Key::Alt => EnigoKey::Alt,
            Key::Meta => EnigoKey::Meta,
            Key::Space => EnigoKey::Space,
            Key::Enter => EnigoKey::Return,
            Key::Escape => EnigoKey::Escape,
            Key::Tab => EnigoKey::Tab,
            Key::Backspace => EnigoKey::Backspace,
            Key::Up => EnigoKey::UpArrow,
            Key::Down => EnigoKey::DownArrow,
            Key::Left => EnigoKey::LeftArrow,
            Key::Right => EnigoKey::RightArrow,
        }
    }
}

/// Keyboard controller trait
pub trait KeyboardController: Send {
    fn press(&mut self, key: Key) -> Result<()>;
    fn release(&mut self, key: Key) -> Result<()>;
    fn release_all(&mut self) -> Result<()>;
}

/// Enigo-based keyboard controller
pub struct EnigoKeyboardController {
    enigo: Enigo,
    pressed_keys: HashMap<Key, bool>,
}

impl EnigoKeyboardController {
    pub fn new() -> Result<Self> {
        let enigo = Enigo::new(&Settings::default())
            .map_err(|e| Error::Keyboard(format!("Failed to initialize Enigo: {:?}", e)))?;

        Ok(Self {
            enigo,
            pressed_keys: HashMap::new(),
        })
    }
}

impl KeyboardController for EnigoKeyboardController {
    fn press(&mut self, key: Key) -> Result<()> {
        // Check if key is already pressed
        if self.pressed_keys.get(&key).copied().unwrap_or(false) {
            return Ok(());
        }

        tracing::debug!("Pressing key: {:?}", key);

        self.enigo
            .key(key.to_enigo_key(), Direction::Press)
            .map_err(|e| Error::Keyboard(format!("Failed to press key {:?}: {:?}", key, e)))?;

        self.pressed_keys.insert(key, true);
        Ok(())
    }

    fn release(&mut self, key: Key) -> Result<()> {
        // Check if key is actually pressed
        if !self.pressed_keys.get(&key).copied().unwrap_or(false) {
            return Ok(());
        }

        tracing::debug!("Releasing key: {:?}", key);

        self.enigo
            .key(key.to_enigo_key(), Direction::Release)
            .map_err(|e| Error::Keyboard(format!("Failed to release key {:?}: {:?}", key, e)))?;

        self.pressed_keys.insert(key, false);
        Ok(())
    }

    fn release_all(&mut self) -> Result<()> {
        tracing::debug!("Releasing all keys");

        for (key, pressed) in &self.pressed_keys {
            if *pressed {
                self.enigo
                    .key(key.to_enigo_key(), Direction::Release)
                    .map_err(|e| {
                        Error::Keyboard(format!("Failed to release key {:?}: {:?}", key, e))
                    })?;
            }
        }

        self.pressed_keys.clear();
        Ok(())
    }
}
