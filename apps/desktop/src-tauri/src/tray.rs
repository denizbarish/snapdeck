//! The menu bar item: the application's only persistent visible surface, and
//! since `report` lost its notification, the only one a failed capture has.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItem, MenuItemBuilder, Submenu, SubmenuBuilder},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle, Manager, Wry,
};

// `Settings…` is back. It was taken out because there was no settings window
// for it to open, and a menu item that does nothing is worse than no menu item:
// the application declared no windows and built none at runtime, so the item
// could only ever look up a "main" window that did not exist. It now opens
// `settings_window`, which is a window this application really builds.

/// The menu bar artwork, as a macOS template image: black pixels carrying the
/// drawing in their alpha channel and nothing in their colour, which is what
/// lets macOS recolour one asset for a light and a dark menu bar. Paired with
/// `icon_as_template(true)` below, without which macOS renders the black
/// literally and the icon disappears into a dark menu bar, which is what the
/// bundle icon used to do here.
///
/// The 2x raster is the one that ships. `tray-icon` sizes the `NSImage` to 18
/// points whatever its pixel dimensions, so a 36x36 asset lands one image pixel
/// on one device pixel at the 2x scale every current Mac display uses.
/// `tray-icon.png` next to it is the same drawing at 1x; both come out of
/// `generate-tray-icon.mjs`, which is the source the artwork can be changed in.
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray/tray-icon@2x.png");

/// Shown next to the icon while a failure is waiting to be read.
///
/// The tray title is the only part of the menu bar item that can change without
/// the user opening anything, so it is what carries "look at me". One character
/// because the menu bar charges for every one of them.
const FAILURE_MARKER: &str = "!";

/// The tooltip while nothing has gone wrong.
const IDLE_TOOLTIP: &str = "Snapdeck";

/// The menu line standing in for a failure when there has not been one.
///
/// Present rather than hidden so that the place failures will appear is
/// discoverable before the first one, which is the whole point of putting them
/// in the menu instead of in a banner that is gone in five seconds.
const NO_FAILURE_TEXT: &str = "No recent problems";

/// Longest text rendered on one line of the menu, in characters.
///
/// A macOS menu item neither wraps nor scrolls, so past this the menu grows
/// wider than it is worth. Both things this menu renders are as long as they
/// like: a failure message is a whole sentence, and a capture's name comes out
/// of the user's own filename template. A cut failure message is still readable
/// in full in the tooltip, which does wrap, and in the log the next item opens.
const MENU_MESSAGE_LIMIT: usize = 72;

/// The submenu holding the captures the user has just taken.
const RECENT_CAPTURES_TEXT: &str = "Recent Captures";

/// The menu line standing in for a capture when there has not been one.
///
/// A disabled line inside the submenu rather than no submenu at all, for the
/// reason `NO_FAILURE_TEXT` is present: a menu that grows an entry the first
/// time something happens is a menu whose entry nobody was looking for. Kept
/// disabled because there is nothing behind it to reveal.
const NO_RECENT_CAPTURES_TEXT: &str = "No recent captures";

/// The front of every recent capture's menu id, followed by that capture's path
/// in hexadecimal.
///
/// The path itself rather than its position in the list, because the list can
/// be rebuilt while the menu the user is reading is on screen. An editor save
/// runs on a worker thread, a multi-megabyte `write_all` and `sync_all` take
/// long enough to open a menu in, and the main-thread hop the rebuild uses is
/// delivered on `kCFRunLoopCommonModes`, which `NSEventTrackingRunLoopMode`
/// belongs to: the submenu really is rewritten under an open menu. A position
/// would then name whatever moved into it, and clicking would reveal a file the
/// user did not point at. A path names one file whenever it is read.
const RECENT_CAPTURE_ID_PREFIX: &str = "recent_capture_";

/// The submenu the recent captures are rendered into.
///
/// Managed for the reason `FailureSurface` is: the handle has to outlive
/// `build_tray` so that a capture finishing on a worker thread can rebuild the
/// list without the tray having handed anything to `recents`.
struct RecentCaptures(Submenu<Wry>);

