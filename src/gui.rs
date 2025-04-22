use crate::backend::{copy_dirs, flatten_dir_files, hash_dirs, ChecksumReport, Progress};
use human_bytes::human_bytes;
use iced::widget::{button, column, container, progress_bar, row, text, text_input};
use iced::{time, Color, Element, Length, Subscription, Task};
use rfd::FileDialog;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::watch;

#[derive(Debug, Default)]
enum LibreCardAppStage {
    #[default]
    Input,

    Copying {
        progress: Progress,
        rx: watch::Receiver<Progress>,
        completed: bool,
    },

    Checksumming {
        progress: Progress,
        rx: watch::Receiver<Progress>,
    },

    ChecksumComplete {
        report: ChecksumReport,
    },
}

#[derive(Debug, Default)]
pub struct LibreCardApp {
    stage: LibreCardAppStage,
    source_directory: Option<PathBuf>,
    destination_directories: Vec<Option<PathBuf>>,
    error_message: Option<String>,
    total_bytes_copied: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum LibreCardMessage {
    // Input stage messages
    OpenSourceDirectoryDialog,
    OpenDestinationDirectoryDialog(usize),
    AddDestinationDirectory,
    RemoveDestinationDirectory(usize),

    // Action messages
    StartCopy,
    StartChecksum,
    ExportChecksum,

    // Progress updates
    Tick,
    CopyCompleted(Result<u64, String>),
    ChecksumCompleted(Result<ChecksumReport, String>),
    ExportCompleted(Result<(), String>),

    // Error handling
    DismissError,
}

impl LibreCardApp {
    pub fn update(&mut self, message: LibreCardMessage) -> Task<LibreCardMessage> {
        match message {
            LibreCardMessage::Tick => {
                // Poll progress channel on timer tick
                match &mut self.stage {
                    LibreCardAppStage::Copying {
                        progress,
                        rx,
                        completed,
                    } => {
                        if !*completed {
                            if let Ok(true) = rx.has_changed() {
                                *progress = *rx.borrow();
                            }
                        }
                    }
                    LibreCardAppStage::Checksumming { progress, rx } => {
                        if let Ok(true) = rx.has_changed() {
                            *progress = *rx.borrow();
                        }
                    }
                    _ => {}
                }
                Task::none()
            }

            LibreCardMessage::OpenSourceDirectoryDialog => {
                let dir = FileDialog::new().pick_folder();
                self.source_directory = dir;
                Task::none()
            }

            LibreCardMessage::OpenDestinationDirectoryDialog(index) => {
                if index < self.destination_directories.len() {
                    self.destination_directories[index] = FileDialog::new().pick_folder();
                }
                Task::none()
            }

            LibreCardMessage::AddDestinationDirectory => {
                self.destination_directories.push(None);
                Task::none()
            }

            LibreCardMessage::RemoveDestinationDirectory(index) => {
                if self.destination_directories.len() > 1 {
                    self.destination_directories.remove(index);
                }
                Task::none()
            }

            LibreCardMessage::StartCopy => {
                // Validate input
                if self.source_directory.is_none() {
                    self.error_message = Some("Source directory not selected.".to_string());
                    return Task::none();
                }

                let valid_destinations: Vec<PathBuf> = self
                    .destination_directories
                    .iter()
                    .filter_map(|opt| opt.clone())
                    .collect();

                if valid_destinations.is_empty() {
                    self.error_message =
                        Some("No valid destination directories selected.".to_string());
                    return Task::none();
                }

                // Start copy operation
                let source = self.source_directory.clone().unwrap();
                let destinations = valid_destinations;

                let (tx, rx) = watch::channel(Progress::default());

                self.stage = LibreCardAppStage::Copying {
                    progress: Progress::default(),
                    rx,
                    completed: false,
                };

                // Task to perform the copy operation
                Task::perform(
                    async move {
                        match copy_dirs(&source, &destinations, tx).await {
                            Ok(bytes) => LibreCardMessage::CopyCompleted(Ok(bytes)),
                            Err(e) => LibreCardMessage::CopyCompleted(Err(e.to_string())),
                        }
                    },
                    |msg| msg,
                )
            }

            LibreCardMessage::CopyCompleted(result) => {
                match result {
                    Ok(bytes) => {
                        self.total_bytes_copied = Some(bytes);
                        if let LibreCardAppStage::Copying { completed, .. } = &mut self.stage {
                            *completed = true;
                        }
                    }
                    Err(error) => {
                        self.stage = LibreCardAppStage::Input;
                        self.error_message = Some(error);
                    }
                }
                Task::none()
            }

            LibreCardMessage::StartChecksum => {
                let in_copy_stage = matches!(
                    self.stage,
                    LibreCardAppStage::Copying {
                        completed: true,
                        ..
                    }
                );

                if !in_copy_stage {
                    return Task::none();
                }

                let source = self.source_directory.clone().unwrap();
                let destinations: Vec<PathBuf> = self
                    .destination_directories
                    .iter()
                    .filter_map(|opt| opt.clone())
                    .collect();

                // Get list of files to checksum
                match flatten_dir_files(&source) {
                    Ok(files) => {
                        let (tx, rx) = watch::channel(Progress::default());

                        self.stage = LibreCardAppStage::Checksumming {
                            progress: Progress::default(),
                            rx,
                        };

                        // Task to perform the checksum operation
                        Task::perform(
                            async move {
                                match hash_dirs(&source, &destinations, &files, tx).await {
                                    Ok(report) => LibreCardMessage::ChecksumCompleted(Ok(report)),
                                    Err(e) => {
                                        LibreCardMessage::ChecksumCompleted(Err(e.to_string()))
                                    }
                                }
                            },
                            |msg| msg,
                        )
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to list files: {}", e));
                        Task::none()
                    }
                }
            }

            LibreCardMessage::ChecksumCompleted(result) => {
                match result {
                    Ok(report) => {
                        self.stage = LibreCardAppStage::ChecksumComplete { report };
                    }
                    Err(error) => {
                        self.stage = LibreCardAppStage::Input;
                        self.error_message = Some(error);
                    }
                }
                Task::none()
            }

            LibreCardMessage::ExportChecksum => {
                if let LibreCardAppStage::ChecksumComplete { ref report } = self.stage {
                    let report_clone = report.clone();

                    Task::perform(
                        async move {
                            if let Some(path) = FileDialog::new()
                                .add_filter("CSV", &["csv"])
                                .set_file_name("checksum_report.csv")
                                .save_file()
                            {
                                match report_clone.export_report(path) {
                                    Ok(()) => LibreCardMessage::ExportCompleted(Ok(())),
                                    Err(err) => {
                                        LibreCardMessage::ExportCompleted(Err(err.to_string()))
                                    }
                                }
                            } else {
                                LibreCardMessage::ExportCompleted(Ok(()))
                            }
                        },
                        |msg| msg,
                    )
                } else {
                    Task::none()
                }
            }

            LibreCardMessage::ExportCompleted(result) => {
                if let Err(error) = result {
                    self.error_message = Some(format!("Failed to export report: {}", error));
                }
                Task::none()
            }

            LibreCardMessage::DismissError => {
                self.error_message = None;
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<LibreCardMessage> {
        let content = match &self.stage {
            LibreCardAppStage::Input => self.view_input_stage(),
            LibreCardAppStage::Copying {
                progress,
                completed,
                ..
            } => self.view_copy_stage(progress, *completed),
            LibreCardAppStage::Checksumming { progress, .. } => self.view_checksum_stage(progress),
            LibreCardAppStage::ChecksumComplete { report } => {
                self.view_checksum_complete_stage(report)
            }
        };

        let content: Element<LibreCardMessage> = if let Some(error) = &self.error_message {
            column![
                content,
                container(
                    column![
                        text(error).color(Color::from_rgb(0.9, 0.0, 0.0)),
                        button(text("Dismiss")).on_press(LibreCardMessage::DismissError),
                    ]
                    .spacing(10)
                )
                .width(Length::Fill)
                .padding(20)
            ]
            .spacing(20)
            .into()
        } else {
            content
        };

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .into()
    }

    pub fn subscription(&self) -> Subscription<LibreCardMessage> {
        match &self.stage {
            LibreCardAppStage::Copying {
                completed: false, ..
            }
            | LibreCardAppStage::Checksumming { .. } => {
                time::every(Duration::from_millis(200)).map(|_| LibreCardMessage::Tick)
            }
            _ => Subscription::none(),
        }
    }
}

impl LibreCardApp {
    fn view_input_stage(&self) -> Element<LibreCardMessage> {
        let title = text("Choose Source & Destination")
            .size(28)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center);

        // Source directory
        let source_path = self
            .source_directory
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "No directory selected".to_string());

        let source_row = row![
            text("Source Directory:").width(Length::FillPortion(1)),
            text_input("", &source_path)
                .padding(10)
                .width(Length::FillPortion(3)),
            button("Browse").on_press(LibreCardMessage::OpenSourceDirectoryDialog),
        ]
        .spacing(10)
        .align_y(iced::alignment::Alignment::Center);

        // Destination directories
        let mut destination_rows = Vec::new();
        for (idx, dest_opt) in self.destination_directories.iter().enumerate() {
            let dest_path = dest_opt
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "No directory selected".to_string());

            let mut row_elements = vec![
                text(format!("Destination {}:", idx + 1))
                    .width(Length::FillPortion(1))
                    .into(),
                text_input("", &dest_path)
                    .padding(10)
                    .width(Length::FillPortion(3))
                    .into(),
                button("Browse")
                    .on_press(LibreCardMessage::OpenDestinationDirectoryDialog(idx))
                    .into(),
            ];

            // Add remove button if more than one destination exists
            if self.destination_directories.len() > 1 {
                row_elements.push(
                    button("Remove")
                        .on_press(LibreCardMessage::RemoveDestinationDirectory(idx))
                        .into(),
                );
            }

            destination_rows.push(
                row(row_elements)
                    .spacing(10)
                    .align_y(iced::alignment::Alignment::Center),
            );
        }

        // Add destination button
        let add_button =
            button("Add Destination Directory").on_press(LibreCardMessage::AddDestinationDirectory);

        // Start copy button - only enabled if we have valid source and at least one destination
        let is_valid_input = self.source_directory.is_some()
            && self.destination_directories.iter().any(|d| d.is_some());

        let start_button = button(text("Start Copy").size(20))
            .width(Length::Fill)
            .padding(15);

        let start_button = if is_valid_input {
            start_button.on_press(LibreCardMessage::StartCopy)
        } else {
            start_button
        };

        // Assemble everything
        let mut content = column![title, source_row].spacing(20);

        for row in destination_rows {
            content = content.push(row);
        }

        content = content
            .push(add_button)
            .push(start_button)
            .spacing(20)
            .padding(20)
            .width(Length::Fill);

        container(content).into()
    }

