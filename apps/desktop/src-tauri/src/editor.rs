//! The editor window.
//!
//! An ordinary window, and deliberately nothing like the overlay next door: a
//! title bar, a resize grip and a place in the window list, because the capture
//! is already finished by the time this opens. The file has been written and
//! the clipboard holds the image, so this window is an offer rather than a
//! step. Closing it without touching anything loses nothing.
//!
//! The picture reaches the page over the asset protocol, exactly as the
//! overlay's frozen frame does: the absolute path travels in the URL and the
//! webview reads the file itself. Nothing here moves pixels over IPC, which
//! matters more for the editor than it did for the overlay, because this file
//! is the full-resolution capture rather than a throwaway backdrop.
//!
//! One editor at a time. The label carries a sequence number so that a capture
//! taken while an editor is open can build its window without waiting for the
//! previous one to leave the window map: closing is a request that the event
//! loop honours later, and a label is only free once `Destroyed` has arrived.
//! `close_editor` therefore closes every `editor-` window, the way
//! `overlay::close_overlays` closes every overlay, and at most one of them is
//! ever an editor the user can still see.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use percent_encoding::utf8_percent_encode;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::{overlay::PATH_QUERY_ENCODE_SET, report::report_failure};

const EDITOR_LABEL_PREFIX: &str = "editor-";

/// Vertical room for the toolbar above the picture, in points.
///
/// An allowance rather than a measurement: the toolbar is laid out by the
/// editor's own CSS and wraps when the window is narrow, so Rust cannot know
/// its height. Being wrong here costs a little letterboxing or a picture fitted
/// a few percent below 1:1, never a broken window.
const TOOLBAR_ALLOWANCE: f64 = 56.0;

/// The narrowest and shortest an editor window may open.
///
/// The one deliberate exception to "never larger than the image": a capture of
/// a 200-point dialog would otherwise open a window too narrow for its own
/// toolbar, which wraps onto three rows and leaves less room for the picture
/// than the floor does. The picture is still never enlarged past 1:1, because
/// the editor's own viewport caps it there.
const MIN_EDITOR_WIDTH: f64 = 720.0;
const MIN_EDITOR_HEIGHT: f64 = 260.0;

/// How much of the monitor's work area a new editor window may take.
///
/// A full-screen capture is exactly the size of the display, so an unclamped
/// window would open larger than the space it has to live in and put its own
/// title bar under the menu bar.
const MAX_WORK_AREA_FRACTION: f64 = 0.9;

/// Source of editor window labels, unique for the life of the process.
static NEXT_EDITOR_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn next_editor_label() -> String {
    let sequence = NEXT_EDITOR_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{EDITOR_LABEL_PREFIX}{sequence}")
}

/// The editor's URL. The capture's absolute path travels with it so the page
/// only has to call `convertFileSrc`, and the size travels with it so the
/// editor can lay out its document before the image has finished decoding.
pub fn editor_url(path: &Path, width: u32, height: u32) -> String {
    let raw_path = path.to_string_lossy();
    let path = utf8_percent_encode(&raw_path, PATH_QUERY_ENCODE_SET);
    format!("editor.html?path={path}&width={width}&height={height}")
}

/// Opens the editor on a capture that has already been saved.
///
/// Callable from the blocking capture worker, which is where it is called
/// from: window creation is hopped onto the main thread, the only thread macOS
/// lets create one, and this returns as soon as the work is handed off.
///
/// A failure is reported rather than returned. There is no caller left to
/// answer to by the time the window is built, and the user has to know that
/// the editor they were expecting is not coming; the capture itself is safe,
/// which is what the message says.
pub fn open_editor(app: &AppHandle, path: &Path, width: u32, height: u32) {
    let handle = app.clone();
    // Owned, because the closure outlives this call: it runs on the next turn
    // of the main thread's event loop, by which time the capture worker's
    // borrow is long gone.
    let owned = path.to_path_buf();
    if let Err(err) = app.run_on_main_thread(move || {
        // The previous editor first. It is a request the event loop honours
        // later, which is why the new window gets a label of its own rather
        // than reusing this one's.
        close_editor(&handle);
        if let Err(err) = build_editor_window(&handle, &owned, width, height) {
            report_failure(
                &handle,
                &format!(
                    "Snapdeck saved the capture to {} and copied it to the clipboard, but could not open the editor. (Details: {err})",
                    owned.display()
                ),
            );
        }
    }) {
        report_failure(
            app,
            &format!(
                "Snapdeck saved the capture to {} and copied it to the clipboard, but could not reach the main thread to open the editor. (Details: {err})",
                path.display()
            ),
        );
    }
}