/// The parts of the tray that a failed capture writes to.
///
/// Managed rather than global so that the handles live exactly as long as the
/// application does, and so that `report` can find them from any thread without
/// this module having to hand them to it.
struct FailureSurface {
    tray: TrayIcon,
    last_failure: MenuItem<Wry>,
}

/// Single entry point for every capture request, from the tray or a shortcut.
/// Returns immediately: `open_overlays` moves the capture to a worker thread
/// and reports its own failures, so this call must stay on the main thread but
/// never blocks it.
pub fn request_capture(app: &AppHandle, mode: &str) {
    // A new capture makes the last failure history. Clearing it here, in the
    // one place every capture goes through, is also what keeps the marker from
    // being permanent: nothing else in the application knows that a capture
    // succeeded.
    clear_failure(app);
    crate::overlay::open_overlays(app, mode);
}

/// Puts a failed capture in the menu bar, where the user is already looking.
///
/// Three places at once, because they answer different questions: the title
/// says something happened, the tooltip says what, and the menu line says what
/// while the user is in the menu on their way to the log. Every write is
/// best-effort; the caller is reporting a failure and has nowhere to report a
/// failure to report it.
pub fn show_failure(app: &AppHandle, message: &str) {
    let Some(surface) = app.try_state::<FailureSurface>() else {
        return;
    };
    let _ = surface.tray.set_title(Some(FAILURE_MARKER));
    let _ = surface.tray.set_tooltip(Some(message));
    let _ = surface.last_failure.set_text(shorten(message));
}

/// Returns the tray to its quiet state.
fn clear_failure(app: &AppHandle) {
    let Some(surface) = app.try_state::<FailureSurface>() else {
        return;
    };
    let _ = surface.tray.set_title(None::<&str>);
    let _ = surface.tray.set_tooltip(Some(IDLE_TOOLTIP));
    let _ = surface.last_failure.set_text(NO_FAILURE_TEXT);
}

/// Cuts `message` down to something a menu can render on one line.
///
/// Counts characters rather than bytes: the messages carry file paths, and a
/// byte-wise cut through a multi-byte path would not be valid UTF-8.
fn shorten(message: &str) -> String {
    if message.chars().count() <= MENU_MESSAGE_LIMIT {
        return message.to_string();
    }
    let kept: String = message.chars().take(MENU_MESSAGE_LIMIT - 1).collect();
    format!("{}…", kept.trim_end())
}

/// Opens the log holding the full text of every failure.
///
/// Reveals the file when there is one and falls back to its directory, which is
/// created here if the first failure has not created it yet, so the item is
/// never a click that does nothing. `open` is spawned rather than waited on:
/// this runs on the main thread, inside the menu event handler.
fn reveal_log(app: &AppHandle) {
    let Some(file) = crate::report::log_file_path(app) else {
        return;
    };
    if file.is_file() {
        reveal_in_finder(&file);
        return;
    }
    let Some(directory) = file.parent() else {
        return;
    };
    if std::fs::create_dir_all(directory).is_err() {
        return;
    }
    let _ = std::process::Command::new("open").arg(directory).spawn();
}

/// Selects `path` in the Finder without opening it.
///
/// `-R` is the whole of the difference, and it is the point: opening a capture
/// hands it to Preview, or to whatever the user has bound to PNG, which is not
/// what "show me where it went" asks for. Spawned rather than waited on,
/// because this runs on the main thread inside the menu event handler.
fn reveal_in_finder(path: &Path) {
    let _ = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn();
}

/// Shows the capture at `index` in the Finder, or says why it cannot.
///
/// A capture can be gone by the time it is clicked: the list is pruned when it
/// is loaded and whenever it changes, but the user can delete a file in the
/// seconds between opening the menu and choosing an item, and nothing tells the
/// application about it. `open -R` on a missing file fails where nobody can see
/// it, which would make this the one item in the menu that silently does
/// nothing, so the failure is reported the way every other failure in this
/// application is, and the entry is taken out rather than left to fail again.
fn reveal_recent_capture(app: &AppHandle, path: &Path) {
    if !path.is_file() {
        crate::report::report_failure(
            app,
            &format!(
                "Snapdeck could not show that capture in the Finder: there is no longer a file at {}. It has been taken out of Recent Captures.",
                path.display()
            ),
        );
        crate::recents::forget_missing(app, path);
        return;
    }
    reveal_in_finder(path);
}

