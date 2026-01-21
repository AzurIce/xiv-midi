use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("MIDI error: {0}")]
    Midi(#[from] midir::InitError),

    #[error("MIDI connection error: {0}")]
    MidiConnection(#[from] midir::ConnectError<midir::MidiInput>),

    #[error("Invalid MIDI message: {0}")]
    InvalidMidiMessage(String),

    #[error("Keyboard error: {0}")]
    Keyboard(String),

    #[error("Mapping error: {0}")]
    Mapping(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
