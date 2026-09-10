pub mod bridge;
mod commands;
mod editor;
mod output;
mod overlay;
mod recents;
mod report;
mod settings;
mod settings_window;
mod shortcuts;
mod state;
mod tray;
mod update_history;
mod updater;

use settings::Settings;
use shortcuts::{register_shortcuts, Shortcuts};
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
        // The only plugin here that can reach the network, and it does so only
        // when something calls it. Built by `updater::plugin` rather than by
        // `tauri_plugin_updater::Builder::new()` so that the rule for what
        // counts as a newer release is written once, in `updater::is_upgrade`.
        // No window is granted its commands: every check is started from Rust.
        .plugin(updater::plugin())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    // The bindings actually registered, not the built-in ones
                    // and not the settings file's. Reading the defaults here
                    // was what made a rebind only half work: the new
                    // combination registered, fired this handler, matched
                    // nothing, and did exactly nothing. The settings are the
                    // same trap one step further along: a stored combination
                    // that could not be registered stays in the file for the
                    // user to change, while something else is bound, and only
                    // the thing that is bound can be the thing that fired.
                    let Some(shortcuts) = app.state::<AppState>().registered_shortcuts() else {
                        return;
                    };
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
            let (settings, bound) = adopt(&handle);
            let state = handle.state::<AppState>();
            state.set_settings(settings);
            state.set_registered_shortcuts(bound);
            // After the tray exists, because this fills the submenu the tray
            // just built, and before anything can be captured, so the first
            // capture of the run is added to the list that was on disk rather
            // than to an empty one.
            recents::restore(&handle);
            // After the settings are in force, because this is the one thing
            // in the application that reads a setting to decide whether it may
            // happen at all: it does nothing unless the user has turned it on.
            updater::check_at_launch(&handle);
            // Last, because it is the one thing here that can fail without the
            // launch failing with it, and it has to be able to report that.
            open_bridge(&handle);
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

/// Opens the loopback bridge the browser extension connects to, or tells the
/// user why it could not be opened.
///
/// A bridge that cannot listen is not a reason to fail the launch. The port is
/// a fixed one and anything on this machine may already hold it, and every
/// other way of taking a screenshot still works without a bridge; what the user
/// must not get is silence, because from the browser's side a bridge that is
/// not running and one that refused them look the same.
///
/// The server is handed to `AppState` rather than dropped here: dropping a
/// `BridgeServer` stops it, so a listener that nothing holds would close on the
/// next line.
fn open_bridge(app: &tauri::AppHandle) {
    let token = match bridge::token::generate_token() {
        Ok(token) => token,
        Err(err) => {
            report::report_failure(
                app,
                &format!("the browser extension bridge has no pairing token, so it was not started: {err}"),
            );
            return;
        }
    };

    let info = bridge::protocol::AppInfo {
        name: app.package_info().name.clone(),
        version: app.package_info().version.to_string(),
    };
    let policy = std::sync::Arc::new(bridge::intake::AppPolicy::new(app.clone(), token, info));

    match bridge::server::BridgeServer::start(
        bridge::protocol::BRIDGE_PORT,
        policy,
        bridge::server::BridgeLimits::default(),
    ) {
        Ok(server) => app.state::<AppState>().set_bridge_server(server),
        Err(err) => report::report_failure(app, &err),
    }
}

/// Puts the stored settings into force at launch, and answers with both the
/// settings and the bindings that were actually registered.
///
/// Two things can disagree with the file by the time it is read.
///
/// A stored binding may no longer be registerable: another application can have
/// taken it since it was chosen. Falling back to the built-in bindings is the
/// only outcome that leaves the user with working keys, and it is reported,
/// because a shortcut that silently became a different shortcut is worse than
/// one that plainly stopped working.
///
/// What it deliberately does not do is write the fallback into
/// `settings.shortcuts`. That was a way to lose the user's choice without
/// telling them: the settings then held the built-in bindings, the next save of
/// any unrelated field wrote those over the stored combination, and the window
/// meanwhile rendered them as though they were working. The file keeps saying
/// what the user chose, the second half of this pair says what is bound, and
/// the settings window is given both.
///
/// The login item may have been removed from outside this application. The
/// system is the authority on whether it exists, so its answer is adopted here;
/// a checkbox that insists it is ticked while the plist is gone is a lie the
/// settings window has no way to catch.
fn adopt(app: &tauri::AppHandle) -> (Settings, Option<Shortcuts>) {
    let mut settings = settings::load(app);
    let (bound, complaint) = adopt_shortcuts(
        |shortcuts| register_shortcuts(app, shortcuts),
        &settings.shortcuts,
        &Settings::default().shortcuts,
    );
    if let Some(complaint) = complaint {
        report::report_failure(app, &complaint);
    }
    if let Some(enabled) = settings::launch_at_login_state(app) {
        settings.launch_at_login = enabled;
    }
    (settings, bound)
}

