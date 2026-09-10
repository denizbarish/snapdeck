//! Commands the frontend may invoke.
//!
//! Two of them keep the overlay alive: the dismissal, because the overlay
//! covers every display and swallows every click, so without a way to say
//! "never mind" the only exit is the reveal deadline, which does not apply to
//! a window that did show its frame; and the window list, because window mode
//! has nothing to highlight until it knows where the windows are. The rest end
//! the capture: `capture_region` turns a confirmed selection into a file and a
//! clipboard image, and the two permission commands report what the preflight
//! knows for a settings surface to show.
//!
//! The last three belong to the editor, which opens on top of a capture that is
//! already complete. They are what an annotated picture leaves through: back
//! onto the file it came from, onto the clipboard, or nowhere at all when the
//! user closes the window. `save_edited` is the only command in this file that
//! takes a path from the webview, and it is checked accordingly: against the
//! capture the window that asked was opened on, and never by truncating the
//! file it is about to replace.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use image::{ImageReader, Limits};
use serde::Serialize;
use snapdeck_capture::{
    macos::permission::{
        request_screen_capture_permission, screen_capture_permission, PermissionState,
        SETTINGS_DEEP_LINK,
    },
    CaptureTarget, Frame, Rect, ScreenCapturer, WindowInfo,
};
use tauri::{image::Image, AppHandle, Manager, WebviewWindow};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;

use crate::{
    editor,
    output::{render_filename, save_capture_without_overwriting, OffsetDateTimeParts},
    overlay,
    report::report_failure,
    settings::{self, Settings},
    state::AppState,
};

/// A point in the global point space shared by every display.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// Everything window mode needs to highlight the window under the pointer.
///
/// The origin travels with the list because the two are in different spaces:
/// window bounds are in the global point space that spans every display, while
/// the overlay's pointer events are display-local. One of them has to be
/// rebased onto the other, and only Rust knows where this display sits.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowListResult {
    /// On-screen windows in global points, front to back, with this
    /// application's own overlays removed.
    ///
    /// The order is the contract the frontend relies on: it takes the first
    /// window containing the pointer, so a list in any other order picks the
    /// window behind and draws an outline that disagrees with the frozen frame
    /// under it. `SCShareableContent` promises no order and measurably does
    /// not provide one, so `ScreenCapturer::windows` sorts by the window
    /// server's own stacking list before this ever sees it.
    pub windows: Vec<WindowInfo>,
    /// Top-left corner of the display this overlay covers, in global points.
    pub origin: Point,
}

/// What the screen recording preflight knows, plus where to go about it.
///
/// The deep link travels with the state so that the surface showing it never
/// has to spell out a `x-apple.systempreferences:` URL of its own; the one
/// this application uses is defined once, in the capture crate.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReport {
    pub state: PermissionState,
    pub settings_url: &'static str,
}

/// Where a finished capture ended up, and how big it is in pixels.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureResult {
    /// Absolute path of the saved PNG, or `null` when the file could not be
    /// written and the capture only reached the clipboard. The failure is
    /// reported to the user either way; see `capture_and_write`.
    pub path: Option<String>,
    /// Pixels, not points: the region is captured at the display's own
    /// density, so on Retina this is twice the number of points selected.
    pub width: u32,
    pub height: u32,
}

/// The current screen recording permission, without prompting.
///
/// `Granted` is a necessary condition and never a sufficient one. macOS
/// decides what a process may capture when the process starts, so a grant made
/// while Snapdeck is running is invisible to it: the preflight flips to
/// `Granted`, and every ScreenCaptureKit call keeps failing until the app is
/// relaunched. That is why macOS itself offers "Quit & Reopen" on the prompt.
/// Anything rendering this must therefore say "granted, relaunch Snapdeck"
/// rather than "ready", because the second is a claim this cannot support.
#[tauri::command]
pub fn permission_state() -> PermissionReport {
    PermissionReport {
        state: screen_capture_permission(),
        settings_url: SETTINGS_DEEP_LINK,
    }
}

/// Asks macOS for the screen recording permission.
///
/// Only the first call in the life of the system prompts; afterwards macOS
/// stays silent and returns the stored answer, which is why the report carries
/// `settings_url`: a `Denied` here means the user has to be sent to the
/// settings pane by hand. The relaunch caveat on `permission_state` applies
/// verbatim to a `Granted` returned here.
#[tauri::command]
pub fn request_permission() -> PermissionReport {
    PermissionReport {
        state: request_screen_capture_permission(),
        settings_url: SETTINGS_DEEP_LINK,
    }
}

/// Closes every overlay and drops the frozen frames behind them.
///
/// All of them, not just the one that asked. Every overlay is built
/// `.focused(true)`, so on a multi-display setup only the last one built holds
/// the keyboard, and an Escape that closed the focused window alone would
/// leave the other displays covered with nothing left to press Escape in.
///
/// The frames go too, but only when this dismissal can claim the capture slot.
/// `overlay::close_overlays` deliberately leaves them, because it also runs
/// from inside a capture that is still writing them. A dismissal usually means
/// the user has finished with this capture, and the frames are
/// full-resolution, lossless copies of everything that was on their screen, so
/// keeping them in `~/Library/Caches` until the next capture happens to
/// overwrite them is a privacy cost with no upside.
///
/// "Usually", because a failed `capture_region` sends the overlay here through
/// its own `.catch`, and a second capture may already be under way by then. It
/// holds the slot for exactly as long as it is writing the frames its own
/// overlays will show, so a claim that fails means the frames on disk belong
/// to that capture and not to this dismissal. They are then left to whoever
/// owns them: the next trigger discards them before it freezes again.
///
/// Synchronous on purpose, which is the opposite of `list_windows` next door:
/// `WebviewWindow::close` has to run on the main thread on macOS, and a
/// synchronous Tauri command is the one kind that already does.
#[tauri::command]
pub fn close_overlays(app: AppHandle) {
    overlay::close_overlays(&app);
    if let Some(_guard) = app.state::<AppState>().begin_capture() {
        overlay::discard_cached_frozen_frames(&app);
    }
}

