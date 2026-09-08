use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle,
};

// No settings item. There is no settings window to open: the application
// declares no windows and builds none at runtime, so the item could only ever
// look up a "main" window that does not exist and do nothing. The settings
// surface arrives with Plan 4, and the menu item arrives with it.

/// Single entry point for every capture request, from the tray or a shortcut.
/// Returns immediately: `open_overlays` moves the capture to a worker thread
/// and reports its own failures, so this call must stay on the main thread but
/// never blocks it.
pub fn request_capture(app: &AppHandle, mode: &str) {
    crate::overlay::open_overlays(app, mode);
}

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let region = MenuItemBuilder::with_id("capture_region", "Capture Region").build(app)?;
    let window = MenuItemBuilder::with_id("capture_window", "Capture Window").build(app)?;
    let display = MenuItemBuilder::with_id("capture_display", "Capture Full Screen").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Snapdeck").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&region, &window, &display])
        .separator()
        .items(&[&quit])
        .build()?;

    TrayIconBuilder::with_id("main")
        .menu(&menu)
        .icon(
            app.default_window_icon()
                .cloned()
                .ok_or(tauri::Error::UnknownPath)?,
        )
        .on_menu_event(|app, event| match event.id().as_ref() {
            "capture_region" => request_capture(app, "region"),
            "capture_window" => request_capture(app, "window"),
            "capture_display" => request_capture(app, "display"),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}
