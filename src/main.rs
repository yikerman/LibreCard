#![windows_subsystem = "windows"]

use eframe::egui;
use eframe::egui::{FontData, Pos2, Vec2};
use eframe::epaint::text::{FontInsert, InsertFontFamily};
use gui::LibreCardApp;

mod backend;
mod gui;

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder {
            inner_size: Some(Vec2 {
                x: 1600.0,
                y: 900.0,
            }),
            min_inner_size: Some(Vec2 {
                x: 1200.0,
                y: 600.0,
            }),
            position: Some(Pos2 { x: 0.0, y: 0.0 }),
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "LibreCard",
        options,
        Box::new(|creation_context| {
            creation_context.egui_ctx.set_zoom_factor(1.5);

            creation_context.egui_ctx.add_font(FontInsert::new(
                "Source Han Sans SC",
                FontData::from_static(include_bytes!("../static/SourceHanSansSC-Regular.otf")),
                vec![
                    InsertFontFamily {
                        family: egui::FontFamily::Proportional,
                        priority: egui::epaint::text::FontPriority::Highest,
                    },
                    InsertFontFamily {
                        family: egui::FontFamily::Monospace,
                        priority: egui::epaint::text::FontPriority::Lowest,
                    },
                ],
            ));

            Ok(Box::new(LibreCardApp::default()))
        }),
    )
}
