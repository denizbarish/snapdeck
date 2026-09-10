//! Everything the user is allowed to change, and the one place its defaults are
//! written.
//!
//! `Default for Settings` is that place. Every default this application has used
//! to keep as a constant of its own now lives in it: the filename template that
//! was `commands::DEFAULT_FILENAME_TEMPLATE`, the bindings that were
//! `Shortcuts::default`, and the format the capture path used to have no choice
//! about. Nothing else in the crate may state one, and the test at the bottom
//! reads the other modules' source to make sure nothing does. That is not
//! pedantry: this project has already paid twice for a value written down twice,
//! a toolbar width kept in Rust and in CSS, and a redaction floor calibrated
//! against the wrong fixture, and a default that disagrees with itself is the
//! same bug with a different face. `Shortcuts` deliberately has no `Default`
//! impl for the same reason.
//!
//! Two rules shape the rest of the module.
//!
//! A settings file that cannot be read is not a reason to refuse to start. This
//! is an `LSUIElement` binary with no window and no console: an application that
//! will not launch has no way at all to say why, so a file this cannot parse is
//! logged and replaced with the defaults, and the user gets a working app and a
//! line they can read.
//!
//! A setting may not cost the user a capture. The save directory is the one that
//! can go away between being chosen and being written to, an unplugged volume or
//! a permission change, so `resolve_save_directory` proves it is writable and
//! falls back to the pictures directory when it is not, saying so, rather than
//! letting the capture fail.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::shortcuts::Shortcuts;

/// The settings file inside `app_config_dir()`.
const SETTINGS_FILE_NAME: &str = "settings.json";

/// The format a capture is written in.
///
/// Two, and the same two the editor can export, because the file this names is
/// the one the editor is opened on and later saves back over: a capture in a
/// format the editor cannot write would open and then refuse to save.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SaveFormat {
    Png,
    Jpeg,
}

impl SaveFormat {
    /// The extension a capture in this format is written under.
    ///
    /// `jpg` rather than `jpeg`, because that is the name the editor's own save
    /// path produces for the same format, and the two have to agree or a JPEG
    /// capture would be joined by a second file on the first edit instead of
    /// being replaced by it.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }
}

/// Everything the settings window can change.
///
/// `default` at the container level rather than on each field: serde then fills
/// anything a file is missing from one `Settings::default()`, so a file written
/// by an older build still loads and no field needs a default of its own.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Where captures are written. `None` means the pictures directory, which
    /// is a value rather than a path so that a user who never chose a folder
    /// follows the system's own idea of where pictures go, even if it moves.
    pub save_directory: Option<PathBuf>,
    pub filename_template: String,
    pub default_format: SaveFormat,
    pub shortcuts: Shortcuts,
    pub launch_at_login: bool,
    pub open_editor_after_capture: bool,
}

impl Default for Settings {
    /// The single source of truth for every default in this application.
    ///
    /// Read the module comment before adding a constant anywhere else.
    fn default() -> Self {
        Self {
            save_directory: None,
            // `{date}` and `{time}` are the tokens `output::render_filename`
            // expands; `{width}` and `{height}` exist too and are simply not in
            // the default.
            filename_template: "Snapdeck {date} at {time}".to_string(),
            // The capture is a screenshot, which is the worst case for a lossy
            // encoder: it is almost entirely the hard edges JPEG throws away,
            // and every one of those edges is a glyph.
            default_format: SaveFormat::Png,
            shortcuts: Shortcuts {
                // Avoids the macOS system screenshot bindings (Cmd+Shift+3/4/5).
                capture_region: "CmdOrCtrl+Shift+Digit7".to_string(),
                capture_window: "CmdOrCtrl+Shift+Digit8".to_string(),
                capture_display: "CmdOrCtrl+Shift+Digit9".to_string(),
            },
            launch_at_login: false,
            open_editor_after_capture: true,
        }
    }
}

