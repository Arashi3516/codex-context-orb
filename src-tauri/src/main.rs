#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assessments;
mod evidence;
mod diagnostics;
mod telemetry;
mod magnet;
mod magnet_geometry;
mod magnet_platform;

use tauri::Manager;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--inspect-session") {
        if args.len() != 3 {
            eprintln!("Usage: codex-context-orb --inspect-session <exact-session-id>");
            std::process::exit(2);
        }
        match diagnostics::inspect(&args[2]) {
            Ok(report) => println!("{}", report),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(2);
            }
        }
        return;
    }
    tauri::Builder::default()
        .manage(magnet::Magnet::default())
        .invoke_handler(tauri::generate_handler![telemetry::read_hook_events, assessments::read_semantic_assessments, evidence::read_evidence_reports, evidence::read_evidence_history, magnet::get_magnet_state, magnet::set_magnet_preferences, magnet::begin_magnetic_drag, magnet::end_magnetic_drag, magnet::resize_orb_window])
        .setup(|app| {
            if let Some(window) = app.get_webview_window("orb") {
                magnet::start(window.clone());
                window.show()?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("could not start Context Orb");
}
