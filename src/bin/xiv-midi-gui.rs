use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui;
use egui_taffy::{TuiBuilderLogic, taffy, tui};
use midir::MidiInputConnection;
use std::collections::HashMap;
use std::path::PathBuf;
use taffy::prelude::length;
use xiv_midi::{
    engine::MidiEngine,
    keyboard::{EnigoKeyboardController, Key},
    mapping::{Action, MappingConfig, NoteMapping, create_ffxiv_default_mapping},
    midi::MidiEventType,
};

#[derive(Debug, Clone)]
enum AppEvent {
    DeviceConnected(String),
    DeviceDisconnected,
    MidiEvent { note: u8, velocity: u8, is_on: bool },
}

#[derive(Debug, Clone)]
struct MappingOption {
    name: String,
    path: Option<PathBuf>,
    is_readonly: bool,
}

struct MappingEditor {
    available_mappings: Vec<MappingOption>,
    selected_mapping_index: usize,
    current_mapping: MappingConfig,
    selected_note: Option<u8>,
    rename_buffer: String,
    is_renaming: bool,
    is_modified: bool,
    new_mapping_name: String,
    show_new_mapping_dialog: bool,
    // Action editor state
    show_action_dialog: bool,
    editing_action_index: Option<(ActionListType, usize)>, // (list type, index)
    action_editor: ActionEditor,
    // Unsaved changes dialog
    show_unsaved_dialog: bool,
    pending_action: Option<PendingAction>,
    switch_to_main_requested: bool,
}

#[derive(Debug, Clone)]
enum PendingAction {
    LoadMapping(usize),
    SwitchToMainTab,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ActionListType {
    OnPress,
    OnRelease,
}

struct ActionEditor {
    action_type: ActionType,
    // For Press/Release
    selected_key: Key,
    capturing_key: bool, // True when waiting for user to press a key
    // For Delay
    delay_ms: String,
    // For SetModifiers
    shift: bool,
    ctrl: bool,
    alt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ActionType {
    Press,
    Release,
    Delay,
    SetModifiers,
}

struct XivMidiApp {
    // State
    devices: Vec<String>,
    selected_device: Option<String>,
    connection: Option<MidiInputConnection<()>>,

    // Mapping
    available_mappings: Vec<MappingOption>,
    selected_mapping_index: usize,
    mapping: MappingConfig,

    // Editor
    editor: MappingEditor,

    // Communication
    event_tx: Sender<AppEvent>,
    event_rx: Receiver<AppEvent>,

    // UI State
    log_messages: Vec<String>,
    active_notes: HashMap<u8, u8>,
    current_tab: AppTab,

    // Status
    status: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AppTab {
    Main,
    Editor,
}

impl ActionEditor {
    fn new() -> Self {
        Self {
            action_type: ActionType::Press,
            selected_key: Key::A,
            capturing_key: false,
            delay_ms: "100".to_string(),
            shift: false,
            ctrl: false,
            alt: false,
        }
    }

    fn reset(&mut self) {
        self.action_type = ActionType::Press;
        self.selected_key = Key::A;
        self.capturing_key = false;
        self.delay_ms = "100".to_string();
        self.shift = false;
        self.ctrl = false;
        self.alt = false;
    }

    fn load_action(&mut self, action: &Action) {
        match action {
            Action::Press(key) => {
                self.action_type = ActionType::Press;
                self.selected_key = *key;
            }
            Action::Release(key) => {
                self.action_type = ActionType::Release;
                self.selected_key = *key;
            }
            Action::Delay(ms) => {
                self.action_type = ActionType::Delay;
                self.delay_ms = ms.to_string();
            }
            Action::SetModifiers { shift, ctrl, alt } => {
                self.action_type = ActionType::SetModifiers;
                self.shift = *shift;
                self.ctrl = *ctrl;
                self.alt = *alt;
            }
        }
    }

    fn build_action(&self) -> Option<Action> {
        match self.action_type {
            ActionType::Press => Some(Action::Press(self.selected_key)),
            ActionType::Release => Some(Action::Release(self.selected_key)),
            ActionType::Delay => {
                if let Ok(ms) = self.delay_ms.parse::<u64>() {
                    Some(Action::Delay(ms))
                } else {
                    None
                }
            }
            ActionType::SetModifiers => Some(Action::SetModifiers {
                shift: self.shift,
                ctrl: self.ctrl,
                alt: self.alt,
            }),
        }
    }

    fn is_valid(&self) -> bool {
        match self.action_type {
            ActionType::Delay => self.delay_ms.parse::<u64>().is_ok(),
            _ => true,
        }
    }
}

impl MappingEditor {
    fn new() -> Self {
        Self {
            available_mappings: Vec::new(),
            selected_mapping_index: 0,
            current_mapping: create_ffxiv_default_mapping(),
            selected_note: None,
            rename_buffer: String::new(),
            is_renaming: false,
            is_modified: false,
            new_mapping_name: String::new(),
            show_new_mapping_dialog: false,
            show_action_dialog: false,
            editing_action_index: None,
            action_editor: ActionEditor::new(),
            show_unsaved_dialog: false,
            pending_action: None,
            switch_to_main_requested: false,
        }
    }

