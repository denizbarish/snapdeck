//! The one place a failure the user could not otherwise notice is reported.
//!
//! Snapdeck is an `LSUIElement` binary: no Dock icon, no window declared, and no
//! console in a release build. An `eprintln!` on the capture path therefore
//! reaches nobody, which turns every failure below the trigger into "the
//! shortcut did nothing". The design spec asks for two surfaces for a failed
//! capture, a line in a log file and a message to the user, and both are raised
//! from one call so that no failure site has to remember either of them.
//!
//! The message used to be a macOS user notification and is now the menu bar
//! item, because the notification never arrived. `tauri-plugin-notification`
//! 2.4.0 delivers on macOS through `notify-rust`, which drives
//! `NSUserNotificationCenter`, the API Apple deprecated in macOS 11. Measured on
//! macOS 26.5.2 from a bundle Notification Center had accepted: that path
//! returns `Ok` and puts nothing on screen. It cannot report the silence
//! either, because the plugin's `show()` spawns the send and drops its result,
//! and its `request_permission()` is a desktop stub returning `Granted` without
//! asking macOS anything, so there was never a permission to request nor an
//! error to log.
//!
//! Notifications are not categorically impossible here, and it is worth being
//! precise about why they were dropped rather than fixed. What decides whether
//! `UNUserNotificationCenter` will register an application is where the bundle
//! lives, not how it is signed: measured, an ad-hoc bundle and an Apple
//! Development signed one behave identically, both refused with "Notifications
//! are not allowed for this application" when run out of a build directory and
//! both accepted from `~/Applications`. Reaching a real banner would therefore
//! mean dropping the plugin for a direct `UNUserNotificationCenter` binding,
//! carrying a permission prompt and a denied state, and still showing nothing
//! whenever the application runs from where it was built, which is where it
//! runs during development and every verification run. The menu bar item costs
//! none of that: this application owns it, no permission gates it, it works
//! from any path, and it keeps saying so until the next capture instead of for
//! five seconds. See `tray::show_failure`.
//!
//! Nothing here reports its own failure to the caller, and nothing here panics.
//! It runs on the capture worker, inside the recovery of a capture that already
//! panicked, and on the main thread's event loop, and none of those has anywhere
//! to send a failed report. The two halves are independent on purpose: a tray
//! that has not been built yet still leaves the log line behind, and a log
//! directory that cannot be created still leaves the tray marker.

use std::{io::Write, path::PathBuf};

use tauri::{AppHandle, Manager};

use crate::output::OffsetDateTimeParts;

/// The log file inside `app_log_dir()`.
///
/// Appended to, never rotated: a line is written only when a capture fails, so
/// the file grows at the rate the user hits bugs rather than at the rate they
/// take screenshots.
const LOG_FILE_NAME: &str = "snapdeck.log";

/// Tells the user that something failed, in both places they can see it.
///
/// `message` is addressed to the user and should say what to do about it, not
/// only what went wrong: it is rendered verbatim in the tray tooltip, where
/// there is no room for a second attempt at explaining.
pub fn report_failure(app: &AppHandle, message: &str) {
    // Kept, because `pnpm tauri dev` does run with a terminal attached and this
    // is the cheapest place to read a failure while developing. It is no longer
    // the only place.
    eprintln!("snapdeck: {message}");
    append_to_log(app, message);
    crate::tray::show_failure(app, message);
}

/// Where the log lives, whether or not it has been written yet.
///
/// The tray's Open Log item needs the same path this module writes to, and one
/// of the two of them has to own it.
pub fn log_file_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_log_dir()
        .ok()
        .map(|directory| directory.join(LOG_FILE_NAME))
}

/// Appends one timestamped line to the log file, creating it if needed.
///
/// Local wall clock, from the same clock the filenames use, because a user
/// reading their own log to say when something failed is thinking in the time
/// their screen was showing.
fn append_to_log(app: &AppHandle, message: &str) {
    let Some(path) = log_file_path(app) else {
        return;
    };
    let Some(directory) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(directory).is_err() {
        return;
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    let at = OffsetDateTimeParts::now();
    let _ = writeln!(
        file,
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {message}",
        at.year, at.month, at.day, at.hour, at.minute, at.second
    );
}
