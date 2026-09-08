mod output;
mod overlay;
mod shortcuts;
mod state;
mod tray;

use shortcuts::{register_shortcuts, Shortcuts};
use state::AppState;
use tauri_plugin_global_shortcut::ShortcutState;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    if let Some(mode) = Shortcuts::default().mode_for_parsed(shortcut) {
                        tray::request_capture(app, mode);
                    }
                })
                .build(),
        )
        .manage(AppState::new())
        .setup(|app| {
            let handle = app.handle().clone();
            tray::build_tray(&handle)?;
            if let Err(err) = register_shortcuts(&handle, &Shortcuts::default()) {
                eprintln!("failed to register shortcuts: {err}");
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running snapdeck");
}
