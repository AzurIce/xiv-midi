# XIV MIDI

Convert MIDI input to keyboard presses for FFXIV performance system.

## Features

- 🎹 Convert MIDI notes to keyboard presses
- 🎮 Two interfaces: CLI and GUI
- ⚙️ Customizable key mappings via JSON
- 🎵 Default FFXIV mapping (3 octaves)
- 📝 Comprehensive logging with tracing

## Building

```bash
cargo build --release
```

## Usage

### CLI Version

#### List available MIDI devices

```bash
cargo run --bin xiv-midi list
```

#### Run with default FFXIV mapping

```bash
cargo run --bin xiv-midi run --device "Your MIDI Device"
```

#### Generate default mapping config

```bash
cargo run --bin xiv-midi generate-config --output my-mapping.json
```

#### Run with custom mapping

```bash
cargo run --bin xiv-midi run --device "Your MIDI Device" --mapping my-mapping.json
```

### GUI Version

Simply run:

```bash
cargo run --bin xiv-midi-gui
```

The GUI provides:
- Device selection and connection
- Piano visualization showing active notes
- Event logging
- Easy-to-use interface

## Default FFXIV Mapping

The default mapping covers 3 octaves (C3-C6) and maps to FFXIV's performance keyboard:

```
Q 2 W 3 E R 5 T 6 Y 7 U I
```

- **C3-B3 (Low octave)**: Ctrl + Key
- **C4-B4 (Middle octave)**: Key (no modifier)
- **C5-B5 (High octave)**: Shift + Key

## Custom Mappings

You can create custom mappings by editing the JSON configuration file. Each note can have:
- `on_press`: Actions to perform when note is pressed
- `on_release`: Actions to perform when note is released

Available actions:
- `Press`: Press a key
- `Release`: Release a key
- `Delay`: Wait for specified milliseconds
- `SetModifiers`: Set modifier keys (shift, ctrl, alt)

Example:

```json
{
  "channel": 0,
  "mappings": {
    "60": {
      "on_press": [
        {"Press": "Q"}
      ],
      "on_release": [
        {"Release": "Q"}
      ]
    }
  }
}
```

## Architecture

The project is organized into modular components:

- **`error.rs`**: Error types and Result type
- **`midi.rs`**: MIDI message parsing
- **`keyboard.rs`**: Keyboard input simulation
- **`mapping.rs`**: Key mapping configuration
- **`engine.rs`**: Core MIDI processing engine

## Dependencies

- `clap`: CLI argument parsing
- `midir`: MIDI input handling
- `enigo`: Keyboard input simulation
- `eframe`: GUI framework
- `crossbeam-channel`: Thread communication
- `tracing`: Logging
- `serde/serde_json`: Configuration serialization

## License

MIT

## Author

AzurIce