/// The windows window mode may highlight, plus the origin of the display the
/// asking overlay covers.
///
/// `async` is load-bearing. Tauri runs a synchronous command on the main
/// thread, and `ScreenCapturer` documents every one of its calls as blocking
/// for a full platform round trip; Task 6 measured 108 to 230 ms for a capture
/// on this machine. Running that on the main thread would freeze the AppKit
/// event loop, and with it the very overlay that asked. `spawn_blocking`
/// rather than a plain `async` body because the work inside really is blocking
/// and would otherwise sit on an async worker that has other futures to poll.
#[tauri::command]
pub async fn list_windows(app: AppHandle, display_id: u32) -> Result<WindowListResult, String> {
    tauri::async_runtime::spawn_blocking(move || collect_windows(&app, display_id))
        .await
        .map_err(|err| format!("the window list task did not finish: {err}"))?
}

/// Blocking worker only; see `list_windows`.
fn collect_windows(app: &AppHandle, display_id: u32) -> Result<WindowListResult, String> {
    let state = app.state::<AppState>();
    // Recorded when the windows were built, on the main thread that `NSWindow`
    // requires. Reading it here is a lock and a clone, with no hop to a main
    // thread that is busy creating the very windows being asked about; see
    // `AppState::overlay_window_ids`.
    //
    // The editors as well as the overlays. An editor is an ordinary window in
    // the window list, so window mode would otherwise offer the user a picture
    // of the editor they took the last capture in, and offer it first: it is
    // the frontmost window on screen.
    let mut ours = state.overlay_window_ids();
    ours.extend(state.editor_window_ids());

    let display = state
        .capturer
        .displays()
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|display| display.id == display_id)
        // Reachable: a display can be unplugged between the capture that built
        // this overlay and the overlay asking about it. Refusing beats
        // guessing an origin, because a wrong origin highlights the wrong
        // rectangle rather than failing visibly.
        .ok_or_else(|| format!("no display with id {display_id}"))?;

    let windows = state.capturer.windows().map_err(|err| err.to_string())?;
    Ok(WindowListResult {
        windows: without_windows(windows, &ours),
        origin: Point {
            x: display.bounds.x,
            y: display.bounds.y,
        },
    })
}

/// Drops the windows whose ids are in `excluded`, keeping the rest in order.
///
/// Split out from `collect_windows` because it is the whole of that function
/// that can be tested: everything around it needs an `AppHandle`, a live
/// display and a screen recording grant. Order is preserved because the list
/// is front to back and the frontend takes the first match.
fn without_windows(windows: Vec<WindowInfo>, excluded: &[u32]) -> Vec<WindowInfo> {
    windows
        .into_iter()
        .filter(|window| !excluded.contains(&window.id))
        .collect()
}

/// Turns a confirmed selection into a PNG in the pictures directory and an
/// image on the clipboard.
///
/// `rect` is in the display-local points the overlay works in, because only
/// Rust knows where that display sits in the global space.
///
/// `async` plus `spawn_blocking` for the same reason as `list_windows`: a
/// synchronous command runs on the main thread, and everything below blocks
/// for a platform round trip.
#[tauri::command]
pub async fn capture_region(
    app: AppHandle,
    display_id: u32,
    rect: Rect,
) -> Result<CaptureResult, String> {
    let handle = app.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || capture_selection(&app, display_id, rect))
            .await
            .map_err(|err| format!("the capture task did not finish: {err}"))?;
    // The overlay that asked is deliberately closed before the capture runs, so
    // an `Err` returned from here has no webview left to reach and no console
    // to land in. The notification and the log file are the only surfaces left,
    // which is what the rest of the capture path now uses too.
    if let Err(err) = &result {
        report_failure(
            &handle,
            &format!("Snapdeck could not save the capture: {err}"),
        );
    }
    result
}

/// Blocking worker only; see `capture_region`.
fn capture_selection(
    app: &AppHandle,
    display_id: u32,
    rect: Rect,
) -> Result<CaptureResult, String> {
    let state = app.state::<AppState>();
    // The same one-at-a-time slot the trigger claims. Held for the whole of
    // this, not because two captures would race for the file, but because the
    // cleanup at the end unlinks `frozen-<id>.png`: without the claim, a
    // shortcut pressed during these few hundred milliseconds would start a
    // capture, write fresh frozen frames, and have them deleted underneath its
    // half-built overlays. `None` means such a capture is already under way,
    // and it has closed these overlays and will draw its own.
    let _guard = state
        .begin_capture()
        .ok_or_else(|| "another capture started before this selection was confirmed".to_string())?;

    dismiss_overlays_and_wait(app)?;

    let captured = capture_and_write(app, &state, display_id, rect);
    // Unconditionally, and here rather than inside: every step below the
    // dismissal can fail, and the overlays are already off the screen by then,
    // so a `?` that returned straight to the caller would leave a
    // full-resolution, lossless copy of everything that was on the user's
    // screen in `~/Library/Caches`. The frontend cannot make up for it either:
    // its `.catch` needs a live webview, and this capture destroyed the one
    // that asked. The likeliest failure of the lot, a grant lost between the
    // freeze and Enter, is also the one where the user stops pressing the
    // shortcut, so nothing later would come along and clear the residue.
    //
    // Still inside the guard, so no capture started in the meantime can be
    // writing the frames this deletes.
    overlay::discard_cached_frozen_frames(app);
    // The editor opens on top of a capture that is already finished, and it is
    // the last thing that happens rather than a step in the middle: the file is
    // written and the clipboard holds the image before this line runs, so
    // nothing the editor does or fails to do can cost the user a capture.
    //
    // Only when there is a file. The page reads the picture over the asset
    // protocol, so a capture that reached the clipboard and not the disk has
    // nothing for the editor to open, and the notification that half of it
    // failed has already been sent.
    //
    // And only when the user wants one. `open_editor_after_capture` is the
    // setting for people who take a screenshot to paste it, for whom a window
    // opening on every capture is something to close every time.
    if !state.settings().open_editor_after_capture {
        return captured.map(|captured| captured.result);
    }
    if let Ok(Captured {
        result:
            CaptureResult {
                path: Some(path),
                width,
                height,
            },
        scale,
    }) = &captured
    {
        // The source display's scale, not the primary monitor's: it is what
        // turns these pixels back into the points a window is measured in, and
        // on a mixed-DPI setup the display the capture came from and the
        // display the window opens on disagree about it.
        editor::open_editor(app, Path::new(path), *width, *height, *scale);
    }
    captured.map(|captured| captured.result)
}