    fn scan_mappings(&mut self, log: &mut Vec<String>) {
        self.available_mappings.clear();

        // Add default mapping
        self.available_mappings.push(MappingOption {
            name: "Default FFXIV".to_string(),
            path: None,
            is_readonly: true,
        });

        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let mappings_dir = exe_dir.join("mappings");

                if mappings_dir.exists() && mappings_dir.is_dir() {
                    match std::fs::read_dir(&mappings_dir) {
                        Ok(entries) => {
                            let mut files: Vec<_> = entries
                                .filter_map(|entry| entry.ok())
                                .filter(|entry| {
                                    entry
                                        .path()
                                        .extension()
                                        .map(|ext| ext == "json")
                                        .unwrap_or(false)
                                })
                                .collect();

                            files.sort_by_key(|entry| entry.file_name());

                            for entry in files {
                                let path = entry.path();
                                let name = path
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("Unknown")
                                    .to_string();

                                self.available_mappings.push(MappingOption {
                                    name,
                                    path: Some(path),
                                    is_readonly: false,
                                });
                            }

                            log.push(format!(
                                "Found {} mapping file(s)",
                                self.available_mappings.len() - 1
                            ));
                        }
                        Err(e) => {
                            log.push(format!("Error reading mappings directory: {}", e));
                        }
                    }
                }
            }
        }
    }

    fn load_mapping(&mut self, index: usize, log: &mut Vec<String>) {
        if index >= self.available_mappings.len() {
            log.push("Invalid mapping index".to_string());
            return;
        }

        let mapping_option = &self.available_mappings[index];

        self.current_mapping = if let Some(ref path) = mapping_option.path {
            match MappingConfig::from_file(path) {
                Ok(m) => {
                    log.push(format!("Loaded '{}'", mapping_option.name));
                    m
                }
                Err(e) => {
                    log.push(format!("Error loading '{}': {}", mapping_option.name, e));
                    create_ffxiv_default_mapping()
                }
            }
        } else {
            log.push("Loaded default mapping".to_string());
            create_ffxiv_default_mapping()
        };

        self.selected_mapping_index = index;
        self.is_modified = false;
        self.selected_note = None;
    }