/// The decision inside `adopt`, with the registration injected.
///
/// Split out for the reason `shortcuts::rebind_with` is: the case worth testing,
/// a stored combination the platform refuses, is not something a test can
/// arrange through a live `GlobalShortcut`.
///
/// Answers with what is bound and with what the user is owed an explanation
/// about. `None` for the first is a real answer: nothing is registered, and the
/// settings window has to be able to say so rather than render three keys that
/// do nothing.
fn adopt_shortcuts<F>(
    mut register: F,
    stored: &Shortcuts,
    defaults: &Shortcuts,
) -> (Option<Shortcuts>, Option<String>)
where
    F: FnMut(&Shortcuts) -> Result<(), String>,
{
    let Err(err) = register(stored) else {
        return (Some(stored.clone()), None);
    };
    // Already the built-in set, so there is no fallback left to try and no
    // point in saying it was tried.
    if stored == defaults {
        return (
            None,
            Some(format!(
                "Snapdeck could not register any capture shortcut ({err}). Use the menu bar item to take a capture."
            )),
        );
    }
    match register(defaults) {
        Ok(()) => (
            Some(defaults.clone()),
            Some(format!(
                "Snapdeck could not use your capture shortcuts ({err}), so the built-in ones are bound instead. Your choice is still in Settings; open it to pick another combination."
            )),
        ),
        Err(fallback_err) => (
            None,
            Some(format!(
                "Snapdeck could not register any capture shortcut ({err}, and then {fallback_err}). Use the menu bar item to take a capture."
            )),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> Shortcuts {
        Settings::default().shortcuts
    }

    fn stored() -> Shortcuts {
        Shortcuts {
            capture_region: "CmdOrCtrl+Alt+Shift+KeyR".to_string(),
            ..defaults()
        }
    }

    #[test]
    fn a_stored_set_that_registers_is_what_is_bound() {
        let (bound, complaint) = adopt_shortcuts(|_| Ok(()), &stored(), &defaults());
        assert_eq!(bound, Some(stored()));
        assert_eq!(complaint, None);
    }

    /// The launch-path failure this pair of tests exists for. The built-in set
    /// takes over the keyboard, and the user's own choice stays exactly where
    /// it was, so the next save cannot write the fallback over it.
    #[test]
    fn a_stored_set_the_platform_refuses_falls_back_without_becoming_the_setting() {
        let stored = stored();
        let (bound, complaint) = adopt_shortcuts(
            |shortcuts| {
                if *shortcuts == stored {
                    Err("the region shortcut is already taken".to_string())
                } else {
                    Ok(())
                }
            },
            &stored,
            &defaults(),
        );
        assert_eq!(
            bound,
            Some(defaults()),
            "the built-in bindings are what the keyboard has"
        );
        let complaint = complaint.expect("a shortcut that changed under the user has to be said");
        assert!(complaint.contains("already taken"), "{complaint}");
        assert!(
            complaint.contains("still in Settings"),
            "the user has to be told their choice was kept: {complaint}"
        );
    }

    /// The false "in force" case. When nothing registers, nothing may claim to
    /// be bound: `None` is what stops the settings window rendering three keys
    /// that do nothing and answering a save with "the new settings are in force
    /// now".
    #[test]
    fn nothing_is_claimed_as_bound_when_nothing_registers() {
        let (bound, complaint) = adopt_shortcuts(
            |_| Err("the manager is gone".to_string()),
            &stored(),
            &defaults(),
        );
        assert_eq!(bound, None);
        let complaint = complaint.expect("an empty keyboard has to be said out loud");
        assert!(
            complaint.contains("Use the menu bar item"),
            "and it has to name the way out: {complaint}"
        );
    }

    /// A stored set that is already the built-in one has no fallback, and the
    /// message must not pretend a second attempt happened.
    #[test]
    fn the_built_in_set_failing_is_reported_once() {
        let (bound, complaint) = adopt_shortcuts(
            |_| Err("the manager is gone".to_string()),
            &defaults(),
            &defaults(),
        );
        assert_eq!(bound, None);
        let complaint = complaint.expect("an empty keyboard has to be said out loud");
        assert!(
            !complaint.contains("and then"),
            "there was only one attempt: {complaint}"
        );
    }
}
