#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assessments;
mod telemetry;

use tauri::{Manager, PhysicalPosition};

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![telemetry::read_hook_events, assessments::read_semantic_assessments])
        .setup(|app| {
            if let Some(window) = app.get_webview_window("orb") {
                if let Some(monitor) = window.current_monitor()? {
                    let scale = monitor.scale_factor();
                    let position = monitor.position();
                    let size = monitor.size();
                    window.set_position(PhysicalPosition::new(
                        position.x + size.width as i32 - (120.0 * scale) as i32,
                        position.y + size.height as i32 - (160.0 * scale) as i32,
                    ))?;
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("could not start Context Orb");
}