/// The menu id for one recent capture: the prefix and the path's own bytes in
/// hexadecimal.
///
/// Lossless and reversible, which a position in the list is not and a rendered
/// string is not either. `OsStr::as_bytes` is the whole path exactly as the
/// filesystem holds it, so a name macOS accepts and Unicode does not survives
/// the trip, and hexadecimal keeps the id to the `[A-Za-z0-9_]` alphabet a menu
/// id is comfortable with.
fn recent_capture_id(path: &Path) -> String {
    let bytes = path.as_os_str().as_bytes();
    let mut id = String::with_capacity(RECENT_CAPTURE_ID_PREFIX.len() + bytes.len() * 2);
    id.push_str(RECENT_CAPTURE_ID_PREFIX);
    for byte in bytes {
        // Into a `String`, which cannot fail to be written to.
        let _ = write!(id, "{byte:02x}");
    }
    id
}

/// The capture a menu id names, or `None` for every other item in the menu.
fn recent_capture_path(id: &str) -> Option<PathBuf> {
    let hex = id.strip_prefix(RECENT_CAPTURE_ID_PREFIX)?;
    // Both halves are load-bearing. An odd number of digits is not a whole byte,
    // and `u8::from_str_radix` accepts a leading `+`, so without the digit check
    // `+f` would decode rather than being refused.
    if hex.is_empty() || hex.len() % 2 != 0 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_bytes().chunks_exact(2) {
        let pair = std::str::from_utf8(pair).ok()?;
        bytes.push(u8::from_str_radix(pair, 16).ok()?);
    }
    Some(PathBuf::from(OsString::from_vec(bytes)))
}

/// What one recent capture is called in the menu.
///
/// The file name rather than the path: the folder is the same for all of them
/// until the user changes it, and a menu item wide enough for a home directory
/// is a menu item nobody can read. `shorten` for the reason a failure message
/// gets it, since the name is the user's template's and can be any length.
///
/// Two entries can therefore read the same. A filename template without `{date}`
/// or `{time}` in it renders one name for every capture, and once the save
/// folder has moved, two files in two folders under that one name are two lines
/// in this menu that nobody can tell apart; a name long enough to be shortened
/// loses its extension to the ellipsis and can collide the same way. Accepted
/// rather than fixed: the alternative is a menu item carrying a folder path,
/// which is unreadable for every user who never moved their save folder, and the
/// item still reveals the right file when it is clicked, because the id carries
/// the path and not the label.
fn menu_label(path: &Path) -> String {
    let name = path.file_name().unwrap_or(path.as_os_str());
    shorten(&name.to_string_lossy())
}