/// Closes every editor window.
///
/// Every one, not the newest, for the same reason `overlay::close_overlays`
/// does: a window that has been asked to close still holds its label until the
/// event loop destroys it, so "the editor" is briefly two windows and only one
/// of them is on screen.
///
/// Main thread only, which the synchronous `commands::close_editor` and
/// `open_editor`'s own hop both already are.
pub fn close_editor(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(EDITOR_LABEL_PREFIX) {
            let _ = window.close();
        }
    }
}

/// Main thread only: macOS requires window creation there.
fn build_editor_window(
    app: &AppHandle,
    path: &Path,
    width: u32,
    height: u32,
) -> tauri::Result<WebviewWindow> {
    let (window_width, window_height) = editor_window_size(app, width, height);
    WebviewWindowBuilder::new(
        app,
        next_editor_label(),
        WebviewUrl::App(editor_url(path, width, height).into()),
    )
    // The file's own name, because that is what the window is: one saved
    // capture, and the thing the user will look for in the window list when a
    // second one is open.
    .title(window_title(path))
    .inner_size(window_width, window_height)
    .min_inner_size(
        MIN_EDITOR_WIDTH.min(window_width),
        MIN_EDITOR_HEIGHT.min(window_height),
    )
    .resizable(true)
    .center()
    // Orders the window to the front of this application's own windows. It is
    // not enough on its own and is not meant to be: Snapdeck is a menu bar
    // agent that is never the active application, so a window at the front of
    // its own stack still opens behind whatever the user was looking at. The
    // page asks for focus once it is running, which is the only moment AppKit
    // honours the request; asking here, before the window has been composited,
    // was measured to do nothing at all. The overlay learnt the same thing.
    .focused(true)
    .build()
}

/// The window's inner size in points, from the capture's size in pixels.
///
/// Points, not pixels: `inner_size` is logical, and the capture was taken at
/// the display's own density, so a retina screenshot is half its pixel size on
/// screen. The primary monitor's scale is the one used because `center()`
/// places the window there.
///
/// Never larger than the picture plus its toolbar, and never larger than the
/// space available. The editor's viewport caps the picture at 1:1, so a window
/// bigger than the capture would only add empty stage around it.
fn editor_window_size(app: &AppHandle, width: u32, height: u32) -> (f64, f64) {
    let monitor = app.primary_monitor().ok().flatten();
    let scale = monitor
        .as_ref()
        .map_or(1.0, |monitor| monitor.scale_factor());
    // A monitor that cannot be read leaves the picture as the only bound,
    // which is the same answer as a monitor large enough not to bite.
    let (available_width, available_height) =
        monitor
            .as_ref()
            .map_or((f64::INFINITY, f64::INFINITY), |monitor| {
                let area = monitor.work_area();
                (
                    f64::from(area.size.width) / scale * MAX_WORK_AREA_FRACTION,
                    f64::from(area.size.height) / scale * MAX_WORK_AREA_FRACTION,
                )
            });

    let picture_width = f64::from(width) / scale;
    let picture_height = f64::from(height) / scale;
    (
        picture_width
            .max(MIN_EDITOR_WIDTH)
            .min(available_width)
            .max(1.0),
        (picture_height + TOOLBAR_ALLOWANCE)
            .max(MIN_EDITOR_HEIGHT)
            .min(available_height)
            .max(1.0),
    )
}

/// What the title bar says: the capture's file name, or a fallback for a path
/// that has none, which no capture produces and a malformed one might.
fn window_title(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Snapdeck".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_url_carries_the_path_and_the_capture_size() {
        let url = editor_url(Path::new("/Users/a/Pictures/Snapdeck.png"), 1200, 800);
        assert_eq!(
            url,
            "editor.html?path=/Users/a/Pictures/Snapdeck.png&width=1200&height=800"
        );
    }

    /// The same escaping the overlay's frame path gets, and for the same
    /// reason: a space or an ampersand in a picture's name would otherwise end
    /// the query value and the page would open on a truncated path.
    #[test]
    fn editor_url_escapes_characters_that_would_end_the_query() {
        let url = editor_url(Path::new("/Users/a/My Pictures/A&B.png"), 10, 10);
        assert!(
            url.starts_with("editor.html?path=/Users/a/My%20Pictures/A%26B.png&"),
            "unexpected url: {url}"
        );
    }

    #[test]
    fn the_title_is_the_captures_file_name() {
        assert_eq!(
            window_title(Path::new("/Users/a/Pictures/Snapdeck 1.png")),
            "Snapdeck 1.png"
        );
        assert_eq!(window_title(Path::new("/")), "Snapdeck");
    }

    /// Every editor gets a label of its own, because a window that has been
    /// asked to close still holds the one it has.
    #[test]
    fn editor_labels_do_not_repeat() {
        let first = next_editor_label();
        let second = next_editor_label();
        assert_ne!(first, second);
        assert!(first.starts_with(EDITOR_LABEL_PREFIX));
        assert!(second.starts_with(EDITOR_LABEL_PREFIX));
    }
}