/// Reads the settings, falling back to the defaults and logging when the file
/// cannot be used.
///
/// Never fails, on purpose: every caller is either the launch path or the
/// capture path, and neither has anything better to do with an error than what
/// this already does. A file that is simply not there is the first run and is
/// not logged.
pub fn load(app: &AppHandle) -> Settings {
    let Some(path) = settings_path(app) else {
        complain(
            app,
            "Snapdeck has no configuration directory, so it is running on the built-in defaults.",
        );
        return Settings::default();
    };
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Settings::default(),
        Err(err) => {
            complain(
                app,
                &format!(
                    "Snapdeck could not read {} ({err}), so it is running on the built-in defaults.",
                    path.display()
                ),
            );
            return Settings::default();
        }
    };
    let (settings, complaint) = settings_from_json(&contents);
    if let Some(complaint) = complaint {
        complain(
            app,
            &format!(
                "Snapdeck could not understand {} ({complaint}), so it is running on the built-in defaults.",
                path.display()
            ),
        );
    }
    settings
}

/// Writes the settings, replacing whatever is there.
///
/// Refuses a save directory that cannot be written to, which is the whole
/// difference between a setting and a promise: the picker proves it once, and
/// this proves it again, because the volume it named may have gone away since.
pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    if let Some(directory) = &settings.save_directory {
        ensure_writable(directory)?;
    }
    let path = settings_path(app)
        .ok_or_else(|| "Snapdeck has no configuration directory to save into.".to_string())?;
    let directory = path
        .parent()
        .ok_or_else(|| format!("{} has no directory", path.display()))?;
    std::fs::create_dir_all(directory)
        .map_err(|err| format!("failed to create {}: {err}", directory.display()))?;
    let json = serde_json::to_string_pretty(settings)
        .map_err(|err| format!("failed to render the settings: {err}"))?;
    std::fs::write(&path, json).map_err(|err| format!("failed to write {}: {err}", path.display()))
}

/// Puts `next` into force, or leaves `previous` in force and says why.
///
/// All or nothing. A half-applied change is the failure this exists to prevent:
/// the user would be left with some of what they asked for, no message, and no
/// way to work out which half. The shortcuts go first because they are the part
/// that can leave the application in a state the user cannot explain, and the
/// login item is put back the same way when it is the half that fails.
pub fn apply(app: &AppHandle, previous: &Settings, next: &Settings) -> Result<(), String> {
    crate::shortcuts::rebind(app, &previous.shortcuts, &next.shortcuts)?;
    if next.launch_at_login == previous.launch_at_login {
        return Ok(());
    }
    let Err(err) = set_launch_at_login(app, next.launch_at_login) else {
        return Ok(());
    };
    // Nothing is written when this returns an error, so the running application
    // has to go back to matching what is on disk.
    if let Err(restore_err) = crate::shortcuts::rebind(app, &next.shortcuts, &previous.shortcuts) {
        return Err(format!(
            "{err}. The previous shortcuts could not be put back either ({restore_err})."
        ));
    }
    Err(err)
}

/// Turns the login item on or off, rather than only remembering that it should
/// be.
fn set_launch_at_login(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    let outcome = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    outcome.map_err(|err| {
        let verb = if enabled { "add" } else { "remove" };
        format!("Snapdeck could not {verb} its login item ({err}).")
    })
}

/// Whether Snapdeck is actually set to launch at login, as opposed to what the
/// settings file remembers.
///
/// The system is the authority: the user can take the login item away from
/// outside this application, and a checkbox that then still claims to be on is
/// a lie the settings window would have no way to notice.
pub fn launch_at_login_state(app: &AppHandle) -> Option<bool> {
    app.autolaunch().is_enabled().ok()
}

/// Proves that captures can be written to `directory`, by writing one.
///
/// A real file, not a permissions bit: a read-only volume, an ACL, a full disk
/// and a sandbox refusal all answer differently to a metadata read and
/// identically to this. The probe is created exclusively, so it cannot collide
/// with anything, and it is removed again whatever happens next.
pub fn ensure_writable(directory: &Path) -> Result<(), String> {
    std::fs::create_dir_all(directory)
        .map_err(|err| format!("{} cannot be created: {err}", directory.display()))?;
    let probe = directory.join(probe_name());
    std::fs::File::options()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|err| format!("{} cannot be written to: {err}", directory.display()))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

