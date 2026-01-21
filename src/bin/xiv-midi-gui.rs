use eframe::egui;
use std::path::PathBuf;
use xiv_midi::{
    engine::MidiEngine,
    keyboard::EnigoKeyboardController,
    mapping::{create_ffxiv_default_mapping, MappingConfig},
};
use crossbeam_channel::{unbounded, Receiver, Sender};
use midir::MidiInputConnection;

#[derive(Debug, Clone)]
enum AppEvent {
    Log(String),
    DeviceConnected(String),
    DeviceDisconnected,
    MidiEvent { note: u8, velocity: u8, is_on: bool },
}

struct XivMidiApp {
    // State
    devices: Vec<String>,
    selected_device: Option<String>,
    connection: Option<MidiInputConnection<()>>,
    mapping_path: Option<PathBuf>,

    // Communication
    event_tx: Sender<AppEvent>,
    event_rx: Receiver<AppEvent>,

    // UI State
    log_messages: Vec<String>,
    active_notes: std::collections::HashSet<u8>,

    // Status
    status: String,
}

impl XivMidiApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (event_tx, event_rx) = unbounded();

        Self {
            devices: Vec::new(),
            selected_device: None,
            connection: None,
            mapping_path: None,
            event_tx,
            event_rx,
            log_messages: Vec::new(),
            active_notes: std::collections::HashSet::new(),
            status: "Ready".to_string(),
        }
    }

    fn log(&mut self, message: String) {
        tracing::info!("{}", message);
        self.log_messages.push(message);
        if self.log_messages.len() > 100 {
            self.log_messages.remove(0);
        }
    }

    fn refresh_devices(&mut self) {
        match MidiEngine::<EnigoKeyboardController>::list_devices() {
            Ok(devices) => {
                self.devices = devices;
                self.log(format!("Found {} MIDI device(s)", self.devices.len()));
            }
            Err(e) => {
                self.log(format!("Error listing devices: {}", e));
            }
        }
    }

    fn connect_device(&mut self, device_name: String) {
        self.log(format!("Connecting to '{}'...", device_name));

        // Load mapping
        let mapping = if let Some(ref path) = self.mapping_path {
            match MappingConfig::from_file(path) {
                Ok(m) => {
                    self.log(format!("Loaded mapping from {}", path.display()));
                    m
                }
                Err(e) => {
                    self.log(format!("Error loading mapping: {}, using default", e));
                    create_ffxiv_default_mapping()
                }
            }
        } else {
            self.log("Using default FFXIV mapping".to_string());
            create_ffxiv_default_mapping()
        };

        // Create keyboard controller
        let keyboard = match EnigoKeyboardController::new() {
            Ok(k) => k,
            Err(e) => {
                self.log(format!("Error creating keyboard controller: {}", e));
                return;
            }
        };

        // Create engine
        let engine = MidiEngine::new(keyboard, mapping);

        // Connect
        match engine.connect(&device_name) {
            Ok(conn) => {
                self.connection = Some(conn);
                self.status = format!("Connected to '{}'", device_name);
                self.log(format!("Successfully connected to '{}'", device_name));
                let _ = self.event_tx.send(AppEvent::DeviceConnected(device_name.clone()));
            }
            Err(e) => {
                self.log(format!("Error connecting: {}", e));
                self.status = "Connection failed".to_string();
            }
        }
    }

    fn disconnect_device(&mut self) {
        if self.connection.is_some() {
            self.connection = None;
            self.status = "Disconnected".to_string();
            self.log("Disconnected from device".to_string());
            let _ = self.event_tx.send(AppEvent::DeviceDisconnected);
        }
    }

    fn process_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                AppEvent::Log(msg) => {
                    self.log(msg);
                }
                AppEvent::DeviceConnected(_) => {}
                AppEvent::DeviceDisconnected => {}
                AppEvent::MidiEvent { note, velocity: _, is_on } => {
                    if is_on {
                        self.active_notes.insert(note);
                    } else {
                        self.active_notes.remove(&note);
                    }
                }
            }
        }
    }
}