/// A finished capture, plus the one thing about it that only the capture path
/// knows and the frontend has no use for.
///
/// Kept out of `CaptureResult` because that type is the command's serialised
/// answer to the page, and the scale factor is for the editor window builder
/// on this side of the boundary.
struct Captured {
    result: CaptureResult,
    /// Scale factor of the display the region was taken from.
    scale: f32,
}

/// The capture itself, once the screen is clear: everything from the display
/// lookup to the clipboard write.
///
/// Split out from `capture_selection` so that its caller can run the frozen
/// frame cleanup on every path out of it, successful or not.
fn capture_and_write(
    app: &AppHandle,
    state: &AppState,
    display_id: u32,
    rect: Rect,
) -> Result<Captured, String> {
    let display = state
        .capturer
        .displays()
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|display| display.id == display_id)
        .ok_or_else(|| format!("no display with id {display_id}"))?;

    // The overlay reports display-local points; capture expects global points.
    let global = Rect {
        x: display.bounds.x + rect.x,
        y: display.bounds.y + rect.y,
        width: rect.width,
        height: rect.height,
    };
    let frame = state
        .capturer
        .capture(CaptureTarget::Region(global))
        .map_err(|err| err.to_string())?;

    // The settings in force, read once, so a save that lands in the settings
    // window halfway through this capture cannot split it between two folders.
    let settings = state.settings();
    // Never a reason to lose the picture. A folder that has gone away, an
    // unplugged volume or a permission change puts the capture in the pictures
    // directory instead and says so, rather than failing over a setting.
    let (directory, complaint) = settings::resolve_save_directory(app, &settings);
    if let Some(complaint) = complaint {
        report_failure(app, &complaint);
    }
    // Best effort, and deliberately not a `?`. This used to return early, which
    // meant a pictures directory that could not be created threw away a capture
    // the user had already framed and confirmed, clipboard and all. A failure
    // here now shows up as a failed save below, which keeps the clipboard.
    let _ = std::fs::create_dir_all(&directory);
    // The clipboard first, and both outcomes collected rather than the first
    // failure returned. A full disk, a read-only pictures directory or a
    // permission problem takes the file away, and a `?` here used to take the
    // clipboard with it: the capture the user had already framed and confirmed
    // was thrown away over a directory, when the image was in hand and one
    // paste away from being useful. Only losing both is a failed capture.
    let clipboard = copy_to_clipboard(app, &frame);
    let saved = write_capture(&frame, &directory, &settings);

    let result = |path: Option<String>| Captured {
        result: CaptureResult {
            path,
            width: frame.width,
            height: frame.height,
        },
        scale: display.scale_factor,
    };
    match (saved, clipboard) {
        (Ok(path), Ok(())) => Ok(result(Some(path.to_string_lossy().into_owned()))),
        // Half a capture is still a capture, and the half that survived is the
        // one the user can act on, so it is reported here rather than returned
        // as a failure: an `Err` from this command means nothing was captured.
        (Err(save_err), Ok(())) => {
            report_failure(
                app,
                &format!(
                    "Snapdeck could not save the capture to {} ({save_err}), so it is only on the clipboard. Paste it before you copy anything else.",
                    directory.display()
                ),
            );
            Ok(result(None))
        }
        (Ok(path), Err(clipboard_err)) => {
            report_failure(
                app,
                &format!(
                    "Snapdeck saved the capture to {} but could not put it on the clipboard ({clipboard_err}).",
                    path.display()
                ),
            );
            Ok(result(Some(path.to_string_lossy().into_owned())))
        }
        (Err(save_err), Err(clipboard_err)) => Err(format!("{save_err}, and {clipboard_err}")),
    }
}

/// Writes a finished capture where the settings say, under the name they say,
/// in the format they say, and answers with the path used.
///
/// Split out from `capture_and_write` so that the one claim the settings make
/// about a capture, that the template and the format the user saved are the
/// ones their next file is written with, can be checked against a real file
/// rather than trusted: everything around it needs a display, a screen
/// recording grant and a clipboard.
///
/// The saved path comes back from the write rather than being built here,
/// because a name already taken gets a suffix instead of the previous capture's
/// contents, and because the format decides the extension.
fn write_capture(frame: &Frame, directory: &Path, settings: &Settings) -> Result<PathBuf, String> {
    let name = render_filename(
        &settings.filename_template,
        OffsetDateTimeParts::now(),
        frame.width,
        frame.height,
    );
    save_capture_without_overwriting(frame, directory, &name, settings.default_format)
}

