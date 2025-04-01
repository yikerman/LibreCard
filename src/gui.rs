use crate::backend::{CopyProgress, copy_dir_threaded};
use eframe::egui::Context;
use eframe::{App, Frame, egui};
use rfd::FileDialog;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct LibreCardApp {
    source_path: Option<PathBuf>,
    destination_path: Option<PathBuf>,
    input_error: Option<String>,
    progress: Option<Arc<Mutex<CopyProgress>>>,
}

impl Default for LibreCardApp {
    fn default() -> Self {
        Self {
            source_path: None,
            destination_path: None,
            input_error: None,
            progress: None,
        }
    }
}

impl App for LibreCardApp {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                // Source: <input> <Browse...>
                ui.horizontal(|ui| {
                    ui.label("源文件夹");

                    let path_text = match &self.source_path {
                        Some(path) => path.to_string_lossy().to_string(),
                        None => "未选择源文件夹".to_string(),
                    };

                    ui.add_sized(
                        [ui.available_width() - 85.0, 20.0],
                        egui::TextEdit::singleline(&mut path_text.clone()),
                    );

                    if ui.button("浏览...").clicked() {
                        if let Some(path) = FileDialog::new().pick_folder() {
                            self.source_path = Some(path);
                        }
                    }
                });

                // Destination: <input> <Browse...>
                ui.horizontal(|ui| {
                    ui.label("目标文件夹");

                    let path_text = match &self.destination_path {
                        Some(path) => path.to_string_lossy().to_string(),
                        None => "未选择目标文件夹".to_string(),
                    };

                    ui.add_sized(
                        [ui.available_width() - 85.0, 20.0],
                        egui::TextEdit::singleline(&mut path_text.clone()),
                    );

                    if ui.button("浏览...").clicked() {
                        if let Some(path) = FileDialog::new().pick_folder() {
                            self.destination_path = Some(path);
                        }
                    }
                });
            });

            if let Some(error) = &self.input_error {
                ui.label(error);
            }

            let mut start_button = false;
            if let Some(progress) = self.progress.clone() {
                let progress = progress.lock().unwrap();
                match &*progress {
                    CopyProgress::Copy { total, copied } => {
                        ui.label(format!("拷贝中... {}/{}", copied, total));
                    }
                    CopyProgress::Checksum { total, completed } => {
                        ui.label(format!("校验中... {}/{}", completed, total));
                    }
                    CopyProgress::Finished { report } => {
                        ui.label(format!(
                            "完成拷贝 {} 个文件，有 {} 个错误",
                            report.total_files(),
                            report.count_errors()
                        ));
                        start_button = true;
                    }
                    CopyProgress::Error { error } => {
                        ui.label(format!("错误： {}", error));
                        start_button = true;
                    }
                }
            } else {
                start_button = true;
            }

            if start_button {
                if ui.button("拷贝").clicked() {
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
                        self.input_error = Some("未输入源文件夹和目标文件夹".to_owned());
                    }
                }
            }
        });
    }
}
