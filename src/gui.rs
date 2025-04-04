use crate::backend::{ChecksumReport, Progress, SizeResult, copy_dirs};
use csv::Writer;
use eframe::egui::{Context, Ui};
use eframe::emath::Numeric;
use eframe::{App, Frame, egui};
use human_bytes::human_bytes;
use rfd::FileDialog;
use rust_i18n::t;
use std::error::Error;
use std::fs::File;
use std::path::{Path, PathBuf};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::watch;

enum AppState {
    Input {
        source_path: Option<PathBuf>,
        destination_path: Vec<Option<PathBuf>>,
        tooltip: Option<String>,
    },
    Copying {
        return_receiver: watch::Receiver<Option<SizeResult>>,
        progress_receiver: watch::Receiver<Progress>,
    },
}

impl Default for AppState {
    fn default() -> Self {
        Self::Input {
            source_path: None,
            destination_path: vec![],
            tooltip: None,
        }
    }
}

pub struct LibreCardApp {
    rt: Runtime,
    state: AppState,
}

impl Default for LibreCardApp {
    fn default() -> Self {
        Self {
            state: AppState::default(),
            rt: Builder::new_multi_thread().enable_all().build().unwrap(),
        }
    }
}

impl App for LibreCardApp {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let next_state = match &mut self.state {
                AppState::Input {
                    source_path,
                    destination_path,
                    tooltip,
                } => Self::input_ui(ui, &self.rt, source_path, destination_path, tooltip),
                AppState::Copying {
                    return_receiver: rx1,
                    progress_receiver: rx2,
                } => Self::copying_ui(ui, ctx, rx1, rx2),
            };
            if let Some(new_state) = next_state {
                self.state = new_state;
            }
        });
    }
}

impl LibreCardApp {
    fn input_ui(
        ui: &mut Ui,
        rt: &Runtime,
        source_path: &mut Option<PathBuf>,
        destination_paths: &mut Vec<Option<PathBuf>>,
        tooltip: &mut Option<String>,
    ) -> Option<AppState> {
        ui.heading(t!("title.input"));
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
            // Source: <input textbox> <browse button>
            ui.horizontal(|ui| {
                ui.label(t!("src_folder"));

                let path_text = match source_path {
                    Some(path) => path.to_string_lossy().to_string(),
                    None => t!("src_folder.not_selected").to_string(),
                };

                ui.label(path_text);

                if ui.button(t!("browse_folder")).clicked() {
                    if let Some(path) = FileDialog::new().pick_folder() {
                        *source_path = Some(path);
                    }
                }
            });

            // Destinations:
            // <input textbox> <browse button> <delete button>
            // ...
            // <add button>
            for (i, path) in destination_paths.clone().iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(t!("dst_folder"));
                    let path_text = match path {
                        Some(path) => path.to_string_lossy().to_string(),
                        None => t!("dst_folder.not_selected").to_string(),
                    };

                    ui.label(path_text);

                    if ui.button(t!("browse_folder")).clicked() {
                        if let Some(path) = FileDialog::new().pick_folder() {
                            destination_paths[i] = Some(path);
                        }
                    }

                    if ui.button(t!("dst_folder.delete")).clicked() {
                        destination_paths.remove(i);
                    }
                });
            }
        });
        if ui.button(t!("dst_folder.add")).clicked() {
            destination_paths.push(None);
        }

        // <form tooltip>
        if let Some(tooltip) = &tooltip {
            ui.label(tooltip);
        }

        // <start button>
        if ui.button(t!("copying.start")).clicked() {
            if source_path.is_none() {
                *tooltip = Some(t!("src_folder.not_selected").to_string());
                return None;
            }
            if destination_paths.is_empty() {
                *tooltip = Some(t!("dst_folder.not_selected").to_string());
                return None;
            }

            let source_path = source_path.clone().unwrap();

            let mut destination_paths_checked: Vec<PathBuf> = vec![];
            for path in destination_paths {
                if let Some(path) = path {
                    destination_paths_checked.push(path.clone());
                } else {
                    *tooltip = Some(t!("dst_folder.not_selected").to_string());
                    return None;
                }
            }
            let destination_paths_checked = destination_paths_checked;

            // Start the copying process
            let (tx1, rx1) = watch::channel(None);
            let (tx2, rx2) = watch::channel(Progress::default());
            rt.spawn(async move {
                let result = copy_dirs(&source_path, &destination_paths_checked, tx2).await;
                tx1.send(Some(result)).unwrap();
            });

            return Some(AppState::Copying {
                return_receiver: rx1,
                progress_receiver: rx2,
            });
        }

        None
    }

    fn copying_ui(
        ui: &mut Ui,
        ctx: &Context,
        return_receiver: &mut watch::Receiver<Option<SizeResult>>,
        progress_receiver: &mut watch::Receiver<Progress>,
    ) -> Option<AppState> {
        let result = &*return_receiver.borrow_and_update();
        let progress = &*progress_receiver.borrow_and_update();
        match result {
            Some(r) => match r {
                Ok(bytes) => {
                    ui.heading(t!("copying.finished", size => human_bytes(bytes.to_f64())));
                }
                Err(e) => {
                    ui.heading(t!("copying.error", error => format!("{}", e)));
                }
            },
            None => {
                ui.heading(t!("copying", copied => progress.completed, total => progress.total));
                ctx.request_repaint();
            }
        }

        None
    }
}

impl ChecksumReport {
    pub fn export_report<P: AsRef<Path>>(&self, to_file: P) -> Result<(), Box<dyn Error>> {
        let file = File::create(to_file)?;
        let mut writer = Writer::from_writer(file);
        let mut header: Vec<String> = vec!["Source".to_owned(), "Source Hash".to_owned()];
        for i in 0..self.0.len() {
            header.push(format!("Destination File {}", i + 1));
            header.push(format!("Destination Hash {}", i + 1));
        }
        writer.write_record(header)?;

        for row in &self.0 {
            let mut record: Vec<String> = vec![
                row.source_hash.0.to_string_lossy().into_owned(),
                format!("{:X}", row.source_hash.1).to_owned(),
            ];
            for dest in &row.destination_hash {
                record.push(dest.0.to_string_lossy().into_owned());
                record.push(format!("{:X}", dest.1).to_owned());
            }
            writer.write_record(record)?;
        }
        writer.flush()?;
        Ok(())
    }
}