/// Puts the captured pixels on the clipboard.
///
/// Split out so that its failure is a value the caller can weigh against the
/// file's, rather than an early return that decides for it.
fn copy_to_clipboard(app: &AppHandle, frame: &Frame) -> Result<(), String> {
    let rgba = frame
        .to_rgba8()
        .map_err(|err| format!("failed to convert the capture for the clipboard: {err}"))?;
    app.clipboard()
        .write_image(&Image::new(&rgba, frame.width, frame.height))
        .map_err(|err| format!("failed to copy the capture to the clipboard: {err}"))
}

/// Takes the overlays off the screen and waits until they are actually gone.
///
/// Both halves matter. The region is captured again at native resolution
/// rather than cropped out of the frozen frame, so an overlay still on screen
/// is not a cosmetic problem: it is what the user's file would contain, a
/// picture of the dimmed selection UI instead of their content. And closing is
/// only a request. `WebviewWindow::close` posts to the event loop, the label
/// leaves the window map when `Destroyed` arrives, and the pixels leave with
/// it, so capturing on the next line would photograph a window that is still
/// up. A timeout abandons the capture rather than saving that picture.
fn dismiss_overlays_and_wait(app: &AppHandle) -> Result<(), String> {
    let handle = app.clone();
    // On the main thread, which is where macOS requires window teardown and
    // where `commands::close_overlays` already gets to be by being synchronous.
    // This one is not: it runs on a blocking worker, so it has to ask.
    app.run_on_main_thread(move || overlay::close_overlays(&handle))
        .map_err(|err| format!("could not reach the main thread to close the overlays: {err}"))?;
    if overlay::wait_for_overlays_to_close(app) {
        return Ok(());
    }
    Err("the overlays did not leave the screen in time, so nothing was captured".to_string())
}

/// The file extensions the editor can produce, and the only ones this command
/// will write.
///
/// Not a formality. `path` is chosen by the webview, and while the editor only
/// ever asks for the extension matching the blob it encoded, this command is
/// the boundary: nothing that arrives here is trusted to be one of those asks.
/// Confining the writes to image files keeps a compromised page from dropping a
/// shell script or a `.command` into a directory the user opens in the Finder.
///
/// No `webp`. WKWebView answers `convertToBlob('image/webp')` with PNG bytes
/// rather than refusing, so the editor dropped the format entirely and `toBlob`
/// now rejects a blob whose type is not the one it asked for. An entry here
/// would re-admit exactly the file that removal exists to prevent: PNG bytes
/// under a `.webp` name, in the user's pictures folder, written by this
/// command. An allowlist wider than the feature it guards is not a spare
/// allowance, it is the hole.
const EDITABLE_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

/// Writes an edited capture back beside the capture it was opened on, and
/// returns the path it used.
///
/// The same file as the capture when the format is unchanged, which is the
/// common case: the page is handed the capture's own path in its URL and gives
/// it straight back, so saving updates the picture the user already has rather
/// than growing a second copy per edit. A different format changes the
/// extension, and therefore the path, so the new file lands beside the original
/// instead of replacing it.
///
/// `async` plus `spawn_blocking` for the reason `capture_region` gives: a
/// synchronous command runs on the main thread, and this one writes a
/// multi-megabyte file.
#[tauri::command]
pub async fn save_edited(
    app: AppHandle,
    window: WebviewWindow,
    path: String,
    bytes: Vec<u8>,
) -> Result<String, String> {
    // Which capture this window was opened on, taken from the window that
    // invoked the command rather than from the request. Rust chose that path
    // and put it in the window's URL, so it already knows the only files this
    // save may touch; a window with no capture behind it is not an editor and
    // has nothing to save.
    let capture = app
        .state::<AppState>()
        .editor_capture(window.label())
        .ok_or_else(|| format!("{} is not an editor window", window.label()))?;
    // The capture's own directory, not the pictures directory. They are the
    // same folder until the user chooses another one, and once they have, the
    // pictures directory is a boundary that would refuse every save of every
    // capture they took. Derived from the capture rather than from the settings
    // for the same reason the capture is: this is a path Rust chose and
    // recorded, so it cannot be changed by a page, and it stays right for an
    // editor that is still open on a capture taken before the folder moved.
    let directory = capture
        .parent()
        .ok_or_else(|| format!("{} has no directory", capture.display()))?
        .to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        write_edited(&directory, &capture, &path, &bytes)
            .map(|written| written.to_string_lossy().into_owned())
    })
    .await
    .map_err(|err| format!("the save task did not finish: {err}"))?
}

/// Blocking worker only; see `save_edited`.
///
/// Takes the directory rather than an `AppHandle` so that the part worth
/// testing, which paths it is willing to write to, can be tested against a
/// temporary directory.
fn write_edited(
    directory: &Path,
    capture: &Path,
    requested: &str,
    bytes: &[u8],
) -> Result<PathBuf, String> {
    let target = resolve_save_target(directory, capture, requested)?;
    write_atomically(&target, bytes)?;
    Ok(target)
}

