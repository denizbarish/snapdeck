//! The settings window.
//!
//! An ordinary window, like the editor next door and unlike the overlay: a title
//! bar, a place in the window list, and nothing to do with a capture. It is the
//! only window this application opens that the user asks for by name.
//!
//! Exactly one at a time. A second `Settings…` raises the one that is already
//! open rather than building another, because two of them would disagree the
//! moment one of them saved: each holds the settings it loaded, and the second
//! to save would quietly put back what the first had just changed.
//!
//! The window itself is deliberately not remembered anywhere.
//! `get_webview_window` is Tauri's own live answer to "is it open", so there is
//! no handle to leak and no way for this module's idea of the window to survive
//! the window itself.
//!
//! Its window-server id is remembered, which is a different thing: a number
//! that the capture picker needs and that can only be read on the main thread
//! while the window is alive. It is dropped again on `Destroyed`, because the
//! server reuses those numbers and a stale one would quietly hide somebody
//! else's window from the picker.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

// The window server's own id, which is what `SCWindow.windowID` reports and
// therefore what the capture picker matches against. Defined by the overlay
// because the overlay needed it first, and borrowed here by the editor's own
// route, for the same reason: one piece of unsafe code doing one thing.
use crate::overlay::overlay_window_id as window_server_id;
use crate::{report::report_failure, state::AppState};

/// The window's label, which is also what `capabilities/settings.json` grants
/// its permissions to.
const SETTINGS_LABEL: &str = "settings";

/// The window's size in points.
///
/// Tall rather than wide: every control is a labelled row, and a wider window
/// would only put more distance between a label and the control it names. Not
/// resizable below this, because the shortcut recorder's three rows and the
/// folder path have nowhere to wrap to.
const SETTINGS_WIDTH: f64 = 460.0;
const SETTINGS_HEIGHT: f64 = 620.0;
const MIN_SETTINGS_WIDTH: f64 = 400.0;
const MIN_SETTINGS_HEIGHT: f64 = 420.0;

/// Opens the settings window, or brings the open one to the front.
///
/// Main thread only, which the tray's menu event handler already is.
///
/// A failure is reported rather than returned: the caller is a menu item, which
/// has nowhere to put an error, and a `Settings…` that does nothing at all is
/// the exact complaint that took this item out of the menu in the first place.
pub fn open_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        // All three, because the window can be behind, minimised, or hidden,
        // and only the last of them is what the user sees as "not open".
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    if let Err(err) = build(app) {
        report_failure(
            app,
            &format!("Snapdeck could not open the settings window. (Details: {err})"),
        );
    }
}

fn build(app: &AppHandle) -> tauri::Result<()> {
    let window =
        WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("settings.html".into()))
            .title("Snapdeck Settings")
            .inner_size(SETTINGS_WIDTH, SETTINGS_HEIGHT)
            .min_inner_size(MIN_SETTINGS_WIDTH, MIN_SETTINGS_HEIGHT)
            .resizable(true)
            .center()
            // Orders the window to the front of this application's own windows, and is
            // not enough on its own: Snapdeck is a menu bar agent that is never the
            // active application, so the page asks for focus once it is running. The
            // editor learnt the same thing, and its comment says why AppKit ignores a
            // request made before the window has been composited.
            .focused(true)
            .build()?;

    // Recorded here, on the main thread the id has to be read from and while
    // the window is certainly alive, exactly as `editor::build_editor_window`
    // records an editor's. Without it the capture picker offers the settings
    // window, and offers it first: it is frontmost for as long as the user is
    // looking at it, which is the whole time window mode could be started from
    // it.
    let state = app.state::<AppState>();
    state.set_settings_window_id(window_server_id(&window));

    // `Destroyed` rather than the close button, because the title bar's red
    // button never reaches Rust otherwise. A stale id would exclude whatever
    // window the server hands the number to next, which is a window the user
    // could no longer photograph and no way to work out why.
    let handle = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            handle.state::<AppState>().set_settings_window_id(None);
        }
    });
    Ok(())
}
