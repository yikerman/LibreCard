use crate::backend::{ChecksumReport, CopyProgress, copy_dir_threaded};
use csv::Writer;
use eframe::egui::{Context, Ui};
use eframe::{App, Frame, egui};
use rfd::FileDialog;
use std::error::Error;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use rust_i18n::t;

pub struct LibreCardApp {
    source_path: Option<PathBuf>,
    destination_path: Option<PathBuf>,
    tooltip: Option<String>,
    progress: Option<Arc<Mutex<CopyProgress>>>,
}

impl Default for LibreCardApp {
    fn default() -> Self {
        Self {
            source_path: None,
            destination_path: None,
            tooltip: None,
            progress: None,
        }
    }
}

impl App for LibreCardApp {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            self.file_selection(ui);

            self.start_and_progress(ctx, ui);

            if let Some(info) = &self.tooltip {
                ui.label(info);
            }
        });
    }
}

impl LibreCardApp {
    fn file_selection(&mut self, ui: &mut Ui) {
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
            // Source: <input> <Browse...>
            ui.horizontal(|ui| {
                ui.label(t!("src_folder"));

                let path_text = match &self.source_path {
                    Some(path) => path.to_string_lossy().to_string(),
                    None => t!("src_folder.not_selected").to_string(),
                };

                ui.add_sized(
                    [ui.available_width() - 85.0, 20.0],
                    egui::TextEdit::singleline(&mut path_text.clone()),
                );

                if ui.button(t!("browse_folder")).clicked() {
                    if let Some(path) = FileDialog::new().pick_folder() {
                        self.source_path = Some(path);
                    }
                }
            });

            // Destination: <input> <Browse...>
            ui.horizontal(|ui| {
                ui.label(t!("dest_folder"));

                let path_text = match &self.destination_path {
                    Some(path) => path.to_string_lossy().to_string(),
                    None => t!("dest_folder.not_selected").to_string(),
                };

                ui.add_sized(
                    [ui.available_width() - 85.0, 20.0],
                    egui::TextEdit::singleline(&mut path_text.clone()),
                );

                if ui.button(t!("browse_folder")).clicked() {
                    if let Some(path) = FileDialog::new().pick_folder() {
                        self.destination_path = Some(path);
                    }
                }
            });
        });
    }

    fn start_and_progress(&mut self, ctx: &Context, ui: &mut Ui) {
        let mut start_button = false;
        if let Some(progress) = self.progress.clone() {
            let progress = progress.lock().unwrap();
            match &*progress {
                CopyProgress::Copy { total, copied } => {
                    ui.label(t!("copying", total => total, copied => copied));
                    ctx.request_repaint();
                }
                CopyProgress::Checksum { total, completed } => {
                    ui.label(t!("checksum", total => total, completed => completed));
                    ctx.request_repaint();
                }
                CopyProgress::Finished { report } => {
                    ui.label(
                        t!("copying.finished", total => report.0.len()),
                    );
                    if ui.button(t!("export_report")).clicked() {
                        if let Some(path) =
                            FileDialog::new().set_file_name("report.csv").save_file()
                        {
                            if let Err(e) = report.export_report(path) {
                                self.tooltip = Some(e.to_string());
                            } else {
                                self.tooltip = Some(t!("export_report.finished").to_string());
                            }
                        }
                    }
                    start_button = true;
                }
                CopyProgress::Error { error } => {
                    ui.label(t!("copying.error", error => error));
                    start_button = true;
                }
            }
        } else {
            start_button = true;
        }

        if start_button {
            if ui.button(t!("copying.start")).clicked() {
                if let (Some(source), Some(destination)) =
                    (self.source_path.clone(), self.destination_path.clone())
                {
                    let progress = Arc::new(Mutex::new(CopyProgress::Copy {
                        total: 0,
                        copied: 0,
                    }));
                    self.progress = Some(progress.clone());
                    copy_dir_threaded(&source, &destination, progress).unwrap();
                } else {
                    self.tooltip = Some(t!("folder_not_selected").to_string());
                }
            }
        }
    }
}

impl ChecksumReport {
    pub fn export_report<P: AsRef<Path>>(&self, to_file: P) -> Result<(), Box<dyn Error>> {
        let file = File::create(to_file)?;
        let mut writer = Writer::from_writer(file);
        writer.write_record(&[
            "Source",
            "Source Hash",
            "Destination",
            "Destination Hash",
            "Passed Checksum",
        ])?;
        for row in &self.0 {
            writer.write_record(&csv::StringRecord::from(vec![
                row.source.to_string_lossy(),
                format!("{:016x}", row.source_hash).into(),
                row.destination.to_string_lossy(),
                format!("{:016x}", row.destination_hash).into(),
                if row.source_hash == row.destination_hash {
                    "Y".into()
                } else {
                    "N".into()
                },
            ]))?;
        }
        writer.flush()?;
        Ok(())
    }
}