/// Source of temporary file names, unique for the life of the process.
static NEXT_TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Puts `bytes` at `target` without destroying what is there until they are all
/// on disk.
///
/// This is the only place in the application that overwrites a capture, so it
/// is the only place that can lose one, and `fs::write` loses it in the
/// ordinary way: it opens with `O_TRUNC`, so the original is gone before the
/// first byte of the replacement is written. A full disk on a multi-megabyte
/// PNG, a volume unplugged from under a relocated pictures folder or a crash
/// halfway through then leaves the user with neither picture, while the page
/// says "not saved", which is worse than untrue.
///
/// A sibling temporary file and a rename instead. `rename` is atomic on APFS,
/// so the capture is either the old picture or the whole new one, and it does
/// not follow a symlink at the final component, which makes the refusal in
/// `resolve_save_target` a policy rather than a race it has to win. The
/// temporary name is derived from the target and lands in the same directory
/// that was just checked, so nothing about where this can write has changed,
/// and one filesystem is what keeps the rename a rename.
///
/// `create_new` rather than a plain create: it cannot open an existing file and
/// cannot follow a link planted at the name. `sync_all` before the rename,
/// because publishing a file the filesystem has not written yet is how an
/// atomic rename still ends in an empty capture after a power cut.
fn write_atomically(target: &Path, bytes: &[u8]) -> Result<(), String> {
    let directory = target
        .parent()
        .ok_or_else(|| format!("{} has no directory", target.display()))?;
    let temporary = directory.join(temporary_name(target));

    let mut file = File::options()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|err| {
            format!(
                "failed to create a temporary file beside {}: {err}",
                target.display()
            )
        })?;
    // Every exit from here on removes the temporary file: a half-written
    // picture under a dotted name is residue the user cannot interpret and
    // nothing else would ever clean up.
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(err) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!(
            "failed to write the edited picture for {}: {err}",
            target.display()
        ));
    }
    std::fs::rename(&temporary, target).map_err(|err| {
        let _ = std::fs::remove_file(&temporary);
        format!(
            "failed to put the edited picture at {}: {err}",
            target.display()
        )
    })
}

/// The name the edited picture is written under until the rename publishes it.
///
/// Derived from the target so that it stays inside the directory
/// `resolve_save_target` has already resolved and checked, hidden by a leading
/// dot so that a save killed between the create and the rename leaves nothing
/// the user has to recognise in their pictures folder, and made unique by the
/// process id and a counter so that two saves of the same picture cannot land
/// on one temporary file.
fn temporary_name(target: &Path) -> String {
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let sequence = NEXT_TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!(".{name}.snapdeck-{}-{sequence}.tmp", std::process::id())
}

/// Where `requested` may be written, or why it may not be.
///
/// This is a security boundary and not a convenience check. The path comes from
/// the webview, so it is a string a compromised page chooses, and the command
/// behind it writes arbitrary bytes: without this, `../../.zshrc` is a valid
/// save target. Five things have to hold, and all five are enforced here rather
/// than left to the caller:
///
/// - the path is absolute, because the only paths the editor is ever given are;
/// - it names a file whose extension is one the editor can produce;
/// - its directory, once symlinks are resolved, is exactly the directory the
///   capture is in, so neither `..` nor a symlinked parent walks out of it;
/// - it names `capture`, or `capture` in another of those formats, because
///   those are the only files this editor was opened to write;
/// - nothing already at that name is a symlink. The write itself no longer
///   follows one, since `write_atomically` renames onto the target rather than
///   opening it, so this is a refusal to replace a link the user put there
///   rather than the thing that stops an escape.
///
/// The fourth is what the directory check alone cannot do. "Beside the capture
/// with an image extension" admits `wedding.jpg`, so without it a compromised
/// page can overwrite any picture the user keeps in their save folder with the
/// current canvas. Rust chose the capture's path and put it in this window's
/// URL, so it already knows the only legitimate answers, and the
/// different-format save still works because only the extension is allowed to
/// differ.
///
/// Subdirectories are refused along with everything else. Captures are written
/// flat into the save folder, so a target one level down is not a case that
/// exists, and the strictest rule that still admits every real save is the one
/// worth having.
fn resolve_save_target(
    directory: &Path,
    capture: &Path,
    requested: &str,
) -> Result<PathBuf, String> {
    let requested = Path::new(requested);
    if !requested.is_absolute() {
        return Err(format!("{} is not an absolute path", requested.display()));
    }
    let name = requested
        .file_name()
        .ok_or_else(|| format!("{} does not name a file", requested.display()))?;
    let extension = requested
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    if !extension.is_some_and(|extension| EDITABLE_EXTENSIONS.contains(&extension.as_str())) {
        return Err(format!(
            "{} is not one of the image formats the editor writes",
            requested.display()
        ));
    }

    // Both sides canonicalised, because either can contain a symlink that is
    // not the caller's doing: `/tmp` is one on macOS, and a user may well have
    // moved their pictures folder onto another volume.
    let root = directory
        .canonicalize()
        .map_err(|err| format!("cannot resolve {}: {err}", directory.display()))?;
    let parent = requested
        .parent()
        .ok_or_else(|| format!("{} has no directory", requested.display()))?
        .canonicalize()
        .map_err(|err| {
            format!(
                "cannot resolve the directory of {}: {err}",
                requested.display()
            )
        })?;
    if parent != root {
        return Err(format!(
            "{} is outside {}",
            requested.display(),
            root.display()
        ));
    }

    let target = root.join(name);
    // The stem, because the extension is the one part a different export format
    // is allowed to change. Both sides come from a file name, so neither is a
    // path this could be tricked about.
    if target.file_stem() != capture.file_stem() {
        return Err(format!(
            "{} is not the capture this editor was opened on",
            requested.display()
        ));
    }
    // `symlink_metadata` rather than `exists`, which follows the link and would
    // answer for whatever is on the far end of it. A dangling link is refused
    // too: the write would create the file it points at.
    if let Ok(metadata) = std::fs::symlink_metadata(&target) {
        if metadata.file_type().is_symlink() {
            return Err(format!("{} is a symbolic link", target.display()));
        }
    }
    Ok(target)
}

/// Puts an edited picture on the clipboard, replacing the capture that the
/// original put there.
///
/// The bytes arrive encoded, because that is what the editor's canvas produces
/// and what its other exit, the file, needs; the clipboard needs raw pixels, so
/// they are decoded here rather than sent twice from the page.
#[tauri::command]
pub async fn copy_edited(app: AppHandle, bytes: Vec<u8>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || copy_encoded_to_clipboard(&app, &bytes))
        .await
        .map_err(|err| format!("the clipboard task did not finish: {err}"))?
}