    fn duplicate_mapping(&mut self, index: usize, log: &mut Vec<String>) {
        if index >= self.available_mappings.len() {
            log.push("Invalid mapping index".to_string());
            return;
        }

        let source = &self.available_mappings[index];
        let mut new_name = format!("{}_copy", source.name);

        let mut counter = 1;
        while self.available_mappings.iter().any(|m| m.name == new_name) {
            new_name = format!("{}_copy_{}", source.name, counter);
            counter += 1;
        }

        let mapping = if let Some(ref path) = source.path {
            match MappingConfig::from_file(path) {
                Ok(m) => m,
                Err(e) => {
                    log.push(format!("Error loading source: {}", e));
                    return;
                }
            }
        } else {
            create_ffxiv_default_mapping()
        };

        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let mappings_dir = exe_dir.join("mappings");
                if let Err(e) = std::fs::create_dir_all(&mappings_dir) {
                    log.push(format!("Error creating directory: {}", e));
                    return;
                }

                let new_path = mappings_dir.join(format!("{}.json", new_name));
                match mapping.to_file(&new_path) {
                    Ok(_) => {
                        log.push(format!("Duplicated to '{}'", new_name));
                        self.scan_mappings(log);
                    }
                    Err(e) => log.push(format!("Error saving: {}", e)),
                }
            }
        }
    }

    fn delete_mapping(&mut self, index: usize, log: &mut Vec<String>) {
        if index >= self.available_mappings.len() {
            log.push("Invalid mapping index".to_string());
            return;
        }

        let mapping = &self.available_mappings[index];

        if mapping.is_readonly {
            log.push("Cannot delete default mapping".to_string());
            return;
        }

        if let Some(ref path) = mapping.path {
            match std::fs::remove_file(path) {
                Ok(_) => {
                    log.push(format!("Deleted '{}'", mapping.name));
                    self.scan_mappings(log);
                    if self.selected_mapping_index >= self.available_mappings.len() {
                        self.selected_mapping_index = 0;
                        self.load_mapping(0, log);
                    }
                }
                Err(e) => log.push(format!("Error deleting: {}", e)),
            }
        }
    }

    fn rename_mapping(&mut self, index: usize, new_name: String, log: &mut Vec<String>) {
        if index >= self.available_mappings.len() {
            log.push("Invalid mapping index".to_string());
            return;
        }

        let mapping = &self.available_mappings[index];

        if mapping.is_readonly {
            log.push("Cannot rename default mapping".to_string());
            return;
        }

        if new_name.is_empty() {
            log.push("Name cannot be empty".to_string());
            return;
        }

        if self.available_mappings.iter().any(|m| m.name == new_name) {
            log.push("Name already exists".to_string());
            return;
        }

        if let Some(ref old_path) = mapping.path {
            if let Some(parent) = old_path.parent() {
                let new_path = parent.join(format!("{}.json", new_name));
                match std::fs::rename(old_path, &new_path) {
                    Ok(_) => {
                        log.push(format!("Renamed to '{}'", new_name));
                        self.scan_mappings(log);
                        self.is_renaming = false;
                    }
                    Err(e) => log.push(format!("Error renaming: {}", e)),
                }
            }
        }
    }

    fn save_current(&mut self, log: &mut Vec<String>) {
        let mapping = &self.available_mappings[self.selected_mapping_index];

        if mapping.is_readonly {
            log.push("Cannot save default mapping (use duplicate)".to_string());
            return;
        }

        if let Some(ref path) = mapping.path {
            match self.current_mapping.to_file(path) {
                Ok(_) => {
                    self.is_modified = false;
                    log.push(format!("Saved '{}'", mapping.name));
                }
                Err(e) => log.push(format!("Error saving: {}", e)),
            }
        }
    }

    fn create_new(&mut self, name: String, log: &mut Vec<String>) {
        if name.is_empty() {
            log.push("Name cannot be empty".to_string());
            return;
        }

        if self.available_mappings.iter().any(|m| m.name == name) {
            log.push("Name already exists".to_string());
            return;
        }

        let mapping = MappingConfig {
            channel: Some(0),
            mappings: HashMap::new(),
        };

        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let mappings_dir = exe_dir.join("mappings");
                if let Err(e) = std::fs::create_dir_all(&mappings_dir) {
                    log.push(format!("Error creating directory: {}", e));
                    return;
                }

                let path = mappings_dir.join(format!("{}.json", name));
                match mapping.to_file(&path) {
                    Ok(_) => {
                        log.push(format!("Created '{}'", name));
                        self.scan_mappings(log);
                        self.show_new_mapping_dialog = false;
                        self.new_mapping_name.clear();
                    }
                    Err(e) => log.push(format!("Error creating: {}", e)),
                }
            }
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, log: &mut Vec<String>) {
        let mut action_queue: Vec<(&str, usize)> = Vec::new();

        egui::SidePanel::left("mapping_list")
            .default_width(250.0)
            .resizable(true)
            .show_inside(ui, |ui| {
                ui.heading("Mappings");
                ui.separator();

                if ui.button("+ New Mapping").clicked() {
                    self.show_new_mapping_dialog = true;
                }

                ui.separator();

                egui::ScrollArea::vertical()
                    .id_salt("mapping_list_scroll")
                    .show(ui, |ui| {
                        for (index, mapping) in self.available_mappings.iter().enumerate() {
                            ui.horizontal(|ui| {
                                let is_selected = index == self.selected_mapping_index;

                                if ui.selectable_label(is_selected, &mapping.name).clicked() {
                                    action_queue.push(("load", index));
                                }

                                if !mapping.is_readonly {
                                    if ui.small_button("📋").on_hover_text("Duplicate").clicked()
                                    {
                                        action_queue.push(("duplicate", index));
                                    }
                                    if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                                        action_queue.push(("delete", index));
                                    }
                                    if ui.small_button("✏").on_hover_text("Rename").clicked() {
                                        self.is_renaming = true;
                                        self.rename_buffer = mapping.name.clone();
                                    }
                                } else {
                                    if ui.small_button("📋").on_hover_text("Duplicate").clicked()
                                    {
                                        action_queue.push(("duplicate", index));
                                    }
                                }
                            });
                        }
                    });
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            let current_name = &self.available_mappings[self.selected_mapping_index].name;
            let is_readonly = self.available_mappings[self.selected_mapping_index].is_readonly;

            ui.heading(format!("Editing: {}", current_name));

            if is_readonly {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 165, 0),
                    "⚠ This is read-only. Duplicate to edit.",
                );
            }

            ui.separator();

            ui.label("Select a MIDI note from the keyboard:");
            self.draw_midi_keyboard(ui);

            ui.separator();

            if let Some(note) = self.selected_note {
                self.draw_note_editor(ui, note, is_readonly, log);
            } else {
                ui.label("Select a note from the keyboard above");
            }

            ui.separator();

            ui.horizontal(|ui| {
                if !is_readonly {
                    if ui
                        .add_enabled(self.is_modified, egui::Button::new("💾 Save"))
                        .clicked()
                    {
                        self.save_current(log);
                    }

                    if self.is_modified {
                        ui.colored_label(egui::Color32::from_rgb(255, 165, 0), "* Modified");
                    }
                }
            });
        });

        // Process actions after drawing
        for (action, index) in action_queue {
            match action {
                "load" => {
                    // Check for unsaved changes before loading
                    if self.is_modified {
                        self.pending_action = Some(PendingAction::LoadMapping(index));
                        self.show_unsaved_dialog = true;
                    } else {
                        self.load_mapping(index, log);
                    }
                }
                "duplicate" => self.duplicate_mapping(index, log),
                "delete" => self.delete_mapping(index, log),
                _ => {}
            }
        }

        // New mapping dialog
        if self.show_new_mapping_dialog {
            egui::Window::new("New Mapping")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Mapping name:");
                    ui.text_edit_singleline(&mut self.new_mapping_name);

                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() {
                            self.create_new(self.new_mapping_name.clone(), log);
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_new_mapping_dialog = false;
                            self.new_mapping_name.clear();
                        }
                    });
                });
        }

        // Rename dialog
        if self.is_renaming {
            egui::Window::new("Rename Mapping")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("New name:");
                    ui.text_edit_singleline(&mut self.rename_buffer);

                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            self.rename_mapping(
                                self.selected_mapping_index,
                                self.rename_buffer.clone(),
                                log,
                            );
                        }
                        if ui.button("Cancel").clicked() {
                            self.is_renaming = false;
                        }
                    });
                });
        }

        // Unsaved changes dialog
        if self.show_unsaved_dialog {
            self.draw_unsaved_dialog(ctx, log);
        }

        // Action editor dialog
        if self.show_action_dialog {
            self.draw_action_dialog(ctx, log);
        }
    }

    fn draw_unsaved_dialog(&mut self, ctx: &egui::Context, log: &mut Vec<String>) {
        let mut should_save = false;
        let mut should_discard = false;
        let mut should_cancel = false;

        egui::Window::new("Unsaved Changes")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("You have unsaved changes.");
                ui.label("Do you want to save them?");

                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    if ui.button("💾 Save").clicked() {
                        should_save = true;
                    }
                    if ui.button("🗑 Discard").clicked() {
                        should_discard = true;
                    }
                    if ui.button("Cancel").clicked() {
                        should_cancel = true;
                    }
                });
            });

        if should_save {
            self.save_current(log);
            if !self.is_modified {
                // Save successful
                if matches!(self.pending_action, Some(PendingAction::SwitchToMainTab)) {
                    self.switch_to_main_requested = true;
                } else {
                    self.execute_pending_action(log);
                }
                self.show_unsaved_dialog = false;
                self.pending_action = None;
            }
        } else if should_discard {
            self.is_modified = false;
            if matches!(self.pending_action, Some(PendingAction::SwitchToMainTab)) {
                self.switch_to_main_requested = true;
            } else {
                self.execute_pending_action(log);
            }
            self.show_unsaved_dialog = false;
            self.pending_action = None;
        } else if should_cancel {
            self.show_unsaved_dialog = false;
            self.pending_action = None;
        }
    }

    fn execute_pending_action(&mut self, log: &mut Vec<String>) {
        if let Some(action) = self.pending_action.take() {
            match action {
                PendingAction::LoadMapping(index) => {
                    self.load_mapping(index, log);
                }
                PendingAction::SwitchToMainTab => {
                    // Will be handled by the caller
                }
            }
        }
    }

    fn draw_action_dialog(&mut self, ctx: &egui::Context, log: &mut Vec<String>) {
        let mut should_close = false;
        let mut should_save = false;

        egui::Window::new("Edit Action")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Action Type:");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.action_editor.action_type, ActionType::Press, "Press");
                    ui.selectable_value(&mut self.action_editor.action_type, ActionType::Release, "Release");
                    ui.selectable_value(&mut self.action_editor.action_type, ActionType::SetModifiers, "SetModifiers");
                    ui.selectable_value(&mut self.action_editor.action_type, ActionType::Delay, "Delay");
                });

                ui.separator();

                match self.action_editor.action_type {
                    ActionType::Press | ActionType::Release => {
                        ui.label("Press a key:");

                        // Key capture area
                        let key_text = if self.action_editor.capturing_key {
                            "... Press any key ...".to_string()
                        } else {
                            format!("{:?}", self.action_editor.selected_key)
                        };

                        let button = egui::Button::new(&key_text)
                            .min_size(egui::vec2(200.0, 40.0));

                        if ui.add(button).clicked() {
                            self.action_editor.capturing_key = true;
                        }

                        // Capture key input
                        if self.action_editor.capturing_key {
                            ui.colored_label(egui::Color32::from_rgb(255, 165, 0), "Waiting for key press...");

                            // Check for key events
                            if let Some(key) = self.capture_key_input(ui) {
                                self.action_editor.selected_key = key;
                                self.action_editor.capturing_key = false;
                            }

                            // ESC to cancel capture
                            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                self.action_editor.capturing_key = false;
                            }
                        }

                        ui.label(egui::RichText::new("Tip: Click the button above and press any key")
                            .small()
                            .italics()
                            .color(egui::Color32::GRAY));
                    }
                    ActionType::Delay => {
                        ui.label("Delay (milliseconds):");
                        ui.text_edit_singleline(&mut self.action_editor.delay_ms);

                        if self.action_editor.delay_ms.parse::<u64>().is_err() {
                            ui.colored_label(egui::Color32::RED, "⚠ Please enter a valid number");
                        }
                    }
                    ActionType::SetModifiers => {
                        ui.label("Modifiers:");
                        ui.checkbox(&mut self.action_editor.shift, "Shift");
                        ui.checkbox(&mut self.action_editor.ctrl, "Ctrl");
                        ui.checkbox(&mut self.action_editor.alt, "Alt");
                    }
                }

                ui.separator();

                ui.horizontal(|ui| {
                    if ui.add_enabled(self.action_editor.is_valid() && !self.action_editor.capturing_key, egui::Button::new("OK")).clicked() {
                        should_save = true;
                        should_close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        should_close = true;
                    }
                });
            });

        if should_save {
            if let Some(action) = self.action_editor.build_action() {
                if let (Some(note), Some((list_type, index))) = (self.selected_note, self.editing_action_index) {
                    let mapping = self.current_mapping.mappings.get_mut(&note).unwrap();
                    let actions = match list_type {
                        ActionListType::OnPress => &mut mapping.on_press,
                        ActionListType::OnRelease => &mut mapping.on_release,
                    };

                    if index < actions.len() {
                        // Edit existing action
                        actions[index] = action;
                        log.push(format!("Updated action at index {}", index));
                    } else {
                        // Add new action
                        actions.push(action);
                        log.push("Added new action".to_string());
                    }
                    self.is_modified = true;
                }
            }
        }

        if should_close {
            self.show_action_dialog = false;
            self.editing_action_index = None;
        }
    }

    fn capture_key_input(&self, ui: &egui::Ui) -> Option<Key> {
        ui.input(|i| {
            // Check letter keys
            for (egui_key, our_key) in [
                (egui::Key::A, Key::A), (egui::Key::B, Key::B), (egui::Key::C, Key::C),
                (egui::Key::D, Key::D), (egui::Key::E, Key::E), (egui::Key::F, Key::F),
                (egui::Key::G, Key::G), (egui::Key::H, Key::H), (egui::Key::I, Key::I),
                (egui::Key::J, Key::J), (egui::Key::K, Key::K), (egui::Key::L, Key::L),
                (egui::Key::M, Key::M), (egui::Key::N, Key::N), (egui::Key::O, Key::O),
                (egui::Key::P, Key::P), (egui::Key::Q, Key::Q), (egui::Key::R, Key::R),
                (egui::Key::S, Key::S), (egui::Key::T, Key::T), (egui::Key::U, Key::U),
                (egui::Key::V, Key::V), (egui::Key::W, Key::W), (egui::Key::X, Key::X),
                (egui::Key::Y, Key::Y), (egui::Key::Z, Key::Z),
            ] {
                if i.key_pressed(egui_key) {
                    return Some(our_key);
                }
            }

            // Check number keys
            for (egui_key, our_key) in [
                (egui::Key::Num0, Key::Num0), (egui::Key::Num1, Key::Num1),
                (egui::Key::Num2, Key::Num2), (egui::Key::Num3, Key::Num3),
                (egui::Key::Num4, Key::Num4), (egui::Key::Num5, Key::Num5),
                (egui::Key::Num6, Key::Num6), (egui::Key::Num7, Key::Num7),
                (egui::Key::Num8, Key::Num8), (egui::Key::Num9, Key::Num9),
            ] {
                if i.key_pressed(egui_key) {
                    return Some(our_key);
                }
            }

            // Check function keys
            for (egui_key, our_key) in [
                (egui::Key::F1, Key::F1), (egui::Key::F2, Key::F2),
                (egui::Key::F3, Key::F3), (egui::Key::F4, Key::F4),
                (egui::Key::F5, Key::F5), (egui::Key::F6, Key::F6),
                (egui::Key::F7, Key::F7), (egui::Key::F8, Key::F8),
                (egui::Key::F9, Key::F9), (egui::Key::F10, Key::F10),
                (egui::Key::F11, Key::F11), (egui::Key::F12, Key::F12),
            ] {
                if i.key_pressed(egui_key) {
                    return Some(our_key);
                }
            }

            // Check special keys
            if i.key_pressed(egui::Key::Space) { return Some(Key::Space); }
            if i.key_pressed(egui::Key::Enter) { return Some(Key::Enter); }
            if i.key_pressed(egui::Key::Tab) { return Some(Key::Tab); }
            if i.key_pressed(egui::Key::Backspace) { return Some(Key::Backspace); }
            if i.key_pressed(egui::Key::ArrowUp) { return Some(Key::Up); }
            if i.key_pressed(egui::Key::ArrowDown) { return Some(Key::Down); }
            if i.key_pressed(egui::Key::ArrowLeft) { return Some(Key::Left); }
            if i.key_pressed(egui::Key::ArrowRight) { return Some(Key::Right); }

            None
        })
    }

    fn draw_midi_keyboard(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 100.0),
            egui::Sense::click(),
        );

        let painter = ui.painter_at(rect);

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

        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let relative_pos = pos - rect.min;

                let mut clicked_note = None;
                for (i, &note) in white_notes.iter().enumerate() {
                    let note_in_octave = note % 12;
                    if !matches!(note_in_octave, 4 | 11) && i < num_white_keys - 1 {
                        let black_note = note + 1;
                        let x = (i as f32 + 1.0) * white_key_width - black_key_width / 2.0;

                        if relative_pos.x >= x
                            && relative_pos.x <= x + black_key_width
                            && relative_pos.y <= black_key_height
                        {
                            clicked_note = Some(black_note);
                            break;
                        }
                    }
                }

                if clicked_note.is_none() {
                    let index = (relative_pos.x / white_key_width) as usize;
                    if index < white_notes.len() {
                        clicked_note = Some(white_notes[index]);
                    }
                }

                if let Some(note) = clicked_note {
                    self.selected_note = Some(note);
                }
            }
        }

        for (i, &note) in white_notes.iter().enumerate() {
            let x = rect.min.x + i as f32 * white_key_width;
            let is_selected = Some(note) == self.selected_note;
            let has_mapping = self.current_mapping.mappings.contains_key(&note);

            let color = if is_selected {
                egui::Color32::from_rgb(100, 150, 255)
            } else if has_mapping {
                egui::Color32::from_rgb(200, 255, 200)
            } else {
                egui::Color32::WHITE
            };

            let key_rect = egui::Rect::from_min_size(
                egui::pos2(x, rect.min.y),
                egui::vec2(white_key_width, white_key_height),
            );

            painter.rect_filled(key_rect, 2.0, color);
            painter.rect_stroke(
                key_rect,
                2.0,
                egui::Stroke::new(1.0, egui::Color32::from_gray(100)),
                egui::epaint::StrokeKind::Outside,
            );
        }

        for (i, &note) in white_notes.iter().enumerate() {
            let note_in_octave = note % 12;
            if !matches!(note_in_octave, 4 | 11) && i < num_white_keys - 1 {
                let black_note = note + 1;
                let x = rect.min.x + (i as f32 + 1.0) * white_key_width - black_key_width / 2.0;

                let is_selected = Some(black_note) == self.selected_note;
                let has_mapping = self.current_mapping.mappings.contains_key(&black_note);

                let color = if is_selected {
                    egui::Color32::from_rgb(50, 100, 200)
                } else if has_mapping {
                    egui::Color32::from_rgb(100, 200, 100)
                } else {
                    egui::Color32::from_gray(40)
                };

                let key_rect = egui::Rect::from_min_size(
                    egui::pos2(x, rect.min.y),
                    egui::vec2(black_key_width, black_key_height),
                );

                painter.rect_filled(key_rect, 1.0, color);
                painter.rect_stroke(
                    key_rect,
                    1.0,
                    egui::Stroke::new(1.0, egui::Color32::BLACK),
                    egui::epaint::StrokeKind::Outside,
                );
            }
        }
    }

    fn draw_note_editor(
        &mut self,
        ui: &mut egui::Ui,
        note: u8,
        is_readonly: bool,
        log: &mut Vec<String>,
    ) {
        let note_name = xiv_midi::midi::MidiNote::new(note)
            .map(|n| n.full_name())
            .unwrap_or_else(|_| note.to_string());

        ui.heading(format!("Note: {} (MIDI {})", note_name, note));

        let has_mapping = self.current_mapping.mappings.contains_key(&note);

        if !has_mapping {
            ui.label("No mapping defined");

            if !is_readonly && ui.button("+ Add Mapping").clicked() {
                self.current_mapping.mappings.insert(
                    note,
                    NoteMapping {
                        on_press: vec![],
                        on_release: vec![],
                    },
                );
                self.is_modified = true;
                log.push(format!("Added mapping for note {}", note));
            }
        } else {
            // Draw action lists
            egui::ScrollArea::vertical()
                .max_height(400.0)
                .show(ui, |ui| {
                    // On Press actions
                    ui.label(egui::RichText::new("On Press:").strong());
                    self.draw_action_list(ui, note, ActionListType::OnPress, is_readonly);

                    ui.add_space(10.0);

                    // On Release actions
                    ui.label(egui::RichText::new("On Release:").strong());
                    self.draw_action_list(ui, note, ActionListType::OnRelease, is_readonly);
                });

            ui.add_space(10.0);

            if !is_readonly && ui.button("🗑 Remove Entire Mapping").clicked() {
                self.current_mapping.mappings.remove(&note);
                self.is_modified = true;
                log.push(format!("Removed mapping for note {}", note));
            }
        }
    }

    fn draw_action_list(
        &mut self,
        ui: &mut egui::Ui,
        note: u8,
        list_type: ActionListType,
        is_readonly: bool,
    ) {
        // Clone actions for display to avoid borrow issues
        let actions = {
            let mapping = self.current_mapping.mappings.get(&note).unwrap();
            match list_type {
                ActionListType::OnPress => mapping.on_press.clone(),
                ActionListType::OnRelease => mapping.on_release.clone(),
            }
        };

        let mut action_to_delete: Option<usize> = None;
        let mut action_to_edit: Option<usize> = None;
        let mut swap_indices: Option<(usize, usize)> = None;

        ui.indent(format!("action_list_{:?}", list_type), |ui| {
            for (index, action) in actions.iter().enumerate() {
                let _response = ui.horizontal(|ui| {
                    // Move up/down buttons for reordering
                    if !is_readonly {
                        if index > 0 {
                            if ui.small_button("⬆").on_hover_text("Move up").clicked() {
                                swap_indices = Some((index, index - 1));
                            }
                        } else {
                            ui.add_enabled(false, egui::Button::new("⬆").small());
                        }

                        if index < actions.len() - 1 {
                            if ui.small_button("⬇").on_hover_text("Move down").clicked() {
                                swap_indices = Some((index, index + 1));
                            }
                        } else {
                            ui.add_enabled(false, egui::Button::new("⬇").small());
                        }
                    }

                    // Action display
                    let action_text = format_action(action);
                    ui.label(action_text);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !is_readonly {
                            if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                                action_to_delete = Some(index);
                            }
                            if ui.small_button("✏").on_hover_text("Edit").clicked() {
                                action_to_edit = Some(index);
                            }
                        }
                    });
                }).response;
            }

            if actions.is_empty() {
                ui.label(egui::RichText::new("(empty)").italics().color(egui::Color32::GRAY));
            }

            if !is_readonly {
                if ui.button("+ Add Action").clicked() {
                    self.action_editor.reset();
                    self.editing_action_index = Some((list_type, actions.len()));
                    self.show_action_dialog = true;
                }
            }
        });

        // Process actions after rendering
        if let Some(index) = action_to_delete {
            let mapping = self.current_mapping.mappings.get_mut(&note).unwrap();
            let actions = match list_type {
                ActionListType::OnPress => &mut mapping.on_press,
                ActionListType::OnRelease => &mut mapping.on_release,
            };
            actions.remove(index);
            self.is_modified = true;
        }

        if let Some(index) = action_to_edit {
            let mapping = self.current_mapping.mappings.get(&note).unwrap();
            let actions = match list_type {
                ActionListType::OnPress => &mapping.on_press,
                ActionListType::OnRelease => &mapping.on_release,
            };
            self.action_editor.load_action(&actions[index]);
            self.editing_action_index = Some((list_type, index));
            self.show_action_dialog = true;
        }

        // Handle swap for reordering
        if let Some((from, to)) = swap_indices {
            let mapping = self.current_mapping.mappings.get_mut(&note).unwrap();
            let actions = match list_type {
                ActionListType::OnPress => &mut mapping.on_press,
                ActionListType::OnRelease => &mut mapping.on_release,
            };
            actions.swap(from, to);
            self.is_modified = true;
        }
    }
}

