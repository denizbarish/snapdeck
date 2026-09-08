mod output;
mod overlay;
mod shortcuts;
mod state;
mod tray;

use shortcuts::{register_shortcuts, Shortcuts};
use state::AppState;
use tauri::RunEvent;
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
        .build(tauri::generate_context!())
        .expect("error while building snapdeck")
        .run(|_app, event| {
            // Closing the last overlay must not end the process. Tauri requests
            // an exit once no window is left, and this is a menu bar agent that
            // has no windows between captures, so without this the app dies the
            // moment a capture is repeated or Task 7's Escape closes the
            // overlays. `code` is `Some` only for a programmatic exit, which is
            // what the tray's Quit item does, so quitting still works.
            if let RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
            }
        });
}
