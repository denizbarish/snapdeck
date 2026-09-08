//! The one place a failure the user could not otherwise notice is reported.
//!
//! Snapdeck is an `LSUIElement` binary: no Dock icon, no window declared, and no
//! console in a release build. An `eprintln!` on the capture path therefore
//! reaches nobody, which turns every failure below the trigger into "the
//! shortcut did nothing". The design spec asks for two surfaces for a failed
//! capture, a line in a log file and a message to the user, and both are raised
//! from one call so that no failure site has to remember either of them.
//!
//! Nothing here reports its own failure to the caller, and nothing here panics.
//! It runs on the capture worker, inside the recovery of a capture that already
//! panicked, and on the main thread's event loop, and none of those has anywhere
//! to send a failed report. The two halves are independent on purpose: a
//! notification macOS refuses still leaves the log line behind, and a log
//! directory that cannot be created still leaves the notification.

use std::io::Write;

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::output::OffsetDateTimeParts;

/// Title of every notification Snapdeck raises. The body carries the detail.
const NOTIFICATION_TITLE: &str = "Snapdeck";

/// The log file inside `app_log_dir()`.
///
/// Appended to, never rotated: a line is written only when a capture fails, so
/// the file grows at the rate the user hits bugs rather than at the rate they
/// take screenshots.
const LOG_FILE_NAME: &str = "snapdeck.log";

/// Tells the user that something failed, in both places they can see it.
///
/// `message` is addressed to the user and should say what to do about it, not
/// only what went wrong: it is rendered verbatim in a notification, where there
/// is no room for a second attempt at explaining.
pub fn report_failure(app: &AppHandle, message: &str) {
    // Kept, because `pnpm tauri dev` does run with a terminal attached and this
    // is the cheapest place to read a failure while developing. It is no longer
    // the only place.
    eprintln!("snapdeck: {message}");
    append_to_log(app, message);
    notify(app, message);
}

/// Appends one timestamped line to the log file, creating it if needed.
///
/// Local wall clock, from the same clock the filenames use, because a user
/// reading their own log to say when something failed is thinking in the time
/// their screen was showing.
fn append_to_log(app: &AppHandle, message: &str) {
    let Ok(directory) = app.path().app_log_dir() else {
        return;
    };
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join(LOG_FILE_NAME))
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

/// Raises the macOS user notification.
///
/// A refusal is printed rather than reported onwards: the surface that would
/// carry that report is the one that just failed.
fn notify(app: &AppHandle, message: &str) {
    if let Err(err) = app
        .notification()
        .builder()
        .title(NOTIFICATION_TITLE)
        .body(message)
        .show()
    {
        eprintln!("snapdeck: the notification for \"{message}\" was refused: {err}");
    }
}