fn format_action(action: &Action) -> String {
    match action {
        Action::Press(key) => format!("Press: {:?}", key),
        Action::Release(key) => format!("Release: {:?}", key),
        Action::Delay(ms) => format!("Delay: {}ms", ms),
        Action::SetModifiers { shift, ctrl, alt } => {
            let mut parts = Vec::new();
            if *shift { parts.push("Shift"); }
            if *ctrl { parts.push("Ctrl"); }
            if *alt { parts.push("Alt"); }
            if parts.is_empty() {
                "SetModifiers: None".to_string()
            } else {
                format!("SetModifiers: {}", parts.join(" + "))
            }
        }
    }
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
            available_mappings: Vec::new(),
            selected_mapping_index: 0,
            mapping: create_ffxiv_default_mapping(),
            editor: MappingEditor::new(),
            event_tx,
            event_rx,
            log_messages: Vec::new(),
            active_notes: HashMap::new(),
            current_tab: AppTab::Main,
            status: "Ready".to_string(),
        };

        app.refresh_devices();
        app.scan_mapping_files();
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

    fn scan_mapping_files(&mut self) {
        self.available_mappings.clear();

        self.available_mappings.push(MappingOption {
            name: "Default FFXIV".to_string(),
            path: None,
            is_readonly: true,
        });

        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let mappings_dir = exe_dir.join("mappings");

                if mappings_dir.exists() && mappings_dir.is_dir() {
                    match std::fs::read_dir(&mappings_dir) {
                        Ok(entries) => {
                            let mut files: Vec<_> = entries
                                .filter_map(|entry| entry.ok())
                                .filter(|entry| {
                                    entry
                                        .path()
                                        .extension()
                                        .map(|ext| ext == "json")
                                        .unwrap_or(false)
                                })
                                .collect();

                            files.sort_by_key(|entry| entry.file_name());

                            for entry in files {
                                let path = entry.path();
                                let name = path
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("Unknown")
                                    .to_string();

                                self.available_mappings.push(MappingOption {
                                    name,
                                    path: Some(path),
                                    is_readonly: false,
                                });
                            }

                            self.log(format!(
                                "Found {} mapping file(s)",
                                self.available_mappings.len() - 1
                            ));
                        }
                        Err(e) => {
                            self.log(format!("Error reading mappings directory: {}", e));
                        }
                    }
                }
            }
        }
    }

    fn load_selected_mapping(&mut self) {
        let mapping_option = &self.available_mappings[self.selected_mapping_index];

        self.mapping = if let Some(ref path) = mapping_option.path {
            match MappingConfig::from_file(path) {
                Ok(m) => {
                    self.log(format!("Loaded mapping: {}", mapping_option.name));
                    m
                }
                Err(e) => {
                    self.log(format!(
                        "Error loading mapping '{}': {}, using default",
                        mapping_option.name, e
                    ));
                    create_ffxiv_default_mapping()
                }
            }
        } else {
            self.log("Using default FFXIV mapping".to_string());
            create_ffxiv_default_mapping()
        };
    }

    fn connect_device(&mut self, device_name: String) {
        self.log(format!("Connecting to '{}'...", device_name));

        self.load_selected_mapping();

        let keyboard = match EnigoKeyboardController::new() {
            Ok(k) => k,
            Err(e) => {
                self.log(format!("Error creating keyboard controller: {}", e));
                return;
            }
        };

        let engine = MidiEngine::new(keyboard, self.mapping.clone());

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

            // Tab bar
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(self.current_tab == AppTab::Main, "🎹 Main")
                    .clicked()
                {
                    // Check if switching from Editor with unsaved changes
                    if self.current_tab == AppTab::Editor && self.editor.is_modified {
                        self.editor.pending_action = Some(PendingAction::SwitchToMainTab);
                        self.editor.show_unsaved_dialog = true;
                    } else {
                        self.current_tab = AppTab::Main;
                    }
                }
                if ui
                    .selectable_label(self.current_tab == AppTab::Editor, "✏ Editor")
                    .clicked()
                {
                    self.current_tab = AppTab::Editor;
                    // Sync editor state when switching to editor tab
                    self.editor.scan_mappings(&mut self.log_messages);
                    self.editor
                        .load_mapping(self.selected_mapping_index, &mut self.log_messages);
                }
            });

            ui.separator();

            // Tab content
            match self.current_tab {
                AppTab::Main => self.draw_main_tab(ui),
                AppTab::Editor => self.editor.draw(ui, ctx, &mut self.log_messages),
            }

            // Handle tab switch request from editor
            if self.editor.switch_to_main_requested {
                self.current_tab = AppTab::Main;
                self.editor.switch_to_main_requested = false;
            }
        });

        ctx.request_repaint();
    }
}

