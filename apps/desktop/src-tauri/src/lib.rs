mod commands;
mod output;
mod overlay;
mod report;
mod shortcuts;
mod state;
mod tray;

use shortcuts::{register_shortcuts, Shortcuts};
use state::AppState;
use tauri::RunEvent;
use tauri_plugin_global_shortcut::ShortcutState;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        // The failure surface for a menu bar app with no window of its own; see
        // `report::report_failure`.
        .plugin(tauri_plugin_notification::init())
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
        .invoke_handler(tauri::generate_handler![
            commands::permission_state,
            commands::request_permission,
            commands::close_overlays,
            commands::list_windows,
            commands::capture_region
        ])
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
        .run(|app, event| match event {
            // Closing the last overlay must not end the process. Tauri requests
            // an exit once no window is left, and this is a menu bar agent that
            // has no windows between captures, so without this the app dies the
            // moment a capture is repeated or Task 7's Escape closes the
            // overlays. `code` is `Some` only for a programmatic exit, which is
            // what the tray's Quit item does, so quitting still works.
            RunEvent::ExitRequested {
                code: None, api, ..
            } => api.prevent_exit(),
            // The frozen frames are full-resolution, lossless copies of
            // everything that was on the user's screen. Nothing else reaches
            // them once the app is closing: the only other cleanup runs on the
            // next capture, and there is no next capture. Without this, one
            // capture and a quit leaves them in `~/Library/Caches` until the app
            // is run again.
            RunEvent::Exit => overlay::discard_cached_frozen_frames(app),
            _ => {}
        });
}