/// Rebuilds the Recent Captures submenu from `captures`.
///
/// The whole submenu each time rather than the lines that changed: the list is
/// at most `recents::RECENT_CAPTURE_LIMIT` items, and every one of them can
/// move when a single capture is added, since a capture already in the list
/// moves to the front and pushes the rest down. An id carries its own path, so
/// there is no item whose text can be changed in place without its id becoming
/// a lie.
///
/// Every write is best-effort, as in `show_failure`: this runs on the back of a
/// capture the user already has, and a menu that could not be rebuilt is not
/// something to take a screenshot away for.
///
/// A full rebuild is sixteen hops to the main thread for five captures: one
/// removal per item, the removal that finds the submenu empty, and a build and
/// an append for each. Measured in a release build, called from a worker thread
/// as `record` calls it, best of 100 after a warm-up: 0.9 ms median, 1.2 ms at
/// the 95th percentile, with a 15.6 ms outlier while the application was still
/// starting up. Once per saved capture, off the main thread, against a capture
/// path that has just spent hundreds of milliseconds in ScreenCaptureKit and an
/// encoder, so it is not worth a cheaper scheme: there is no in-place update
/// available anyway, since an item's id is derived from its path and a capture
/// moving to the front changes which path every line below it holds.
pub fn refresh_recent_captures(app: &AppHandle, captures: &[PathBuf]) {
    let Some(menu) = app.try_state::<RecentCaptures>() else {
        return;
    };
    // Drained one at a time rather than by a counted loop over `items()`, which
    // is what makes a failure safe: an `Err` here means the submenu is not in
    // the state the rest of this was going to build on, and appending to a list
    // that could not be emptied is how the menu ends up showing every capture
    // twice. Leaving it as it is says one stale thing; the alternative says two
    // contradictory ones. It is also one main-thread round trip cheaper than
    // asking for the items first.
    loop {
        match menu.0.remove_at(0) {
            Ok(Some(_)) => continue,
            Ok(None) => break,
            Err(_) => return,
        }
    }
    if captures.is_empty() {
        if let Ok(item) = no_recent_captures(app) {
            let _ = menu.0.append(&item);
        }
        return;
    }
    for path in captures {
        if let Ok(item) =
            MenuItemBuilder::with_id(recent_capture_id(path), menu_label(path)).build(app)
        {
            let _ = menu.0.append(&item);
        }
    }
}