impl XivMidiApp {
    fn draw_main_tab(&mut self, ui: &mut egui::Ui) {
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

        ui.horizontal(|ui| {
            if ui.button("🔄 Refresh Mappings").clicked() {
                self.scan_mapping_files();
            }

            let prev_index = self.selected_mapping_index;
            let current_name = &self.available_mappings[self.selected_mapping_index].name;

            egui::ComboBox::from_label("Key Mapping")
                .selected_text(current_name)
                .show_ui(ui, |ui| {
                    for (index, mapping) in self.available_mappings.iter().enumerate() {
                        ui.selectable_value(&mut self.selected_mapping_index, index, &mapping.name);
                    }
                });

            // Auto-apply when selection changes
            if prev_index != self.selected_mapping_index {
                self.load_selected_mapping();
                if self.connection.is_some() {
                    self.log("Mapping changed - disconnect and reconnect to apply".to_string());
                }
            }
        });

        ui.separator();

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

        ui.heading("Active Notes");
        self.draw_piano(ui);

        ui.separator();

        ui.heading("Mapping & Live Actions");
        self.draw_mapping_info(ui);

        ui.separator();

        ui.heading("Log");
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .max_height(200.0)
            .show(ui, |ui| {
                for msg in &self.log_messages {
                    ui.label(msg);
                }
            });
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

        for (i, &note) in white_notes.iter().enumerate() {
            let x = rect.min.x + i as f32 * white_key_width;
            let color = if let Some(&velocity) = self.active_notes.get(&note) {
                let intensity = (velocity as f32 / 127.0).clamp(0.4, 1.0);
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

        for (i, &note) in white_notes.iter().enumerate() {
            let note_in_octave = note % 12;
            if !matches!(note_in_octave, 4 | 11) && i < num_white_keys - 1 {
                let black_note = note + 1;
                let x = rect.min.x + (i as f32 + 1.0) * white_key_width - black_key_width / 2.0;

                let color = if let Some(&velocity) = self.active_notes.get(&black_note) {
                    let _intensity = (velocity as f32 / 127.0).clamp(0.4, 1.0);
                    egui::Color32::from_rgb(0, 255, 0)
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