/// The largest edited picture this will decode, per side.
///
/// A decode limit rather than a sanity check. The bytes come from the page, and
/// an image header is a promise rather than a measurement: a few hundred bytes
/// can declare 60000 by 60000 and the decoder will try to allocate every one of
/// them. Snapdeck is a menu bar agent, so the process that dies takes the tray,
/// the shortcuts and the user's next capture with it, and there is no window
/// for them to see it happen in. The bound is far above any capture this editor
/// can produce: two Pro Display XDRs side by side are 12032 pixels wide.
///
/// The allocation ceiling that comes with `Limits::default()`, 512 MiB, is left
/// as it is: it is what stops a picture inside these dimensions from being
/// expensive, and 16384 squared in RGBA is 1 GiB, twice it.
const MAX_EDITED_DIMENSION: u32 = 16_384;

/// Blocking worker only; see `copy_edited`.
fn copy_encoded_to_clipboard(app: &AppHandle, bytes: &[u8]) -> Result<(), String> {
    let mut reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|err| format!("failed to read the edited picture: {err}"))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_EDITED_DIMENSION);
    limits.max_image_height = Some(MAX_EDITED_DIMENSION);
    reader.limits(limits);
    let rgba = reader
        .decode()
        .map_err(|err| format!("failed to decode the edited picture: {err}"))?
        .to_rgba8();
    let (width, height) = rgba.dimensions();
    app.clipboard()
        .write_image(&Image::new(rgba.as_raw(), width, height))
        .map_err(|err| format!("failed to copy the edited picture to the clipboard: {err}"))
}

/// Closes the editor window the user is looking at.
///
/// That one and no other: editors are no longer one at a time, and the Close
/// button in one window has no business discarding the annotations in another.
/// The signature is the plan's, so the page still calls this with no arguments
/// and Rust works out which window asked.
///
/// Synchronous on purpose, for the reason `close_overlays` next door gives:
/// `WebviewWindow::close` has to run on the main thread on macOS, and a
/// synchronous Tauri command is the one kind that already does.
#[tauri::command]
pub fn close_editor(app: AppHandle) {
    editor::close_focused_editor(&app);
}

/// The settings in force, which is what the settings window renders.
///
/// Read from the running application rather than from the file, because the two
/// can differ and the running one is the truthful answer: a stored binding that
/// could not be registered was replaced at launch, and the login item is
/// whatever the system says it is.
#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    app.state::<AppState>().settings()
}

/// Puts new settings into force and writes them down, or changes nothing at
/// all.
///
/// All three steps or none, in the order that makes that possible: the refusals
/// that cost nothing come first, then the change that has to be undone if a
/// later one fails, and the file last. What comes back is the settings that are
/// now in force, so the window renders the truth rather than what it asked for.
///
/// Synchronous on purpose. It runs on the main thread, which is where the
/// shortcut manager wants to be reached from anyway, and the only file it
/// writes is a few hundred bytes.
#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> Result<Settings, String> {
    let state = app.state::<AppState>();
    let previous = state.settings();
    // First, because it is the cheapest refusal and the one that must not have
    // rebound a shortcut before it happens.
    if let Some(directory) = &settings.save_directory {
        settings::ensure_writable(directory)?;
    }
    // Registers the new shortcuts, or leaves the previous ones in force and
    // says why; the login item goes with them.
    settings::apply(&app, &previous, &settings)?;
    if let Err(err) = settings::save(&app, &settings) {
        // Nothing was written, so nothing may be left in force: an application
        // whose shortcuts disagree with its own settings file is a state the
        // user cannot explain and the next launch would undo behind their back.
        if let Err(revert_err) = settings::apply(&app, &settings, &previous) {
            return Err(format!(
                "{err}. Your previous settings could not be put back either ({revert_err}); restart Snapdeck."
            ));
        }
        return Err(err);
    }
    state.set_settings(settings.clone());
    Ok(settings)
}

/// Asks the user for a save folder and proves it can be written to.
///
/// `Ok(None)` is a cancelled picker, which is not a failure and must not be
/// rendered as one. A folder that cannot be written to is refused here rather
/// than accepted and discovered at the next capture, which is the whole point
/// of proving it: a setting the user has been shown as accepted has to work.
///
/// `async` plus `spawn_blocking` because `blocking_pick_folder` waits on the
/// main thread to answer, so calling it *from* the main thread, which is where
/// a synchronous command runs, would wait for a reply that cannot be sent.
#[tauri::command]
pub async fn choose_save_directory(app: AppHandle) -> Result<Option<String>, String> {
    // Opens where the captures go now, so the picker starts from the folder the
    // user is about to replace rather than from wherever macOS last was.
    let (start, _) = settings::resolve_save_directory(&app, &app.state::<AppState>().settings());
    tauri::async_runtime::spawn_blocking(move || pick_writable_directory(&app, &start))
        .await
        .map_err(|err| format!("the folder picker did not finish: {err}"))?
}