/// The placeholder line for an empty list.
fn no_recent_captures(app: &AppHandle) -> tauri::Result<MenuItem<Wry>> {
    MenuItemBuilder::with_id("no_recent_captures", NO_RECENT_CAPTURES_TEXT)
        .enabled(false)
        .build(app)
}

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let region = MenuItemBuilder::with_id("capture_region", "Capture Region").build(app)?;
    let window = MenuItemBuilder::with_id("capture_window", "Capture Window").build(app)?;
    let display = MenuItemBuilder::with_id("capture_display", "Capture Full Screen").build(app)?;
    // Built with the placeholder already in it, so the submenu is never an
    // empty rectangle in the seconds before `recents::restore` fills it and
    // never becomes one if a later launch cannot read the file.
    let recent_captures = SubmenuBuilder::with_id(app, "recent_captures", RECENT_CAPTURES_TEXT)
        .item(&no_recent_captures(app)?)
        .build()?;
    let last_failure = MenuItemBuilder::with_id("last_failure", NO_FAILURE_TEXT)
        .enabled(false)
        .build(app)?;
    let open_log = MenuItemBuilder::with_id("open_log", "Open Log").build(app)?;
    // The ellipsis is the platform's own promise that the item opens something
    // rather than doing something, which is what Apple's own menus mean by it.
    let settings = MenuItemBuilder::with_id("settings", "Settings…").build(app)?;
    // The visible half of the update story, and the reason the automatic check
    // can stay off by default: an update is always one menu item away, so
    // nobody has to leave a network call switched on to get one. The ellipsis
    // is honest here too, since the check ends in a dialog either way.
    let check_for_updates =
        MenuItemBuilder::with_id("check_for_updates", "Check for Updates…").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Snapdeck").build(app)?;
    // The order the design spec asks for: capture actions, recent captures,
    // then the rest. Its own group, because it answers a different question
    // from the three items above it.
    let menu = MenuBuilder::new(app)
        .items(&[&region, &window, &display])
        .separator()
        .item(&recent_captures)
        .separator()
        .items(&[&last_failure, &open_log])
        .separator()
        .items(&[&check_for_updates, &settings, &quit])
        .build()?;

    let tray = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .icon(Image::from_bytes(TRAY_ICON)?)
        .icon_as_template(true)
        .tooltip(IDLE_TOOLTIP)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "capture_region" => request_capture(app, "region"),
            "capture_window" => request_capture(app, "window"),
            "capture_display" => request_capture(app, "display"),
            "open_log" => reveal_log(app),
            "check_for_updates" => crate::updater::check_on_request(app),
            "settings" => crate::settings_window::open_settings(app),
            "quit" => app.exit(0),
            other => {
                if let Some(path) = recent_capture_path(other) {
                    reveal_recent_capture(app, &path);
                }
            }
        })
        .build(app)?;

    app.manage(FailureSurface { tray, last_failure });
    app.manage(RecentCaptures(recent_captures));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_message_is_left_alone() {
        assert_eq!(
            shorten("Snapdeck could not save it."),
            "Snapdeck could not save it."
        );
    }

    #[test]
    fn a_long_message_is_cut_to_the_limit() {
        let shortened = shorten(&"a".repeat(MENU_MESSAGE_LIMIT + 10));
        assert_eq!(shortened.chars().count(), MENU_MESSAGE_LIMIT);
        assert!(shortened.ends_with('…'));
    }

    #[test]
    fn a_cut_falls_between_characters_and_not_inside_one() {
        // A path full of multi-byte characters is the case a byte-wise cut
        // would turn into something that is not a string at all.
        let shortened = shorten(&"ö".repeat(MENU_MESSAGE_LIMIT + 10));
        assert_eq!(shortened.chars().count(), MENU_MESSAGE_LIMIT);
    }

    /// The whole point of the id carrying the path: what is clicked is the file
    /// the item was built for, whatever the list has done in the meantime.
    ///
    /// The last path is the case a rendered id could not carry. macOS accepts
    /// any byte but `/` and NUL in a file name, so a capture folder named by
    /// something other than Unicode is a real path, and a lossy round trip would
    /// turn it into a menu item naming a file that does not exist.
    #[test]
    fn a_recent_capture_id_is_the_path_it_was_built_from() {
        let non_unicode = PathBuf::from(OsString::from_vec(vec![
            b'/', b's', b'h', b'o', b't', b's', b'/', 0xff, 0xfe, b'.', b'p', b'n', b'g',
        ]));
        for path in [
            PathBuf::from("/Pictures/Capture 2026-09-07 at 04.05.06.png"),
            PathBuf::from("/Pictures/bir ekran görüntüsü.jpg"),
            non_unicode,
        ] {
            let id = recent_capture_id(&path);
            assert_eq!(recent_capture_path(&id).as_deref(), Some(path.as_path()));
        }
    }

    /// Two captures are two ids, or a click could not tell them apart at all.
    #[test]
    fn two_captures_do_not_share_an_id() {
        assert_ne!(
            recent_capture_id(Path::new("/Pictures/a.png")),
            recent_capture_id(Path::new("/Pictures/b.png"))
        );
    }

    /// Every other item in the menu goes through the same arm, and none of them
    /// may be read as a capture: `quit` resolving to a path would reveal a file
    /// instead of quitting. Nor may a malformed id, which is what an odd number
    /// of digits, a non-digit and a sign character are here: `u8::from_str_radix`
    /// accepts a leading `+`, so `+f` would otherwise decode to a byte.
    #[test]
    fn no_other_menu_item_is_read_as_a_recent_capture() {
        for id in [
            "capture_region",
            "open_log",
            "settings",
            "quit",
            "no_recent_captures",
            "recent_captures",
            "recent_capture_",
            "recent_capture_abc",
            "recent_capture_zz",
            "recent_capture_+f",
            "recent_capture_ 1",
        ] {
            assert_eq!(recent_capture_path(id), None, "{id}");
        }
    }

    /// The menu shows the file, not the folder it is in: the folder is the same
    /// for every entry, and a path is wider than a menu.
    #[test]
    fn a_capture_is_labelled_with_its_file_name() {
        assert_eq!(
            menu_label(Path::new(
                "/Users/someone/Pictures/Capture 2026-09-07 at 04.05.06.png"
            )),
            "Capture 2026-09-07 at 04.05.06.png"
        );
    }

    /// The name comes out of the user's own filename template, so it can be as
    /// long as they like, and a macOS menu item neither wraps nor scrolls.
    #[test]
    fn a_long_file_name_is_cut_to_the_menu_limit() {
        let name = format!("{}.png", "a".repeat(MENU_MESSAGE_LIMIT + 10));
        let label = menu_label(&Path::new("/Pictures").join(&name));
        assert_eq!(label.chars().count(), MENU_MESSAGE_LIMIT);
        assert!(label.ends_with('…'), "{label}");
    }
}
