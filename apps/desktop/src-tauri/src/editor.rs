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
//! is the full-resolution capture rather than a throwaway backdrop. The static
//! asset scope covers the cache directory the frozen frames live in and nothing
//! else, so each capture is added to the scope as its window is built and taken
//! out again when the window is destroyed; see `allow_capture` below for why
//! widening the configured scope to `~/Pictures` instead is not an option.
//!
//! As many editors as the user has taken captures. A window holds unsaved
//! annotations that Tauri's `close()` cannot ask the page about, so a new
//! capture opens a window of its own and leaves the previous one alone; the
//! label carries a sequence number so that the two never collide, and only the
//! focused editor answers the page's Close button.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use percent_encoding::utf8_percent_encode;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

// The window server's own id for a window, which is what `SCWindow.windowID`
// reports and therefore what the capture picker matches against. The overlay
// defines it because the overlay needed it first; an editor window needs the
// same answer for the same reason, and duplicating the `NSWindow` call here
// would mean a second piece of unsafe code doing one thing.
use crate::overlay::overlay_window_id as window_server_id;
use crate::{overlay::PATH_QUERY_ENCODE_SET, report::report_failure, state::AppState};

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
/// A window may be as large as the picture plus the chrome the toolbar needs,
/// and this is that chrome: a capture of a 200-point dialog would otherwise
/// open a window too narrow for its own toolbar, which wraps onto three rows
/// and leaves less room for the picture than the floor does. The picture is
/// still never enlarged past 1:1, because the editor's own viewport caps it
/// there.
///
/// MIRRORS THE TOOLBAR'S LAYOUT AND MUST BE CHANGED WITH IT. Measured against
/// the packaged editor at this width, the toolbar in
/// `packages/editor/src/Editor.tsx` fills two rows and its right-hand button
/// group ends about thirteen points from the edge; a narrower window wraps it
/// onto a third. That makes 720 a property of the toolbar and not of Rust, and
/// it is written down twice today: here, and implicitly in that component's
/// layout, where nothing fails if it changes. The number belongs in one place,
/// exported by `packages/editor` and read by both the toolbar's own `min-width`
/// and this constant. That package is owned by another change in this pass, so
/// the Rust half is this comment; the export the other side has to add is
/// recorded in the task report.
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
/// `scale` is the scale factor of the display the capture was taken from, not
/// the one the window will open on: it is what turns the capture's pixels back
/// into the points the window is measured in, and on a mixed-DPI setup the two
/// displays disagree.
///
/// Any editor already open is left alone. It may hold annotations the user has
/// not saved, `close()` is a request the page cannot refuse, and there is no
/// reason for a finished capture to destroy an unfinished one.
///
/// A failure is reported rather than returned. There is no caller left to
/// answer to by the time the window is built, and the user has to know that
/// the editor they were expecting is not coming; the capture itself is safe,
/// which is what the message says.
pub fn open_editor(app: &AppHandle, path: &Path, width: u32, height: u32, scale: f32) {
    let handle = app.clone();
    // Owned, because the closure outlives this call: it runs on the next turn
    // of the main thread's event loop, by which time the capture worker's
    // borrow is long gone.
    let owned = path.to_path_buf();
    if let Err(err) = app.run_on_main_thread(move || {
        if let Err(err) = build_editor_window(&handle, &owned, width, height, scale) {
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

/// Closes the editor window the user is looking at, and only that one.
///
/// The page's Close button is what reaches this, and a page can only be pressed
/// while its own window holds the keyboard, so the focused editor is the window
/// that asked. Closing every editor instead, which is what this used to do,
/// would throw away another window's unsaved annotations as soon as a second
/// one existed.
///
/// Nothing is closed when the focused window is not an editor: that is either a
/// window this has no business closing or no focused window at all, and the
/// title bar's own button still works.
///
/// Main thread only, which the synchronous `commands::close_editor` already is.
pub fn close_focused_editor(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(EDITOR_LABEL_PREFIX) && window.is_focused().unwrap_or(false) {
            let _ = window.close();
            return;
        }
    }
}

/// Main thread only: macOS requires window creation there.
fn build_editor_window(
    app: &AppHandle,
    path: &Path,
    width: u32,
    height: u32,
    scale: f32,
) -> tauri::Result<WebviewWindow> {
    let (window_width, window_height) = editor_window_size(app, width, height, scale);
    let label = next_editor_label();
    // Before the window, because the page asks for the picture as soon as it
    // runs and the protocol handler consults the scope on every request.
    allow_capture(app, path);
    let window = WebviewWindowBuilder::new(
        app,
        label.clone(),
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
    .build()?;

    // Recorded here, on the main thread the window id has to be read from and
    // while the window is certainly alive. The capture behind it is what
    // `save_edited` checks a save request against, and the id is what keeps
    // window mode from offering the editor as something to photograph.
    app.state::<AppState>().register_editor(
        label.clone(),
        path.to_path_buf(),
        window_server_id(&window),
    );

    // `Destroyed` rather than the close command, because the title bar's red
    // button is a way out too and it never reaches Rust otherwise. This is the
    // one place an editor stops existing.
    let handle = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            release_editor(&handle, &label);
        }
    });
    Ok(window)
}

/// Lets the webviews read one capture, and nothing else in the pictures
/// directory.
///
/// The configured scope is `$APPCACHE/**`, which covers the frozen frames and
/// stops there. Widening it to `$PICTURE/**` would be recursive and would apply
/// to every webview in the application rather than to the editor: it grants
/// read access to `~/Pictures/Photos Library.photoslibrary`, which is every
/// original photo, every thumbnail and `Photos.sqlite`. A per-capture grant is
/// expressible instead, because `asset_protocol_scope` hands back a handle onto
/// the same `Arc`-backed scope the protocol handler consults per request.
///
/// The canonical path, because that is the form the handler resolves an
/// incoming request to before matching it: a pictures folder the user has moved
/// onto another volume is reached through a symlink, and the pattern has to be
/// on the far side of it.
fn allow_capture(app: &AppHandle, path: &Path) {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Err(err) = app.asset_protocol_scope().allow_file(&path) {
        // Not fatal here: the window is still worth opening, and the page says
        // plainly that it could not open the picture, which is the surface
        // closest to the user.
        eprintln!(
            "snapdeck: {} could not be added to the asset scope: {err}",
            path.display()
        );
    }
}

/// Forgets an editor window that has been destroyed, and takes its capture back
/// out of the asset scope.
///
/// Only once no editor is left on that capture, which `forget_editor` decides
/// under its own lock.
///
/// A forbidden path stays forbidden for the life of the process: Tauri's scope
/// has no way to undo one, and a forbidden pattern beats an allowed one. That
/// is safe here only because a capture path is opened once and never again:
/// `save_png_without_overwriting` creates each capture under a name nothing
/// else holds, and the editor is opened only on a file that call has just
/// created. A future path that reopens an editor on an existing picture has to
/// revisit this, or that picture will refuse to load.
fn release_editor(app: &AppHandle, label: &str) {
    let Some(capture) = app.state::<AppState>().forget_editor(label) else {
        return;
    };
    let capture = capture
        .canonicalize()
        .unwrap_or_else(|_| capture.to_path_buf());
    if let Err(err) = app.asset_protocol_scope().forbid_file(&capture) {
        eprintln!(
            "snapdeck: {} could not be taken out of the asset scope: {err}",
            capture.display()
        );
    }
}

/// The window's inner size in points, from the capture's size in pixels.
///
/// Points, not pixels: `inner_size` is logical, and the capture was taken at
/// the density of the display it came from, so a retina screenshot is half its
/// pixel size on screen. `capture_scale` is that display's, which is the only
/// one that can convert these pixels; the work area and the scale behind it are
/// the primary monitor's, because `center()` is what decides where the window
/// lives. On a single-display setup the two are the same number, and on a
/// mixed-DPI setup using the primary monitor's for both opened a window at half
/// or twice the size of the picture in it.
///
/// Never larger than the picture plus its toolbar, and never larger than the
/// space available. The editor's viewport caps the picture at 1:1, so a window
/// bigger than the capture would only add empty stage around it.
fn editor_window_size(app: &AppHandle, width: u32, height: u32, capture_scale: f32) -> (f64, f64) {
    let monitor = app.primary_monitor().ok().flatten();
    let host_scale = monitor
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
                    f64::from(area.size.width) / host_scale * MAX_WORK_AREA_FRACTION,
                    f64::from(area.size.height) / host_scale * MAX_WORK_AREA_FRACTION,
                )
            });
    fit_editor_window(
        width,
        height,
        capture_scale,
        available_width,
        available_height,
    )
}

