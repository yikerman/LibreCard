use crate::backend::{ChecksumReport, CopyProgress, copy_dir_threaded};
use csv::Writer;
use eframe::egui::Context;
use eframe::{App, Frame, egui};
use rfd::FileDialog;
use std::error::Error;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct LibreCardApp {
    source_path: Option<PathBuf>,
    destination_path: Option<PathBuf>,
    info: Option<String>,
    progress: Option<Arc<Mutex<CopyProgress>>>,
}

impl Default for LibreCardApp {
    fn default() -> Self {
        Self {
            source_path: None,
            destination_path: None,
            info: None,
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

                    if ui.button("浏览文件夹").clicked() {
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

                    if ui.button("浏览文件夹").clicked() {
                        if let Some(path) = FileDialog::new().pick_folder() {
                            self.destination_path = Some(path);
                        }
                    }
                });
            });

            let mut start_button = false;
            if let Some(progress) = self.progress.clone() {
                let progress = progress.lock().unwrap();
                match &*progress {
                    CopyProgress::Copy { total, copied } => {
                        ui.label(format!("拷贝中... {}/{}", copied, total));
                        ctx.request_repaint();
                    }
                    CopyProgress::Checksum { total, completed } => {
                        ui.label(format!("校验中... {}/{}", completed, total));
                        ctx.request_repaint();
                    }
                    CopyProgress::Finished { report } => {
                        ui.label(format!(
                            "完成拷贝 {} 个文件，有 {} 个错误",
                            report.total_files(),
                            report.count_errors()
                        ));
                        if ui.button("导出报告").clicked() {
                            if let Some(path) =
                                FileDialog::new().set_file_name("report.csv").save_file()
                            {
                                if let Err(e) = report.export_report(path) {
                                    self.info = Some(e.to_string());
                                } else {
                                    self.info = Some("报告已导出".to_string());
                                }
                            }
                        }
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
                if ui.button("开始拷贝").clicked() {
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
                        self.info = Some("未输入源文件夹和目标文件夹".to_owned());
                    }
                }
            }

            if let Some(info) = &self.info {
                ui.label(info);
            }
        });
    }
}

impl ChecksumReport {
    pub fn export_report<P: AsRef<Path>>(&self, to_file: P) -> Result<(), Box<dyn Error>> {
        let file = File::create(to_file)?;
        let mut writer = Writer::from_writer(file);
        writer.write_record(&[
            "源文件",
            "源文件哈希",
            "目标文件",
            "目标文件哈希",
            "哈希一致",
        ])?;
        for row in &self.0 {
            writer.write_record(&csv::StringRecord::from(vec![
                row.source.to_string_lossy(),
                format!("{:016x}", row.source_hash).into(),
                row.destination.to_string_lossy(),
                format!("{:016x}", row.destination_hash).into(),
                if row.source_hash == row.destination_hash {
                    "是".into()
                } else {
                    "否".into()
                },
            ]))?;
        }
        writer.flush()?;
        Ok(())
    }
}