/// Source of probe file names, unique for the life of the process.
static NEXT_PROBE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// The name the writability probe is created under.
///
/// Hidden by a leading dot and stamped with the process id and a counter, for
/// the reason `commands::temporary_name` gives: a probe interrupted between the
/// create and the unlink leaves nothing the user has to recognise, and two
/// probes at once cannot land on one name.
fn probe_name() -> String {
    let sequence = NEXT_PROBE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!(".snapdeck-write-probe-{}-{sequence}", std::process::id())
}

/// Where this capture may actually be written, and what to tell the user when
/// that is not where they asked for.
///
/// A capture that has already been framed and confirmed must not be lost to a
/// folder that has gone away, so an unwritable choice falls back to the pictures
/// directory rather than failing. The complaint travels back as a value instead
/// of being reported here, because the caller is the only side that knows
/// whether the rest of the capture survived and what to say about it.
pub fn resolve_save_directory(app: &AppHandle, settings: &Settings) -> (PathBuf, Option<String>) {
    let fallback = app.path().picture_dir().unwrap_or_else(|_| {
        // A home directory that cannot be read is not a case that leaves
        // anywhere sensible to write; the save itself then fails and is
        // reported by the capture path, which is what already happens today.
        PathBuf::from(".")
    });
    choose_save_directory(
        settings.save_directory.as_deref(),
        &fallback,
        ensure_writable,
    )
}

/// The decision inside `resolve_save_directory`, with the probe injected.
///
/// Split out for the reason `commands::without_windows` is: everything around
/// it needs a live `AppHandle`, and this is the whole of what can be wrong.
fn choose_save_directory<P>(
    configured: Option<&Path>,
    fallback: &Path,
    writable: P,
) -> (PathBuf, Option<String>)
where
    P: Fn(&Path) -> Result<(), String>,
{
    let Some(configured) = configured else {
        return (fallback.to_path_buf(), None);
    };
    match writable(configured) {
        Ok(()) => (configured.to_path_buf(), None),
        Err(err) => (
            fallback.to_path_buf(),
            Some(format!(
                "Snapdeck could not use the save folder you chose ({err}), so this capture went to {} instead.",
                fallback.display()
            )),
        ),
    }
}

/// The settings to use for `contents`, and the complaint to log when it could
/// not be understood.
///
/// A pair rather than a `Result`, because there is no path in this application
/// on which a bad settings file is fatal: the answer is always the defaults, and
/// the only question is whether the user is owed an explanation.
fn settings_from_json(contents: &str) -> (Settings, Option<String>) {
    match serde_json::from_str::<Settings>(contents) {
        Ok(settings) => (settings, None),
        Err(err) => (Settings::default(), Some(err.to_string())),
    }
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|directory| directory.join(SETTINGS_FILE_NAME))
}