impl eframe::App for XivMidiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_events();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("XIV MIDI - FFXIV Performance Tool");
            ui.separator();

            // Connection panel
            ui.horizontal(|ui| {
                if ui.button("🔄 Refresh Devices").clicked() {
                    self.refresh_devices();
                }

                egui::ComboBox::from_label("MIDI Device")
                    .selected_text(
                        self.selected_device
                            .as_ref()
                            .map(|s| s.as_str())
                            .unwrap_or("Select a device..."),
                    )
                    .show_ui(ui, |ui| {
                        for device in &self.devices {
                            ui.selectable_value(
                                &mut self.selected_device,
                                Some(device.clone()),
                                device,
                            );
                        }
                    });

                let is_connected = self.connection.is_some();

                if !is_connected {
                    if ui
                        .add_enabled(
                            self.selected_device.is_some(),
                            egui::Button::new("🔌 Connect"),
                        )
                        .clicked()
                    {
                        if let Some(device) = self.selected_device.clone() {
                            self.connect_device(device);
                        }
                    }
                } else {
                    if ui.button("⏸ Disconnect").clicked() {
                        self.disconnect_device();
                    }
                }
            });

            ui.separator();

            // Status bar
            ui.horizontal(|ui| {
                ui.label("Status:");
                let status_color = if self.connection.is_some() {
                    egui::Color32::GREEN
                } else {
                    egui::Color32::GRAY
                };
                ui.colored_label(status_color, &self.status);
            });

            ui.separator();

            // Piano visualization
            ui.heading("Active Notes");
            self.draw_piano(ui);

            ui.separator();

            // Log panel
            ui.heading("Log");
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .max_height(200.0)
                .show(ui, |ui| {
                    for msg in &self.log_messages {
                        ui.label(msg);
                    }
                });
        });

        // Request repaint if we have active notes
        if !self.active_notes.is_empty() {
            ctx.request_repaint();
        }
    }
}

impl XivMidiApp {
    fn draw_piano(&self, ui: &mut egui::Ui) {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 80.0),
            egui::Sense::hover(),
        );

        let painter = ui.painter_at(rect);

        // Draw range: C2 (MIDI 36) to C6 (MIDI 84)
        let start_note = 36;
        let end_note = 84;
        let white_key_width = rect.width() / 28.0; // Approximate white keys in range
        let white_key_height = rect.height();
        let black_key_width = white_key_width * 0.6;
        let black_key_height = white_key_height * 0.6;

        let mut x = rect.min.x;

        // Draw white keys first
        for note in start_note..=end_note {
            let note_in_octave = note % 12;
            let is_black = matches!(note_in_octave, 1 | 3 | 6 | 8 | 10);

            if !is_black {
                let is_active = self.active_notes.contains(&note);
                let color = if is_active {
                    egui::Color32::from_rgb(100, 200, 100)
                } else {
                    egui::Color32::WHITE
                };

                let key_rect = egui::Rect::from_min_size(
                    egui::pos2(x, rect.min.y),
                    egui::vec2(white_key_width, white_key_height),
                );

                painter.rect_filled(key_rect, 2.0, color);
                painter.rect(
                    key_rect,
                    2.0,
                    egui::Color32::TRANSPARENT,
                    egui::Stroke::new(1.0, egui::Color32::BLACK),
                    egui::epaint::StrokeKind::Outside,
                );

                x += white_key_width;
            }
        }

        // Draw black keys on top
        x = rect.min.x;
        for note in start_note..=end_note {
            let note_in_octave = note % 12;
            let is_black = matches!(note_in_octave, 1 | 3 | 6 | 8 | 10);

            if is_black {
                let is_active = self.active_notes.contains(&note);
                let color = if is_active {
                    egui::Color32::from_rgb(0, 150, 0)
                } else {
                    egui::Color32::BLACK
                };

                // Position black key between white keys
                let prev_white_x = x - white_key_width * 0.5;
                let key_rect = egui::Rect::from_min_size(
                    egui::pos2(prev_white_x + white_key_width * 0.7, rect.min.y),
                    egui::vec2(black_key_width, black_key_height),
                );

                painter.rect_filled(key_rect, 2.0, color);
                painter.rect(
                    key_rect,
                    2.0,
                    egui::Color32::TRANSPARENT,
                    egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
                    egui::epaint::StrokeKind::Outside,
                );
            } else {
                x += white_key_width;
            }
        }
    }
}

fn main() -> eframe::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(false)
        .with_level(true)
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([600.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "XIV MIDI",
        options,
        Box::new(|cc| Ok(Box::new(XivMidiApp::new(cc)))),
    )
}