/// Blocking worker only; see `choose_save_directory`.
fn pick_writable_directory(app: &AppHandle, start: &Path) -> Result<Option<String>, String> {
    let Some(picked) = app
        .dialog()
        .file()
        .set_title("Choose where Snapdeck saves captures")
        .set_directory(start)
        .set_can_create_directories(true)
        .blocking_pick_folder()
    else {
        return Ok(None);
    };
    let picked = picked
        .into_path()
        .map_err(|err| format!("that folder has no path Snapdeck can use: {err}"))?;
    settings::ensure_writable(&picked)?;
    Ok(Some(picked.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: u32) -> WindowInfo {
        WindowInfo {
            id,
            title: None,
            app_name: None,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            layer: 0,
            is_on_screen: true,
        }
    }

    fn ids(windows: &[WindowInfo]) -> Vec<u32> {
        windows.iter().map(|w| w.id).collect()
    }

    #[test]
    fn the_overlays_own_windows_are_dropped() {
        let windows = vec![window(11), window(22), window(33)];
        assert_eq!(ids(&without_windows(windows, &[22])), vec![11, 33]);
    }

    #[test]
    fn one_overlay_per_display_is_dropped() {
        let windows = vec![window(11), window(22), window(33), window(44)];
        assert_eq!(ids(&without_windows(windows, &[22, 44])), vec![11, 33]);
    }

    #[test]
    fn nothing_is_dropped_when_no_overlay_was_recorded() {
        let windows = vec![window(11), window(22)];
        assert_eq!(ids(&without_windows(windows, &[])), vec![11, 22]);
    }

    /// The list is front to back and the frontend takes the first match, so
    /// removing a window may not reshuffle the ones around it.
    #[test]
    fn the_surviving_windows_keep_their_front_to_back_order() {
        let windows = vec![window(5), window(9), window(1), window(7)];
        assert_eq!(ids(&without_windows(windows, &[9])), vec![5, 1, 7]);
    }

    /// A 2x2 BGRA frame with two padding bytes per row, so a capture written
    /// through the settings still goes through the same stride and channel
    /// handling a real one does.
    fn sample_frame() -> Frame {
        #[rustfmt::skip]
        let data = vec![
            0, 0, 255, 255, /**/ 0, 255, 0, 255, /**/ 0, 0,
            255, 0, 0, 255, /**/ 255, 255, 255, 255, /**/ 0, 0,
        ];
        Frame {
            data,
            width: 2,
            height: 2,
            stride: 10,
            pixel_format: snapdeck_capture::PixelFormat::Bgra8,
            scale_factor: 2.0,
            captured_at: std::time::SystemTime::now(),
        }
    }

    /// The claim the settings window makes to the user: the template they saved
    /// is the name their next capture gets.
    ///
    /// A real file, because the interesting failure is a capture path that
    /// still holds a template of its own; the assertion is against the name a
    /// human would predict from the template, not against a second call to the
    /// same renderer.
    #[test]
    fn a_saved_template_is_the_name_the_next_capture_is_written_under() {
        let dir = save_dir("template");
        let settings = Settings {
            filename_template: "shot-{width}x{height}".to_string(),
            ..Settings::default()
        };

        let written = write_capture(&sample_frame(), &dir, &settings).expect("write the capture");

        let name = written
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(name.as_deref(), Some("shot-2x2.png"));
    }

    /// And the format they saved is the format it is written in, extension and
    /// bytes together.
    #[test]
    fn a_saved_format_is_the_format_the_next_capture_is_written_in() {
        let dir = save_dir("capture-format");
        let settings = Settings {
            filename_template: "shot".to_string(),
            default_format: crate::settings::SaveFormat::Jpeg,
            ..Settings::default()
        };

        let written = write_capture(&sample_frame(), &dir, &settings).expect("write the capture");

        let format = image::ImageReader::open(&written)
            .expect("open the capture")
            .format();
        let name = written
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(name.as_deref(), Some("shot.jpg"));
        assert_eq!(format, Some(image::ImageFormat::Jpeg));
    }

    /// A temporary directory of this test's own, canonicalised because
    /// `/var/folders` and `/tmp` are both symlinks on macOS and
    /// `resolve_save_target` compares resolved paths.
    fn save_dir(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock is after the epoch")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("snapdeck-{}-{unique}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create the temporary pictures directory");
        dir.canonicalize().expect("resolve the temporary directory")
    }

    /// The names in a directory, sorted, so a test can say what a save left
    /// behind as well as what it wrote. Temporary files are part of the answer:
    /// a save that leaves one has not finished.
    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("read the directory")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The common case, and the whole point of handing the page the capture's
    /// own path: saving updates the picture the user already has instead of
    /// leaving a second copy behind on every edit.
    #[test]
    fn saving_the_same_name_overwrites_the_capture() {
        let dir = save_dir("overwrite");
        let capture = dir.join("Snapdeck 2026-09-09 at 12.00.00.png");
        std::fs::write(&capture, b"the original capture").expect("write the fixture");

        let written = write_edited(
            &dir,
            &capture,
            &capture.to_string_lossy(),
            b"the annotated capture",
        )
        .expect("the save should be accepted");

        assert_eq!(written, capture);
        assert_eq!(
            std::fs::read(&capture).expect("read the capture"),
            b"the annotated capture"
        );
        let left = entries(&dir);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(left, vec!["Snapdeck 2026-09-09 at 12.00.00.png"]);
    }

    /// A different format is a different extension, so it is a different path,
    /// so the original survives untouched beside the new file.
    #[test]
    fn saving_a_different_extension_writes_a_new_file_beside_the_original() {
        let dir = save_dir("extension");
        let capture = dir.join("Snapdeck.png");
        std::fs::write(&capture, b"the original capture").expect("write the fixture");
        let as_jpeg = dir.join("Snapdeck.jpg");

        let written = write_edited(
            &dir,
            &capture,
            &as_jpeg.to_string_lossy(),
            b"the annotated capture",
        )
        .expect("the save should be accepted");

        assert_eq!(written, as_jpeg);
        let original = std::fs::read(&capture).expect("read the original");
        let new_file = std::fs::read(&as_jpeg).expect("read the new file");
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(original, b"the original capture");
        assert_eq!(new_file, b"the annotated capture");
    }

    /// The security boundary. `path` is a string the webview chooses, so every
    /// one of these is a write this command must refuse, and refusing has to
    /// mean nothing was written rather than written somewhere else.
    ///
    /// Each case is paired with a capture of its own name, so that what refuses
    /// it is the rule the case is about and not the check that the target is
    /// the capture this editor was opened on.
    #[test]
    fn a_path_outside_the_pictures_directory_is_rejected() {
        let dir = save_dir("escape");
        let outside = dir.parent().expect("a parent").join("outside.png");
        // A symlinked directory inside the pictures directory, which is the
        // case the doc comment claims and nothing tested: the parent resolves
        // to somewhere else entirely, and only canonicalising it says so.
        let linked = dir.join("sub");
        std::os::unix::fs::symlink("/etc", &linked).expect("plant the directory symlink");
        let cases = [
            // The classic traversal, in the form a webview would send it.
            dir.join("../outside.png").to_string_lossy().into_owned(),
            // An unrelated absolute path.
            "/tmp/outside.png".to_string(),
            // Somewhere no capture could ever be.
            format!("{}/.zshrc.png", std::env::var("HOME").unwrap_or_default()),
            // A subdirectory of the pictures directory: captures are written
            // flat, so this is refused with the rest.
            dir.join("nested/outside.png")
                .to_string_lossy()
                .into_owned(),
            // Through a symlinked subdirectory, which is a real directory
            // somewhere else.
            linked.join("outside.png").to_string_lossy().into_owned(),
            // Not an image the editor can produce, inside the directory.
            dir.join("payload.command").to_string_lossy().into_owned(),
            // WebP, which the editor stopped producing when it turned out
            // WKWebView encodes PNG under that name. The allowlist has to be
            // exactly the formats the editor can make, or this is a way back to
            // a `.webp` file holding PNG bytes.
            dir.join("payload.webp").to_string_lossy().into_owned(),
            // Relative, which no editor URL ever carries.
            "outside.png".to_string(),
        ];

        let mut refusals = Vec::new();
        for case in &cases {
            // The capture this hypothetical editor was opened on: the same
            // name, in the pictures directory, where a real one would be.
            let stem = Path::new(case)
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            let capture = dir.join(format!("{stem}.png"));
            refusals.push(write_edited(&dir, &capture, case, b"escaped").is_err());
        }

        let escaped = outside.exists();
        std::fs::remove_file(&linked).ok();
        let leftovers = entries(&dir);
        std::fs::remove_file(&outside).ok();
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(refusals, vec![true; cases.len()], "cases: {cases:?}");
        assert!(!escaped, "a refused save still wrote {}", outside.display());
        assert!(leftovers.is_empty(), "a refused save wrote {leftovers:?}");
    }

    /// The directory check is not enough on its own: every picture the user
    /// owns lives in the same directory as their captures, and a compromised
    /// page would otherwise be free to overwrite any of them with its canvas.
    #[test]
    fn a_picture_that_is_not_this_editors_capture_is_rejected() {
        let dir = save_dir("target");
        let capture = dir.join("Snapdeck.png");
        std::fs::write(&capture, b"the original capture").expect("write the fixture");
        let wedding = dir.join("wedding.jpg");
        std::fs::write(&wedding, b"the wedding photograph").expect("write the fixture");

        let refused = write_edited(&dir, &capture, &wedding.to_string_lossy(), b"the canvas");

        let survived = std::fs::read(&wedding).expect("read the wedding photograph");
        let left = entries(&dir);
        std::fs::remove_dir_all(&dir).ok();
        assert!(refused.is_err(), "{refused:?}");
        assert_eq!(survived, b"the wedding photograph");
        assert_eq!(left, vec!["Snapdeck.png", "wedding.jpg"]);
    }

    /// A symlink already sitting at the target name is refused rather than
    /// replaced. The write no longer follows it either way, because the rename
    /// replaces the link and not the file at the far end of it, so this is a
    /// refusal to touch something the user put there.
    #[cfg(unix)]
    #[test]
    fn a_symlink_at_the_target_name_is_rejected() {
        let dir = save_dir("symlink");
        let outside = dir.parent().expect("a parent").join("linked.png");
        std::fs::write(&outside, b"somebody else's file").expect("write the fixture");
        let link = dir.join("Snapdeck.png");
        std::os::unix::fs::symlink(&outside, &link).expect("plant the symlink");

        let refused = write_edited(&dir, &link, &link.to_string_lossy(), b"escaped").is_err();

        let target = std::fs::read(&outside).expect("read the linked file");
        std::fs::remove_file(&outside).ok();
        std::fs::remove_dir_all(&dir).ok();
        assert!(refused);
        assert_eq!(target, b"somebody else's file");
    }

    /// The reason the write is a rename. A save that cannot be completed must
    /// leave the capture the user already has, because that file is the only
    /// copy of it: `fs::write` would have truncated it first and left them with
    /// nothing under a message that says the save failed.
    ///
    /// The failure is forced by taking write permission off the directory,
    /// which is the same shape as the failures this is really about, a full
    /// disk or a volume that has gone away: the new bytes cannot be put down,
    /// and the question is what happened to the old ones.
    #[cfg(unix)]
    #[test]
    fn a_failed_write_leaves_the_capture_where_it_was() {
        use std::os::unix::fs::PermissionsExt;

        let dir = save_dir("failed-write");
        let capture = dir.join("Snapdeck.png");
        std::fs::write(&capture, b"the original capture").expect("write the fixture");
        // Readable and traversable, so the target still resolves; not writable,
        // so nothing new can be created in it.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500))
            .expect("make the directory read-only");

        let failed = write_edited(
            &dir,
            &capture,
            &capture.to_string_lossy(),
            b"the annotated capture",
        );

        let survived = std::fs::read(&capture).expect("read the capture");
        let left = entries(&dir);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .expect("restore the directory");
        std::fs::remove_dir_all(&dir).ok();

        assert!(failed.is_err(), "{failed:?}");
        assert_eq!(
            survived, b"the original capture",
            "a failed save destroyed the only copy of the capture"
        );
        assert_eq!(left, vec!["Snapdeck.png"], "a failed save left {left:?}");
    }
}