/// The arithmetic of `editor_window_size`, without the monitor.
///
/// Split out for the reason `commands::without_windows` is: everything around
/// it needs a live `AppHandle` and a monitor, and this is the whole of what can
/// be wrong.
fn fit_editor_window(
    width: u32,
    height: u32,
    capture_scale: f32,
    available_width: f64,
    available_height: f64,
) -> (f64, f64) {
    // A display that reports a scale of zero or worse would otherwise turn the
    // picture's size into an infinity or a NaN, and every comparison below
    // would then answer the wrong thing rather than fail.
    let capture_scale = f64::from(capture_scale);
    let capture_scale = if capture_scale.is_normal() && capture_scale > 0.0 {
        capture_scale
    } else {
        1.0
    };
    let picture_width = f64::from(width) / capture_scale;
    let picture_height = f64::from(height) / capture_scale;
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
    /// asked to close still holds the one it has, and because two editors are
    /// now open at once by design.
    #[test]
    fn editor_labels_do_not_repeat() {
        let first = next_editor_label();
        let second = next_editor_label();
        assert_ne!(first, second);
        assert!(first.starts_with(EDITOR_LABEL_PREFIX));
        assert!(second.starts_with(EDITOR_LABEL_PREFIX));
    }

    const ROOM: f64 = f64::INFINITY;

    /// The capture's own display converts its pixels, so the same picture is
    /// half the size on a retina display as on a plain one. Using the primary
    /// monitor's scale for both was what opened a window at twice the size of
    /// the picture in it on a mixed-DPI setup.
    #[test]
    fn the_capture_is_measured_with_its_own_displays_scale() {
        let (_, retina_height) = fit_editor_window(1000, 680, 2.0, ROOM, ROOM);
        let (plain_width, plain_height) = fit_editor_window(1000, 680, 1.0, ROOM, ROOM);
        assert_eq!(retina_height, 340.0 + TOOLBAR_ALLOWANCE);
        assert_eq!(plain_width, 1000.0);
        assert_eq!(plain_height, 680.0 + TOOLBAR_ALLOWANCE);
    }

    /// The one deliberate exception to "no larger than the picture": a capture
    /// narrower than the toolbar opens at the toolbar's width.
    #[test]
    fn a_capture_narrower_than_the_toolbar_opens_at_the_floor() {
        assert_eq!(
            fit_editor_window(1000, 680, 2.0, ROOM, ROOM).0,
            MIN_EDITOR_WIDTH
        );
    }

    /// A full-screen capture is exactly the size of its display, so the window
    /// has to be clamped to the space it has to live in.
    #[test]
    fn the_window_never_outgrows_the_work_area() {
        assert_eq!(
            fit_editor_window(3840, 2160, 2.0, 1512.0, 900.0),
            (1512.0, 900.0)
        );
    }

    /// A display that reports no usable scale must still produce a window.
    #[test]
    fn an_unusable_scale_falls_back_to_one_to_one() {
        assert_eq!(
            fit_editor_window(900, 700, 0.0, ROOM, ROOM),
            fit_editor_window(900, 700, 1.0, ROOM, ROOM)
        );
    }

    /// The asset protocol scope is the one thing here that is configuration
    /// rather than code, and widening it is silent: nothing fails, every
    /// webview simply gains read access to everything under the new root.
    /// `~/Pictures` holds the photo library, so this is the regression worth
    /// pinning down.
    #[test]
    fn the_configured_asset_scope_does_not_cover_the_pictures_directory() {
        let config = include_str!("../tauri.conf.json");
        assert!(
            config.contains(r#""scope": ["$APPCACHE/**"]"#),
            "the asset protocol scope is no longer the cache directory alone"
        );
        assert!(
            !config.contains("$PICTURE"),
            "captures are granted one at a time by `allow_capture`, not by the configured scope"
        );
    }
}
