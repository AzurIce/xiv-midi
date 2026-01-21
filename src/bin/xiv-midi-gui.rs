use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui;
use egui_taffy::{TuiBuilderLogic, taffy, tui};
use midir::MidiInputConnection;
use std::path::PathBuf;
use taffy::prelude::length;
use xiv_midi::{
    engine::MidiEngine,
    keyboard::EnigoKeyboardController,
    mapping::{MappingConfig, create_ffxiv_default_mapping},
    midi::MidiEventType,
};

#[derive(Debug, Clone)]
enum AppEvent {
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
    mapping: MappingConfig,

    // Communication
    event_tx: Sender<AppEvent>,
    event_rx: Receiver<AppEvent>,

    // UI State
    log_messages: Vec<String>,
    active_notes: std::collections::HashMap<u8, u8>, // note -> velocity

    // Status
    status: String,
}

impl XivMidiApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (event_tx, event_rx) = unbounded();
        _cc.egui_ctx.all_styles_mut(|style| {
            style.wrap_mode = Some(egui::TextWrapMode::Extend);
        });

        let mut app = Self {
            devices: Vec::new(),
            selected_device: None,
            connection: None,
            mapping_path: None,
            mapping: create_ffxiv_default_mapping(),
            event_tx,
            event_rx,
            log_messages: Vec::new(),
            active_notes: std::collections::HashMap::new(),
            status: "Ready".to_string(),
        };

        app.refresh_devices();
        app
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
        self.mapping = if let Some(ref path) = self.mapping_path {
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
        let engine = MidiEngine::new(keyboard, self.mapping.clone());

        // Connect
        let event_tx = self.event_tx.clone();
        match engine.connect_with_callback(&device_name, move |msg| {
            let _ = event_tx.send(AppEvent::MidiEvent {
                note: msg.note.value(),
                velocity: msg.velocity,
                is_on: msg.event_type == MidiEventType::NoteOn,
            });
        }) {
            Ok(conn) => {
                self.connection = Some(conn);
                self.status = format!("Connected to '{}'", device_name);
                self.log(format!("Successfully connected to '{}'", device_name));
                let _ = self
                    .event_tx
                    .send(AppEvent::DeviceConnected(device_name.clone()));
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
                AppEvent::DeviceConnected(name) => {
                    tracing::debug!("Device connected event: {}", name);
                }
                AppEvent::DeviceDisconnected => {
                    tracing::debug!("Device disconnected event");
                }
                AppEvent::MidiEvent {
                    note,
                    velocity,
                    is_on,
                } => {
                    if is_on {
                        self.active_notes.insert(note, velocity);
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

            // Mapping Info
            ui.heading("Mapping & Live Actions");
            self.draw_mapping_info(ui);

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

        ctx.request_repaint();
    }
}

impl XivMidiApp {
    fn draw_mapping_info(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("mapping_info")
            .max_height(150.0)
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                tui(ui, "mapping_tui")
                    .reserve_available_width()
                    .style(taffy::Style {
                        flex_direction: taffy::FlexDirection::Row,
                        flex_wrap: taffy::FlexWrap::Wrap,
                        gap: length(8.0),
                        padding: length(8.0),
                        ..Default::default()
                    })
                    .show(|tui| {
                        let mut sorted_notes: Vec<_> = self.active_notes.keys().collect();
                        sorted_notes.sort();

                        if sorted_notes.is_empty() {
                            tui.label(
                                egui::RichText::new("No active notes")
                                    .italics()
                                    .color(egui::Color32::GRAY),
                            );
                        } else {
                            for note_val in sorted_notes {
                                if let Some(mapping) = self.mapping.mappings.get(note_val) {
                                    let note_name = xiv_midi::midi::MidiNote::new(*note_val)
                                        .map(|n| n.full_name())
                                        .unwrap_or_else(|_| note_val.to_string());

                                    tui.add_with_border(|tui| {
                                        tui.ui(|ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    egui::RichText::new(format!("{}:", note_name))
                                                        .strong(),
                                                );
                                                for action in &mapping.on_press {
                                                    ui.label(format!("{:?}", action));
                                                }
                                            });
                                        });
                                    });
                                }
                            }
                        }
                    });
            });
    }

    fn draw_piano(&self, ui: &mut egui::Ui) {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 100.0),
            egui::Sense::hover(),
        );

        let painter = ui.painter_at(rect);

        // Draw range: C2 (MIDI 36) to C7 (MIDI 96) - 5 octaves
        let start_note = 36;
        let end_note = 96;

        let mut white_notes = Vec::new();
        for note in start_note..=end_note {
            let note_in_octave = note % 12;
            if !matches!(note_in_octave, 1 | 3 | 6 | 8 | 10) {
                white_notes.push(note);
            }
        }

        let num_white_keys = white_notes.len();
        let white_key_width = rect.width() / num_white_keys as f32;
        let white_key_height = rect.height();
        let black_key_width = white_key_width * 0.7;
        let black_key_height = white_key_height * 0.6;

        // 1. Draw white keys
        for (i, &note) in white_notes.iter().enumerate() {
            let x = rect.min.x + i as f32 * white_key_width;
            let color = if let Some(&velocity) = self.active_notes.get(&note) {
                let intensity = (velocity as f32 / 127.0).clamp(0.4, 1.0);
                // Brighter, more saturated green for active keys
                egui::Color32::from_rgb(
                    (180.0 * (1.0 - intensity)) as u8,
                    255,
                    (180.0 * (1.0 - intensity)) as u8,
                )
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
                egui::Stroke::new(1.0, egui::Color32::from_gray(180)),
                egui::epaint::StrokeKind::Outside,
            );
        }

        // 2. Draw black keys
        for (i, &note) in white_notes.iter().enumerate() {
            let note_in_octave = note % 12;
            // If this white note has a black key to its right (except E and B)
            if !matches!(note_in_octave, 4 | 11) && i < num_white_keys - 1 {
                let black_note = note + 1;
                let x = rect.min.x + (i as f32 + 1.0) * white_key_width - black_key_width / 2.0;

                let color = if let Some(&velocity) = self.active_notes.get(&black_note) {
                    let _intensity = (velocity as f32 / 127.0).clamp(0.4, 1.0);
                    egui::Color32::from_rgb(0, 255, 0) // Pure bright green for black keys
                } else {
                    egui::Color32::from_gray(40)
                };

                let key_rect = egui::Rect::from_min_size(
                    egui::pos2(x, rect.min.y),
                    egui::vec2(black_key_width, black_key_height),
                );

                painter.rect_filled(key_rect, 1.0, color);
                painter.rect(
                    key_rect,
                    1.0,
                    egui::Color32::TRANSPARENT,
                    egui::Stroke::new(1.0, egui::Color32::BLACK),
                    egui::epaint::StrokeKind::Outside,
                );
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
