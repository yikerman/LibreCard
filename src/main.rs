#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use crate::gui::LibreCardApp;

mod backend;
mod gui;

fn main() -> iced::Result {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create Tokio runtime");

    let _guard = runtime.enter();

    iced::application("LibreCard", LibreCardApp::update, LibreCardApp::view)
        .subscription(LibreCardApp::subscription)
        .run()
}