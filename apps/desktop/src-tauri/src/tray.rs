//! The menu bar item: the application's only persistent visible surface, and
//! since `report` lost its notification, the only one a failed capture has.

use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItem, MenuItemBuilder},
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

/// Longest failure message rendered in the menu, in characters.
///
/// The messages are whole sentences and a macOS menu item neither wraps nor
/// scrolls, so past this the menu grows wider than it is worth. The untruncated
/// text is in the tooltip, which does wrap, and in the log the next item opens.
const MENU_MESSAGE_LIMIT: usize = 72;

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
    let mut command = std::process::Command::new("open");
    if file.is_file() {
        command.arg("-R").arg(&file);
    } else {
        let Some(directory) = file.parent() else {
            return;
        };
        if std::fs::create_dir_all(directory).is_err() {
            return;
        }
        command.arg(directory);
    }
    let _ = command.spawn();
}

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let region = MenuItemBuilder::with_id("capture_region", "Capture Region").build(app)?;
    let window = MenuItemBuilder::with_id("capture_window", "Capture Window").build(app)?;
    let display = MenuItemBuilder::with_id("capture_display", "Capture Full Screen").build(app)?;
    let last_failure = MenuItemBuilder::with_id("last_failure", NO_FAILURE_TEXT)
        .enabled(false)
        .build(app)?;
    let open_log = MenuItemBuilder::with_id("open_log", "Open Log").build(app)?;
    // The ellipsis is the platform's own promise that the item opens something
    // rather than doing something, which is what Apple's own menus mean by it.
    let settings = MenuItemBuilder::with_id("settings", "Settings…").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Snapdeck").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&region, &window, &display])
        .separator()
        .items(&[&last_failure, &open_log])
        .separator()
        .items(&[&settings, &quit])
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
            "settings" => crate::settings_window::open_settings(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    app.manage(FailureSurface { tray, last_failure });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{shorten, MENU_MESSAGE_LIMIT};

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
}
