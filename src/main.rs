use eframe::{egui, App, Frame};
use egui::{Context, RichText};
use rfd::FileDialog;
use std::path::PathBuf;

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        ..Default::default()
    };
    eframe::run_native(
        "File Path Selector",
        options,
        Box::new(|_cc| Ok(Box::new(FilePathSelectorApp::default()))),
    )
}

struct FilePathSelectorApp {
    source_path: Option<PathBuf>,
    destination_path: Option<PathBuf>,
}

impl Default for FilePathSelectorApp {
    fn default() -> Self {
        Self {
            source_path: None,
            destination_path: None,
        }
    }
}

impl App for FilePathSelectorApp {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("File Path Selector");
            ui.add_space(10.0);

            // Source path selection
            ui.horizontal(|ui| {
                ui.label("Source:");

                let path_text = match &self.source_path {
                    Some(path) => path.to_string_lossy().to_string(),
                    None => "No file selected".to_string(),
                };

                ui.text_edit_singleline(&mut path_text.clone());

                if ui.button("Browse...").clicked() {
                    if let Some(path) = FileDialog::new().pick_file() {
                        self.source_path = Some(path);
                    }
                }
            });

            ui.add_space(10.0);

            // Destination path selection
            ui.horizontal(|ui| {
                ui.label("Destination:");

                let path_text = match &self.destination_path {
                    Some(path) => path.to_string_lossy().to_string(),
                    None => "No file selected".to_string(),
                };

                ui.text_edit_singleline(&mut path_text.clone());

                if ui.button("Browse...").clicked() {
                    if let Some(path) = FileDialog::new().pick_folder() {
                        self.destination_path = Some(path);
                    }
                }
            });

            ui.add_space(20.0);

            // Display the selected paths
            ui.group(|ui| {
                ui.label(RichText::new("Selected Paths:").strong());
                ui.label(format!("Source: {}", self.source_path
                    .as_ref()
                    .map_or("None".to_string(), |p| p.to_string_lossy().to_string())));
                ui.label(format!("Destination: {}", self.destination_path
                    .as_ref()
                    .map_or("None".to_string(), |p| p.to_string_lossy().to_string())));
            });
        });
    }
}
