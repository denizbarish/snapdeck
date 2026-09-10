mod commands;
mod editor;
mod output;
mod overlay;
mod report;
mod settings;
mod settings_window;
mod shortcuts;
mod state;
mod tray;

use settings::Settings;
use shortcuts::register_shortcuts;
use state::AppState;
use tauri::{Manager, RunEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::ShortcutState;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        // Driven from Rust only: the folder picker is opened by
        // `commands::choose_save_directory`, so no webview is granted a dialog
        // permission and no window can put a file picker on screen by itself.
        .plugin(tauri_plugin_dialog::init())
        // `LaunchAgent` rather than `AppleScript`. The AppleScript path adds a
        // real Login Items entry by driving System Events, which costs the user
        // an Automation consent prompt the first time they tick a checkbox, and
        // fails outright if they refuse it. The launch agent is a plist this
        // application owns, needs no consent, and is what macOS lists under
        // Login Items > Allow in the Background.
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    // The bindings in force, not the built-in ones. Reading the
                    // defaults here was what made a rebind only half work: the
                    // new combination registered, fired this handler, matched
                    // nothing, and did exactly nothing.
                    let shortcuts = app.state::<AppState>().settings().shortcuts;
                    if let Some(mode) = shortcuts.mode_for_parsed(shortcut) {
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
            commands::capture_region,
            commands::save_edited,
            commands::copy_edited,
            commands::close_editor,
            commands::get_settings,
            commands::save_settings,
            commands::choose_save_directory
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            tray::build_tray(&handle)?;
            handle.state::<AppState>().set_settings(adopt(&handle));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building snapdeck")
        .run(|app, event| match event {
            // Closing the last overlay must not end the process. Tauri requests
            // an exit once no window is left, and this is a menu bar agent that
            // has no windows between captures, so without this the app dies the
            // moment a capture is repeated or Escape closes the overlays.
            // `code` is `Some` only for a programmatic exit, which is what the
            // tray's Quit item does, so quitting still works.
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

/// Puts the stored settings into force at launch, and answers with what is
/// actually in force rather than with what was on disk.
///
/// Two things can disagree with the file by the time it is read.
///
/// A stored binding may no longer be registerable: another application can have
/// taken it since it was chosen. Falling back to the built-in bindings is the
/// only outcome that leaves the user with working keys, and it is reported,
/// because a shortcut that silently became a different shortcut is worse than
/// one that plainly stopped working.
///
/// The login item may have been removed from outside this application. The
/// system is the authority on whether it exists, so its answer is adopted here;
/// a checkbox that insists it is ticked while the plist is gone is a lie the
/// settings window has no way to catch.
fn adopt(app: &tauri::AppHandle) -> Settings {
    let mut settings = settings::load(app);
    if let Err(err) = register_shortcuts(app, &settings.shortcuts) {
        let defaults = Settings::default();
        settings.shortcuts = defaults.shortcuts;
        match register_shortcuts(app, &settings.shortcuts) {
            Ok(()) => report::report_failure(
                app,
                &format!("Snapdeck could not use your capture shortcuts ({err}), so it went back to the built-in ones. Open Settings to choose another combination."),
            ),
            Err(fallback_err) => report::report_failure(
                app,
                &format!("Snapdeck could not register any capture shortcut ({err}, and then {fallback_err}). Use the menu bar item to take a capture."),
            ),
        }
    }
    if let Some(enabled) = settings::launch_at_login_state(app) {
        settings.launch_at_login = enabled;
    }
    settings
}
