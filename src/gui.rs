use crate::backend::{
    ChecksumReport, Progress, SizeResult, copy_dirs, flatten_filetree, hash_dirs,
};
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
use tokio::io;
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
        to_checksum: Vec<PathBuf>,
        source_path: PathBuf,
        destination_path: Vec<PathBuf>,
    },
    Checksum {
        return_receiver: watch::Receiver<Option<io::Result<ChecksumReport>>>,
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
                    to_checksum,
                    source_path: src,
                    destination_path: dst,
                } => Self::copying_ui(ui, ctx, &self.rt, to_checksum, rx1, rx2, src, dst),
                AppState::Checksum {
                    return_receiver: rx1,
                    progress_receiver: rx2,
                } => Self::checksum_ui(ui, ctx, rx1, rx2),
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
        ui.heading(t!("input.title"));
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

            // FIXME: Repeated globbing
            let globbed_files_result = flatten_filetree(&source_path);
            let globbed_files = match globbed_files_result {
                Ok(files) => files,
                Err(e) => {
                    eprintln!("Error globbing files: {}", e);
                    *tooltip = Some(t!("error", error => format!("{}", e)).to_string());
                    return None;
                }
            };

            // Start the copying process
            let (tx1, rx1) = watch::channel(None);
            let (tx2, rx2) = watch::channel(Progress::default());
            let source_path_clone = source_path.clone();
            let destination_paths_checked_clone = destination_paths_checked.clone();
            rt.spawn(async move {
                let result = copy_dirs(&source_path, &destination_paths_checked, tx2).await;
                tx1.send(Some(result)).unwrap();
            });

            return Some(AppState::Copying {
                return_receiver: rx1,
                progress_receiver: rx2,
                to_checksum: globbed_files,
                source_path: source_path_clone,
                destination_path: destination_paths_checked_clone,
            });
        }

        None
    }

    fn copying_ui(
        ui: &mut Ui,
        ctx: &Context,
        rt: &Runtime,
        to_checksum: &Vec<PathBuf>,
        return_receiver: &mut watch::Receiver<Option<SizeResult>>,
        progress_receiver: &mut watch::Receiver<Progress>,
        source_path: &PathBuf,
        destination_paths: &Vec<PathBuf>,
    ) -> Option<AppState> {
        let result = &*return_receiver.borrow_and_update();
        let progress = &*progress_receiver.borrow_and_update();
        match result {
            Some(r) => match r {
                Ok(bytes) => {
                    ui.heading(t!("copying.finished", size => human_bytes(bytes.to_f64())));
                    if ui.button(t!("checksum.start")).clicked() {
                        // Start the checksum process
                        let (tx1, rx1) = watch::channel(None);
                        let (tx2, rx2) = watch::channel(Progress::default());
                        let source_path = source_path.clone();
                        let destination_paths = destination_paths.clone();
                        let to_checksum = to_checksum.clone();
                        rt.spawn(async move {
                            let result =
                                hash_dirs(&source_path, &destination_paths, &to_checksum, tx2)
                                    .await;
                            tx1.send(Some(result)).unwrap();
                        });

                        return Some(AppState::Checksum {
                            return_receiver: rx1,
                            progress_receiver: rx2,
                        });
                    }
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

    fn checksum_ui(
        ui: &mut Ui,
        ctx: &Context,
        return_receiver: &mut watch::Receiver<Option<io::Result<ChecksumReport>>>,
        progress_receiver: &mut watch::Receiver<Progress>,
    ) -> Option<AppState> {
        let result = &*return_receiver.borrow_and_update();
        let progress = &*progress_receiver.borrow_and_update();
        match result {
            Some(r) => match r {
                Ok(report) => {
                    let total = report.total_files();
                    let failed = report.count_errors();
                    ui.heading(t!("checksum.finished", total => total, failed => failed));
                    if ui.button(t!("checksum.export")).clicked() {
                        if let Some(path) =
                            FileDialog::new().set_file_name("report.csv").save_file()
                        {
                            // TODO: Show UI
                            if let Err(e) = report.export_report(path) {
                                eprintln!("Error exporting report: {}", e);
                            } else {
                                eprintln!("Successfully exported report");
                            }
                        }
                    }
                }
                Err(e) => {
                    ui.heading(t!("checksum.error", error => format!("{}", e)));
                }
            },
            None => {
                ui.heading(
                    t!("checksum", completed => progress.completed, total => progress.total),
                );
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
        let mut header: Vec<String> = vec![
            "Consistent".to_owned(),
            "Source".to_owned(),
            "Source Hash".to_owned(),
        ];
        let row0 = &self.0[0];
        for i in 0..row0.destinations.len() {
            header.push(format!("Destination File {}", i + 1));
            header.push(format!("Destination Hash {}", i + 1));
        }
        writer.write_record(header)?;

        for row in &self.0 {
            let mut record: Vec<String> = vec![
                if row.consistent() {
                    "Y".to_owned()
                } else {
                    "N".to_owned()
                },
                row.source.0.to_string_lossy().into_owned(),
                format!("{:X}", row.source.1).to_owned(),
            ];
            for dest in &row.destinations {
                record.push(dest.0.to_string_lossy().into_owned());
                record.push(format!("{:X}", dest.1).to_owned());
            }
            writer.write_record(record)?;
        }
        writer.flush()?;
        Ok(())
    }
}