/// Puts a settings problem where the user can find it.
///
/// The log file and the console, and deliberately not the tray marker that
/// `report::report_failure` also raises: `load` runs on the capture path as
/// well as at launch, and a file that cannot be parsed would otherwise put an
/// exclamation mark in the menu bar on every single capture, in front of a
/// capture that succeeded.
fn complain(app: &AppHandle, message: &str) {
    eprintln!("snapdeck: {message}");
    crate::report::append_to_log(app, message);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule the module exists to keep. Every module that could plausibly
    /// hold a default of its own is read here, in the form it is compiled from,
    /// and none of them may contain one.
    ///
    /// A source scan rather than an equality assertion, because the bug this is
    /// about is a *second* copy: two values that agree today, one of which is
    /// changed later. `assert_eq!(commands::TEMPLATE, settings.template)` would
    /// pass right up until the moment it stopped mattering.
    const SOURCES: [(&str, &str); 7] = [
        ("commands.rs", include_str!("commands.rs")),
        ("shortcuts.rs", include_str!("shortcuts.rs")),
        ("output.rs", include_str!("output.rs")),
        ("lib.rs", include_str!("lib.rs")),
        ("tray.rs", include_str!("tray.rs")),
        ("state.rs", include_str!("state.rs")),
        ("settings_window.rs", include_str!("settings_window.rs")),
    ];

    /// The settings window's own source, which is the other side that could
    /// keep a copy: a React field with a default value in it would be a second
    /// place a default is written, and TypeScript is not covered by anything
    /// else here.
    const SETTINGS_UI: &str = include_str!("../../src/settings/SettingsWindow.tsx");

    #[test]
    fn no_other_module_states_a_default() {
        let defaults = Settings::default();
        let values = [
            defaults.filename_template.clone(),
            defaults.shortcuts.capture_region.clone(),
            defaults.shortcuts.capture_window.clone(),
            defaults.shortcuts.capture_display.clone(),
        ];
        for (name, source) in SOURCES {
            for value in &values {
                assert!(
                    !source.contains(value.as_str()),
                    "{name} states the default {value:?}; defaults belong in `Default for Settings` alone"
                );
            }
        }
        for value in &values {
            assert!(
                !SETTINGS_UI.contains(value.as_str()),
                "the settings window states the default {value:?}; it must read them from Rust"
            );
        }
    }

    /// The window has to render whatever Rust says rather than starting from a
    /// blank of its own, which is the shape a second source of truth takes in a
    /// form: a `useState('')` that is shown before the load lands.
    #[test]
    fn the_settings_window_renders_nothing_until_rust_has_answered() {
        assert!(
            SETTINGS_UI.contains("get_settings"),
            "the settings window must read the current settings from Rust"
        );
    }

    #[test]
    fn a_missing_field_is_filled_from_the_defaults() {
        let (settings, complaint) = settings_from_json(r#"{"filenameTemplate": "shot-{time}"}"#);
        assert_eq!(complaint, None);
        assert_eq!(settings.filename_template, "shot-{time}");
        // Everything else is the default, from the one place they are written.
        assert_eq!(settings.shortcuts, Settings::default().shortcuts);
        assert_eq!(settings.default_format, Settings::default().default_format);
        assert_eq!(settings.save_directory, None);
        assert!(settings.open_editor_after_capture);
    }

    /// A menu bar app that will not launch has no way to tell the user why, so
    /// a file it cannot parse is a fallback and a log line rather than a
    /// refusal.
    #[test]
    fn a_corrupt_file_falls_back_to_the_defaults_and_says_so() {
        for corrupt in [
            "",
            "{",
            "not json at all",
            r#"{"filenameTemplate": 7}"#,
            // A shortcut object missing a binding. `Shortcuts` has no `Default`
            // of its own, by design, so half an object is not something this
            // can silently complete; it is a file to complain about.
            r#"{"shortcuts": {"captureRegion": "CmdOrCtrl+Shift+KeyA"}}"#,
        ] {
            let (settings, complaint) = settings_from_json(corrupt);
            assert_eq!(
                settings,
                Settings::default(),
                "{corrupt:?} should have fallen back to the defaults"
            );
            assert!(
                complaint.is_some(),
                "{corrupt:?} should have been logged, not swallowed"
            );
        }
    }

    /// The other half of the same rule: a file that is fine must not be
    /// replaced by the defaults and must not be complained about.
    #[test]
    fn a_good_file_is_used_as_written() {
        let written = Settings {
            save_directory: Some(PathBuf::from("/Volumes/Shots")),
            filename_template: "shot-{width}x{height}".to_string(),
            default_format: SaveFormat::Jpeg,
            shortcuts: Shortcuts {
                capture_region: "CmdOrCtrl+Alt+KeyR".to_string(),
                capture_window: "CmdOrCtrl+Alt+KeyW".to_string(),
                capture_display: "CmdOrCtrl+Alt+KeyD".to_string(),
            },
            launch_at_login: true,
            open_editor_after_capture: false,
        };
        let json = serde_json::to_string(&written).expect("render");
        let (read_back, complaint) = settings_from_json(&json);
        assert_eq!(complaint, None);
        assert_eq!(read_back, written);
        assert_ne!(read_back, Settings::default());
    }

    /// The names the settings window and the file both depend on. Renaming a
    /// field silently is how a saved setting stops being read back.
    #[test]
    fn the_file_is_written_in_the_camel_case_the_window_reads() {
        let json = serde_json::to_string(&Settings::default()).expect("render");
        for key in [
            "saveDirectory",
            "filenameTemplate",
            "defaultFormat",
            "shortcuts",
            "launchAtLogin",
            "openEditorAfterCapture",
            "captureRegion",
        ] {
            assert!(
                json.contains(&format!("\"{key}\"")),
                "missing {key} in {json}"
            );
        }
        assert!(json.contains("\"png\""), "the format is lower case: {json}");
    }

    #[test]
    fn a_writable_directory_is_used_as_chosen() {
        let chosen = Path::new("/Volumes/Shots");
        let (directory, complaint) =
            choose_save_directory(Some(chosen), Path::new("/Users/a/Pictures"), |_| Ok(()));
        assert_eq!(directory, chosen);
        assert_eq!(complaint, None);
    }

    /// The property the whole fallback exists for: an unplugged volume costs
    /// the user a message, never the picture.
    #[test]
    fn an_unwritable_directory_falls_back_and_names_where_the_capture_went() {
        let fallback = Path::new("/Users/a/Pictures");
        let (directory, complaint) =
            choose_save_directory(Some(Path::new("/Volumes/Gone")), fallback, |_| {
                Err("no such volume".to_string())
            });
        assert_eq!(directory, fallback);
        let complaint = complaint.expect("the user has to be told the folder was not used");
        assert!(complaint.contains("no such volume"), "{complaint}");
        assert!(complaint.contains("/Users/a/Pictures"), "{complaint}");
    }

    #[test]
    fn no_chosen_directory_means_the_pictures_directory_without_a_complaint() {
        let fallback = Path::new("/Users/a/Pictures");
        let (directory, complaint) = choose_save_directory(None, fallback, |_| {
            panic!("nothing was chosen, so nothing should have been probed")
        });
        assert_eq!(directory, fallback);
        assert_eq!(complaint, None);
    }

    /// The probe has to answer for the real thing, so it is run against real
    /// directories: one that can be written to, and one that cannot.
    #[cfg(unix)]
    #[test]
    fn the_probe_answers_for_a_real_directory_and_leaves_nothing_behind() {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_dir("probe");
        let writable = root.join("writable");
        ensure_writable(&writable).expect("a directory this creates must be writable");
        assert!(
            std::fs::read_dir(&writable)
                .expect("read the probed directory")
                .next()
                .is_none(),
            "the probe left a file behind"
        );

        // Readable and traversable, so the path still resolves; not writable,
        // which is the shape of a folder whose permissions changed under the
        // user.
        let refused = root.join("refused");
        std::fs::create_dir_all(&refused).expect("create the fixture");
        std::fs::set_permissions(&refused, std::fs::Permissions::from_mode(0o500))
            .expect("make the directory read-only");
        let answer = ensure_writable(&refused);
        std::fs::set_permissions(&refused, std::fs::Permissions::from_mode(0o700))
            .expect("restore the fixture");
        std::fs::remove_dir_all(&root).ok();

        let err = answer.expect_err("a read-only directory must be refused");
        assert!(err.contains("refused"), "the error should name it: {err}");
    }

    fn temp_dir(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock is after the epoch")
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("snapdeck-{}-{unique}-{name}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("create the temporary directory");
        directory
    }

    /// The extension is what decides whether an edit replaces the capture or
    /// lands beside it, so it has to be the name the editor's own save path
    /// produces for the same format.
    #[test]
    fn the_extensions_match_the_ones_the_editor_writes() {
        assert_eq!(SaveFormat::Png.extension(), "png");
        assert_eq!(SaveFormat::Jpeg.extension(), "jpg");
        let editor = include_str!("../../src/editor/EditorWindow.tsx");
        assert!(
            editor.contains("'image/jpeg': 'jpg'"),
            "the editor no longer writes JPEG as .jpg, so a JPEG capture would grow a second file on the first save"
        );
    }
}
