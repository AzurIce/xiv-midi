use clap::{Parser, Subcommand};
use std::path::PathBuf;
use xiv_midi::{
    engine::MidiEngine,
    keyboard::EnigoKeyboardController,
    mapping::{create_ffxiv_default_mapping, MappingConfig},
};

#[derive(Parser)]
#[command(name = "xiv-midi")]
#[command(about = "Convert MIDI input to keyboard presses for FFXIV", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List available MIDI devices
    List,

    /// Run the MIDI to keyboard converter
    Run {
        /// MIDI device name to connect to
        #[arg(short, long)]
        device: String,

        /// Path to custom mapping configuration file (JSON)
        #[arg(short, long)]
        mapping: Option<PathBuf>,
    },

    /// Generate default FFXIV mapping configuration file
    GenerateConfig {
        /// Output path for the configuration file
        #[arg(short, long, default_value = "mapping.json")]
        output: PathBuf,
    },
}

fn main() -> xiv_midi::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(false)
        .with_level(true)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::List => {
            list_devices()?;
        }
        Commands::Run { device, mapping } => {
            run(device, mapping)?;
        }
        Commands::GenerateConfig { output } => {
            generate_config(output)?;
        }
    }

    Ok(())
}

fn list_devices() -> xiv_midi::Result<()> {
    tracing::info!("Listing available MIDI devices...");

    let devices = MidiEngine::<EnigoKeyboardController>::list_devices()?;

    if devices.is_empty() {
        println!("No MIDI devices found.");
    } else {
        println!("Available MIDI devices:");
        for (i, device) in devices.iter().enumerate() {
            println!("  [{}] {}", i + 1, device);
        }
    }

    Ok(())
}

fn run(device_name: String, mapping_path: Option<PathBuf>) -> xiv_midi::Result<()> {
    tracing::info!("Starting xiv-midi...");

    // Load or create mapping
    let mapping = if let Some(path) = mapping_path {
        tracing::info!("Loading mapping from: {}", path.display());
        MappingConfig::from_file(&path)?
    } else {
        tracing::info!("Using default FFXIV mapping");
        create_ffxiv_default_mapping()
    };

    // Create keyboard controller
    let keyboard = EnigoKeyboardController::new()?;

    // Create engine
    let engine = MidiEngine::new(keyboard, mapping);

    // Connect to device
    tracing::info!("Connecting to device: {}", device_name);
    let _connection = engine.connect(&device_name)?;

    println!("✓ Connected to '{}'", device_name);
    println!("Press Ctrl+C to exit...");

    // Keep running until interrupted
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

fn generate_config(output: PathBuf) -> xiv_midi::Result<()> {
    tracing::info!("Generating default mapping configuration...");

    let mapping = create_ffxiv_default_mapping();
    mapping.to_file(&output)?;

    println!("✓ Configuration saved to: {}", output.display());
    println!("You can edit this file and load it with: xiv-midi run -d <device> -m {}", output.display());

    Ok(())
}