    fn view_copy_stage(&self, progress: &Progress, completed: bool) -> Element<LibreCardMessage> {
        let title = text("Copying Files")
            .size(28)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center);

        let progress_value = if progress.total == 0 {
            0.0
        } else {
            progress.completed as f32 / progress.total as f32
        };

        let progress_bar = progress_bar(0.0..=1.0, progress_value)
            .width(Length::Fill)
            .height(30);

        let progress_text = text(format!(
            "Progress: {} / {}",
            progress.completed, progress.total
        ))
        .width(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center);

        let bytes_text = if let Some(bytes) = self.total_bytes_copied {
            text(format!("Total Bytes Copied: {}", human_bytes(bytes as f64)))
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center)
        } else {
            text("")
        };

        let checksum_button = button(text("Verify Checksum").size(20))
            .width(Length::Fill)
            .padding(15);

        let checksum_button = if completed {
            checksum_button.on_press(LibreCardMessage::StartChecksum)
        } else {
            checksum_button
        };

        column![
            title,
            progress_bar,
            progress_text,
            bytes_text,
            checksum_button,
        ]
        .spacing(20)
        .padding(20)
        .width(Length::Fill)
        .into()
    }

    fn view_checksum_stage(&self, progress: &Progress) -> Element<LibreCardMessage> {
        let title = text("Verifying File Integrity")
            .size(28)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center);

        let progress_value = if progress.total == 0 {
            0.0
        } else {
            progress.completed as f32 / progress.total as f32
        };

        let progress_bar = progress_bar(0.0..=1.0, progress_value)
            .width(Length::Fill)
            .height(30);

        let progress_text = text(format!(
            "Progress: {} / {}",
            progress.completed, progress.total
        ))
        .width(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center);

        column![title, progress_bar, progress_text,]
            .spacing(20)
            .padding(20)
            .width(Length::Fill)
            .into()
    }

    fn view_checksum_complete_stage(&self, report: &ChecksumReport) -> Element<LibreCardMessage> {
        let title = text("Checksum Verification Complete")
            .size(28)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center);

        let error_count = report.count_errors();
        let total_files = report.total_files();

        let (status_message, status_color) = if error_count == 0 {
            (
                format!("All {} files verified successfully!", total_files),
                Color::from_rgb(0.0, 0.7, 0.0),
            )
        } else {
            (
                format!(
                    "WARNING: {} out of {} files failed verification!",
                    error_count, total_files
                ),
                Color::from_rgb(0.9, 0.0, 0.0),
            )
        };

        let status_text = text(status_message)
            .width(Length::Fill)
            .size(16)
            .color(status_color)
            .align_x(iced::alignment::Horizontal::Center);

        let export_button = button(text("Export Checksum Report").size(20))
            .on_press(LibreCardMessage::ExportChecksum)
            .width(Length::Fill)
            .padding(15);

        column![title, status_text, export_button,]
            .spacing(20)
            .padding(20)
            .width(Length::Fill)
            .into()
    }
}
