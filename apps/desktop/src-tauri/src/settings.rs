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
//! a permission change, so `resolve_save_directory` checks it and falls back to
//! the pictures directory when it cannot be used, saying so, rather than letting
//! the capture fail.

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
    #[serde(deserialize_with = "shortcuts_or_default")]
    pub shortcuts: Shortcuts,
    pub launch_at_login: bool,
    pub open_editor_after_capture: bool,
    /// Whether to ask the release endpoint for a newer version at launch.
    ///
    /// The only setting in here that decides whether this application talks to
    /// the network at all, which is why it is a setting rather than a
    /// behaviour. See `updater`.
    pub check_for_updates_at_launch: bool,
    /// The token the extension has to present. Empty until the first launch
    /// that mints one.
    ///
    /// The one field in here the user does not choose, and the one the window
    /// shows rather than edits: it is a secret, and the only thing that may
    /// replace it is a fresh one from the system CSPRNG. It lives in the
    /// settings because it has to survive a relaunch, which is the whole of
    /// what pairing means; a token minted at every launch pairs with nothing.
    pub bridge_token: String,
}

/// Reads a `shortcuts` object, filling any binding it does not name from the
/// defaults.
///
/// The nested half of the container's own `default`. Without it, a `shortcuts`
/// object with one binding missing is a parse error, and a parse error here is
/// not local: `load` answers a bad file with `Settings::default()`, so one
/// absent line costs the user their save folder, their template, their format
/// and their login item as well. That is a punishment out of all proportion to
/// a hand edit, and it contradicts the rule the container states, that anything
/// a file is missing comes from one `Settings::default()`.
///
/// This does not recurse, which was the objection to giving `Shortcuts` a
/// `Default` impl and is worth spelling out. `Settings::default` builds its
/// `Shortcuts` from a struct literal and never asks serde for one, so nothing
/// here re-enters this function. `Shortcuts` still has no `Default` of its own:
/// the defaults are still written in exactly one place.
///
/// Unknown fields are still refused. A binding that is *absent* is a file from
/// an older build or a hand edit that dropped a line, and filling it is the
/// kind thing to do; a binding that is *misspelled* is a file that says
/// something this cannot honour, and silently ignoring it would put a shortcut
/// the user believes they set nowhere at all.
fn shortcuts_or_default<'de, D>(deserializer: D) -> Result<Shortcuts, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct PartialShortcuts {
        capture_region: Option<String>,
        capture_window: Option<String>,
        capture_display: Option<String>,
    }

    let partial = PartialShortcuts::deserialize(deserializer)?;
    let defaults = Settings::default().shortcuts;
    Ok(Shortcuts {
        capture_region: partial.capture_region.unwrap_or(defaults.capture_region),
        capture_window: partial.capture_window.unwrap_or(defaults.capture_window),
        capture_display: partial.capture_display.unwrap_or(defaults.capture_display),
    })
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
            // Off, and the one default in here that is a promise rather than a
            // preference: Snapdeck makes no network connection the user did not
            // ask for, and a check that runs because nobody turned it off is
            // one they did not ask for. `Check for Updates…` in the menu bar
            // works whatever this says.
            check_for_updates_at_launch: false,
            // Not a default so much as the absence of one, and the comment on
            // `ensure_bridge_token` is the reason: a constant here would be the
            // same pairing secret on every installation in the world.
            bridge_token: String::new(),
        }
    }
}

/// The token in force, minting and storing one the first time.
///
/// Not a `Default`: a default is a constant, and a constant pairing token would
/// be the same secret on every installation in the world. The default is "none
/// yet", and the first launch is what mints one.
///
/// The write is part of it. A token that is only in memory pairs the extension
/// for exactly as long as this process lives, which is the bug this replaced:
/// the bridge minted one at every launch, so an extension paired yesterday was
/// refused today and there was no way to pair it at all.
///
/// The token is set on `settings` whether or not the write succeeds, and the
/// failure is passed back rather than swallowed. The bridge then works for this
/// run, the caller says why the pairing will not survive a relaunch, and
/// nothing pretends a secret was written down when it was not.
pub fn ensure_bridge_token(app: &AppHandle, settings: &mut Settings) -> Result<String, String> {
    if mint_if_missing(crate::bridge::token::generate_token, settings)? {
        save(app, settings)?;
    }
    Ok(settings.bridge_token.clone())
}

/// The decision inside `ensure_bridge_token`, with the mint injected, and
/// whether it minted anything.
///
/// Split out for the reason `choose_save_directory` is: everything around it
/// needs a live `AppHandle`, and this one line is the whole of what the feature
/// turns on. `false` is what stops the save: a token already in the file is not
/// a token to write again.
fn mint_if_missing<M>(mint: M, settings: &mut Settings) -> Result<bool, String>
where
    M: FnOnce() -> Result<String, String>,
{
    if !settings.bridge_token.is_empty() {
        return Ok(false);
    }
    settings.bridge_token = mint()?;
    Ok(true)
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
/// Read and written by its owner, and by nobody else on the machine.
#[cfg(unix)]
const OWNER_ONLY: u32 = 0o600;

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
    write_privately(&path, json.as_bytes())
}

/// Writes a file only its owner can read.
///
/// The settings hold the bridge's pairing token, and the default mode leaves it
/// readable by every account on the machine.
///
/// Both halves are needed and the tests only prove the second. `mode` applies
/// when this call creates the file, which is what keeps a new file from
/// existing under the wider mode while the bytes are written; it does nothing
/// to a file that is already there, and a settings file written by an older
/// build is already there, holding a token.
fn write_privately(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(OWNER_ONLY);
    }
    let mut file = options
        .open(path)
        .map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    #[cfg(unix)]
    {
        // An existing file keeps the mode it already had: `mode` only applies to
        // a file this call creates. A settings file written by an older build
        // is still holding a token now.
        use std::os::unix::fs::PermissionsExt;
        let _ = file.set_permissions(std::fs::Permissions::from_mode(OWNER_ONLY));
    }
    file.write_all(bytes)
        .map_err(|err| format!("failed to write {}: {err}", path.display()))
}

/// What `apply` did, and what it could not do.
///
/// Three answers rather than one, because the shortcuts are not like the rest of
/// the form and pretending they are is what made a save impossible to complete.
pub struct Applied {
    /// What the platform has bound once `apply` returns.
    ///
    /// The caller has to record this whether the save succeeded or not: a
    /// rollback can leave a different set bound than the one that went in, and
    /// nothing else is in a position to notice.
    pub bound: Option<Shortcuts>,
    /// Why the bindings are not the ones `next` asked for, or `None` when they
    /// are.
    ///
    /// The caller's cue to write the *stored* shortcuts back rather than the
    /// requested ones, and to say so; see `commands::save_settings`.
    pub shortcuts_refused: Option<String>,
    /// Whether the rest of the change went into force.
    pub outcome: Result<(), String>,
}

/// Puts `next` into force, and says which parts of it could not be.
///
/// The login item is all or nothing: it is either on or off, this application
/// is the one that decides, and a failure there means nothing may be written.
///
/// The shortcuts are not, and treating them as though they were is the trap this
/// signature exists to get out of. A combination another application holds
/// cannot be registered no matter how many times it is tried, and it stays in
/// the settings file waiting to be changed. When a refusal was fatal, every
/// later save was refused with it: somebody whose stored shortcut had been taken
/// by an application they do not control could not change their save folder,
/// their filename template or anything else until they resolved a conflict that
/// was not theirs to resolve. So a refused rebind is now reported rather than
/// thrown: `shortcuts_refused` carries the reason, the previous bindings are
/// back in force by the time it is set, and the caller keeps the stored
/// shortcuts and writes the rest.
///
/// `registered` is what the platform has actually accepted, which is not the
/// same question as what `previous` says; see `shortcuts::rebind`.
pub fn apply(
    app: &AppHandle,
    registered: Option<&Shortcuts>,
    previous: &Settings,
    next: &Settings,
) -> Applied {
    apply_with(
        |from, to| crate::shortcuts::rebind(app, from, to),
        |enabled| set_launch_at_login(app, enabled),
        || crate::shortcuts::unregister_shortcuts(app),
        registered,
        previous,
        next,
    )
}

/// The decisions inside `apply`, with the two side effects injected.
///
/// Split out for the reason `shortcuts::rebind_with` is: neither case worth
/// testing can be arranged through the live platform. A combination macOS will
/// refuse is not something a test can set up, and a login item that fails to be
/// written needs a system that refuses to write it.
fn apply_with<R, L, U>(
    mut rebind: R,
    mut set_login: L,
    mut unregister: U,
    registered: Option<&Shortcuts>,
    previous: &Settings,
    next: &Settings,
) -> Applied
where
    R: FnMut(Option<&Shortcuts>, &Shortcuts) -> (Option<Shortcuts>, Result<(), String>),
    L: FnMut(bool) -> Result<(), String>,
    U: FnMut() -> Result<(), String>,
{
    let (bound, outcome) = rebind(registered, &next.shortcuts);
    // `rebind` has already put back whatever it could, so this is a reason to
    // report and not a state to recover from.
    let shortcuts_refused = outcome.err();
    if next.launch_at_login == previous.launch_at_login {
        return Applied {
            bound,
            shortcuts_refused,
            outcome: Ok(()),
        };
    }
    let Err(err) = set_login(next.launch_at_login) else {
        return Applied {
            bound,
            shortcuts_refused,
            outcome: Ok(()),
        };
    };
    // Nothing is written when this returns an error, so the running application
    // has to go back to the bindings it had before this call. Back to
    // `registered` rather than to `previous.shortcuts`, because those two are
    // the same thing only when the stored set was registerable in the first
    // place.
    let Some(restore_to) = registered else {
        // There were no bindings before this, so putting things back means
        // taking down the ones that were just registered.
        return match unregister() {
            Ok(()) => Applied {
                bound: None,
                shortcuts_refused,
                outcome: Err(err),
            },
            Err(restore_err) => Applied {
                bound,
                shortcuts_refused,
                outcome: Err(format!(
                    "{err}. The shortcuts registered along the way could not be taken back down either ({restore_err})."
                )),
            },
        };
    };
    let (restored, restore) = rebind(bound.as_ref(), restore_to);
    match restore {
        Ok(()) => Applied {
            bound: restored,
            shortcuts_refused,
            outcome: Err(err),
        },
        Err(restore_err) => Applied {
            bound: restored,
            shortcuts_refused,
            outcome: Err(format!(
                "{err}. The previous shortcuts could not be put back either ({restore_err})."
            )),
        },
    }
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

/// Proves that captures can be written to `directory`, by writing one, and
/// creates the directory if it is not there yet.
///
/// A real file, not a permissions bit: a read-only volume, an ACL, a full disk
/// and a sandbox refusal all answer differently to a metadata read and
/// identically to this. The probe is created exclusively, so it cannot collide
/// with anything, and it is removed again whatever happens next.
///
/// For the two places a folder is *chosen*, the picker and the save, and for
/// nowhere else. It is the expensive answer, and it has a side effect: a folder
/// that is not there is created. That is right when the user has just named it
/// and wrong on the capture path, which asks `is_writable` instead.
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

/// Whether `directory` looks usable right now, changing nothing.
///
/// The capture path's question, and deliberately a weaker one than
/// `ensure_writable` asks. Proving it the expensive way on every capture costs
/// a `create_dir_all`, a `create_new` and an unlink while the user is waiting,
/// which on iCloud Drive or a network mount is latency they feel, and the
/// `create_dir_all` is worse than slow: a folder the user deleted would be
/// silently put back under them, one capture at a time.
///
/// A metadata read can say yes and be wrong, on a folder somebody else owns
/// with the traversal bits set. That is affordable exactly here and nowhere
/// else: the write that follows fails, `capture_and_write` reports it, and the
/// picture is on the clipboard either way. Being wrong the other way, on a
/// folder that has genuinely gone away, is what the fallback is for.
pub fn is_writable(directory: &Path) -> Result<(), String> {
    let metadata = std::fs::metadata(directory)
        .map_err(|err| format!("{} cannot be used: {err}", directory.display()))?;
    if !metadata.is_dir() {
        return Err(format!("{} is not a folder", directory.display()));
    }
    if metadata.permissions().readonly() {
        return Err(format!("{} is read-only", directory.display()));
    }
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
///
/// Shared with `updater::takes_a_write`, which asks the same question about the
/// folder Snapdeck is installed in and must not answer it with a second naming
/// scheme: two probe names is two things to recognise in a folder listing after
/// a crash.
pub(crate) fn probe_name() -> String {
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
    // `is_writable`, not `ensure_writable`: this runs on the capture path, and
    // the folder was already proved the expensive way when it was chosen.
    choose_save_directory(settings.save_directory.as_deref(), &fallback, is_writable)
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

    /// The rule the module exists to keep. Every module of the crate except
    /// this one is read here, in the form it is compiled from, and none of them
    /// may contain a default.
    ///
    /// A source scan rather than an equality assertion, because the bug this is
    /// about is a *second* copy: two values that agree today, one of which is
    /// changed later. `assert_eq!(commands::TEMPLATE, settings.template)` would
    /// pass right up until the moment it stopped mattering.
    ///
    /// All thirteen, not the seven with an obvious reason to state one.
    /// `editor.rs`, `overlay.rs` and `report.rs` have no such reason today,
    /// which is not a reason to leave them unread: the list is a rule about the
    /// crate, and a rule with three holes in it is where the next copy goes.
    /// Every module added to the crate joins it, which is why `updater.rs` and
    /// `update_history.rs` are here.
    ///
    /// `main.rs` too, and it is the one that had been left out. It is the
    /// binary's own root rather than a module of this library, which is exactly
    /// why it is easy to forget and exactly why it is worth reading: a constant
    /// put there would be outside every other check in this file. It is four
    /// lines today and this keeps it that way.
    const SOURCES: [(&str, &str); 13] = [
        ("commands.rs", include_str!("commands.rs")),
        ("editor.rs", include_str!("editor.rs")),
        ("lib.rs", include_str!("lib.rs")),
        ("main.rs", include_str!("main.rs")),
        ("output.rs", include_str!("output.rs")),
        ("overlay.rs", include_str!("overlay.rs")),
        ("report.rs", include_str!("report.rs")),
        ("settings_window.rs", include_str!("settings_window.rs")),
        ("shortcuts.rs", include_str!("shortcuts.rs")),
        ("state.rs", include_str!("state.rs")),
        ("tray.rs", include_str!("tray.rs")),
        ("update_history.rs", include_str!("update_history.rs")),
        ("updater.rs", include_str!("updater.rs")),
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
        // The one default that is a promise: a file written before this setting
        // existed must not turn a network check on for somebody who never asked
        // for one.
        assert!(!settings.check_for_updates_at_launch);
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
            // A misspelled binding. A file that says something this cannot
            // honour, as opposed to one that leaves a binding out: filling it
            // in silently would leave the user with a shortcut they believe
            // they set and nothing bound to it.
            r#"{"shortcuts": {"captureRegionn": "CmdOrCtrl+Shift+KeyA"}}"#,
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

    /// A `shortcuts` object with a binding left out keeps the ones it does
    /// name, and costs the user nothing else.
    ///
    /// The nested case of the rule the container already follows for every
    /// top-level field. Before this, one absent line made the whole file
    /// unreadable, which took the save folder, the template, the format and the
    /// login item with it.
    #[test]
    fn a_shortcuts_object_missing_a_binding_keeps_the_rest_of_the_file() {
        let (settings, complaint) = settings_from_json(
            r#"{"saveDirectory": "/Volumes/Shots",
                "shortcuts": {"captureRegion": "CmdOrCtrl+Shift+KeyA"}}"#,
        );
        assert_eq!(complaint, None);
        assert_eq!(settings.shortcuts.capture_region, "CmdOrCtrl+Shift+KeyA");
        let defaults = Settings::default().shortcuts;
        assert_eq!(settings.shortcuts.capture_window, defaults.capture_window);
        assert_eq!(settings.shortcuts.capture_display, defaults.capture_display);
        assert_eq!(
            settings.save_directory,
            Some(PathBuf::from("/Volumes/Shots")),
            "the rest of the file must survive one missing binding"
        );
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
            check_for_updates_at_launch: true,
            bridge_token: "0123456789abcdef".to_string(),
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
            "checkForUpdatesAtLaunch",
            "captureRegion",
        ] {
            assert!(
                json.contains(&format!("\"{key}\"")),
                "missing {key} in {json}"
            );
        }
        assert!(json.contains("\"png\""), "the format is lower case: {json}");
    }

    /// The settings hold the bridge's pairing token. On a Mac with more than
    /// one account, the default mode hands that secret to everyone with a
    /// login.
    #[test]
    fn the_settings_file_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;

        let directory = std::env::temp_dir().join(format!(
            "snapdeck-settings-mode-{}-{}",
            std::process::id(),
            NEXT_PROBE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).expect("create the fixture");
        let path = directory.join("settings.json");

        write_privately(&path, b"{}").expect("write the settings");

        let mode = std::fs::metadata(&path)
            .expect("read the mode")
            .permissions()
            .mode();
        std::fs::remove_dir_all(&directory).expect("clean up");
        assert_eq!(mode & 0o777, OWNER_ONLY);
    }

    /// A file written by an older build carries the wider mode, and the token
    /// went into it on the next save.
    #[test]
    fn an_existing_settings_file_is_narrowed_when_it_is_written_again() {
        use std::os::unix::fs::PermissionsExt;

        let directory = std::env::temp_dir().join(format!(
            "snapdeck-settings-widen-{}-{}",
            std::process::id(),
            NEXT_PROBE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).expect("create the fixture");
        let path = directory.join("settings.json");
        std::fs::write(&path, b"{}").expect("write the fixture");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("widen the fixture");

        write_privately(&path, b"{}").expect("write the settings");

        let mode = std::fs::metadata(&path)
            .expect("read the mode")
            .permissions()
            .mode();
        std::fs::remove_dir_all(&directory).expect("clean up");
        assert_eq!(mode & 0o777, OWNER_ONLY);
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

    /// The capture path's question, and the two things it must not do: create
    /// the folder the user deleted, and say yes about a folder that is not
    /// there.
    #[test]
    fn the_capture_path_check_creates_nothing_and_refuses_what_is_gone() {
        let root = temp_dir("is-writable");
        let present = root.join("present");
        std::fs::create_dir_all(&present).expect("create the fixture");
        is_writable(&present).expect("a directory that exists and is writable must be accepted");

        let deleted = root.join("deleted");
        let err = is_writable(&deleted).expect_err("a folder that is not there cannot be used");
        assert!(err.contains("deleted"), "the error should name it: {err}");
        assert!(
            !deleted.exists(),
            "the capture path must not put the user's deleted folder back"
        );

        // A file where a folder should be: the same shape as a folder replaced
        // by a download of the same name.
        let not_a_folder = root.join("file");
        std::fs::write(&not_a_folder, b"").expect("create the fixture");
        let err = is_writable(&not_a_folder).expect_err("a file is not a save folder");
        assert!(err.contains("not a folder"), "{err}");

        std::fs::remove_dir_all(&root).ok();
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

    /// A rebind that never fails, for the cases that are not about the
    /// shortcuts.
    fn accepts(_: Option<&Shortcuts>, to: &Shortcuts) -> (Option<Shortcuts>, Result<(), String>) {
        (Some(to.clone()), Ok(()))
    }

    /// A rebind the platform refuses, which leaves what was already bound in
    /// force. That second half is what `shortcuts::rebind` guarantees and what
    /// makes a refusal survivable at all.
    fn refuses(from: Option<&Shortcuts>, _: &Shortcuts) -> (Option<Shortcuts>, Result<(), String>) {
        (
            from.cloned(),
            Err(
                "the region shortcut is already taken. Your previous shortcuts are still in force."
                    .to_string(),
            ),
        )
    }

    fn folder_changed(previous: &Settings) -> Settings {
        Settings {
            save_directory: Some(PathBuf::from("/Volumes/Shots")),
            ..previous.clone()
        }
    }

    /// The trap this whole signature exists to get out of, in one test. A
    /// stored combination another application holds cannot be registered, and
    /// it must not therefore refuse a save that has nothing to do with it: the
    /// save folder change goes through, and the reason the keyboard did not
    /// change comes back to be shown.
    #[test]
    fn a_save_that_only_changes_the_folder_survives_a_shortcut_another_app_holds() {
        let previous = Settings::default();
        let fallback = Shortcuts {
            capture_region: "CmdOrCtrl+Alt+KeyR".to_string(),
            ..previous.shortcuts.clone()
        };

        let applied = apply_with(
            refuses,
            |_| panic!("the login item did not change and must not be touched"),
            || panic!("nothing was rolled back"),
            Some(&fallback),
            &previous,
            &folder_changed(&previous),
        );

        applied
            .outcome
            .expect("a shortcut somebody else holds may not refuse the rest of the form");
        let refusal = applied
            .shortcuts_refused
            .expect("and the user has to be told why the keyboard did not change");
        assert!(refusal.contains("already taken"), "{refusal}");
        assert_eq!(
            applied.bound,
            Some(fallback),
            "what was working has to still be working"
        );
    }

    /// The same refusal must not stop the rest of the form reaching the system
    /// either: a login item toggled in the same save is still applied.
    #[test]
    fn a_refused_rebind_does_not_hold_back_the_login_item() {
        let previous = Settings::default();
        let next = Settings {
            launch_at_login: !previous.launch_at_login,
            ..folder_changed(&previous)
        };
        let mut asked_for = None;

        let applied = apply_with(
            refuses,
            |enabled| {
                asked_for = Some(enabled);
                Ok(())
            },
            || panic!("nothing failed, so nothing is rolled back"),
            Some(&previous.shortcuts),
            &previous,
            &next,
        );

        applied.outcome.expect("the login item was accepted");
        assert!(applied.shortcuts_refused.is_some());
        assert_eq!(asked_for, Some(next.launch_at_login));
    }

    /// The guarantee that did not change. A login item the system refuses is
    /// not a partial save: nothing is written, and the bindings go back to what
    /// they were before the call.
    #[test]
    fn a_refused_login_item_refuses_the_save_and_puts_the_bindings_back() {
        let previous = Settings::default();
        let next = Settings {
            launch_at_login: !previous.launch_at_login,
            shortcuts: Shortcuts {
                capture_region: "CmdOrCtrl+Alt+KeyR".to_string(),
                ..previous.shortcuts.clone()
            },
            ..previous.clone()
        };

        let applied = apply_with(
            accepts,
            |_| Err("Snapdeck could not add its login item.".to_string()),
            || panic!("there were bindings to go back to"),
            Some(&previous.shortcuts),
            &previous,
            &next,
        );

        let err = applied
            .outcome
            .expect_err("a login item that cannot be written refuses the save");
        assert!(err.contains("login item"), "{err}");
        assert_eq!(
            applied.bound,
            Some(previous.shortcuts),
            "the shortcuts this save registered have to come back off"
        );
        assert_eq!(applied.shortcuts_refused, None);
    }

    /// The same failure with nothing bound to go back to: the rollback is a
    /// removal, not a restore, and the answer has to say the keyboard is empty.
    #[test]
    fn a_refused_login_item_with_nothing_bound_takes_the_new_bindings_down() {
        let previous = Settings::default();
        let next = Settings {
            launch_at_login: !previous.launch_at_login,
            ..previous.clone()
        };
        let mut unregistered = false;

        let applied = apply_with(
            accepts,
            |_| Err("Snapdeck could not add its login item.".to_string()),
            || {
                unregistered = true;
                Ok(())
            },
            None,
            &previous,
            &next,
        );

        applied.outcome.expect_err("the save is refused");
        assert!(
            unregistered,
            "the bindings this save added have to come off"
        );
        assert_eq!(applied.bound, None);
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

    /// `default_format` decides what a capture is written in, and the editor
    /// has to open on that same format or Save stops meaning what it says.
    ///
    /// The failure is worse than an extra file. With `Jpeg` stored, the capture
    /// is `X.jpg`; an editor that opened on PNG saves to `X.png`, which
    /// `resolve_save_target` accepts because only the extension differs, and
    /// `X.jpg` stays on disk. A user who redacted a secret and saved has then
    /// produced a second file and kept the unredacted first one, which is the
    /// opposite of what they asked for.
    ///
    /// Read from the source for the reason the extensions are: this is one
    /// decision written in two languages, and the only thing that can keep them
    /// together is one of them reading the other. The opening format must be
    /// derived from the capture's own path, and there must be no second default
    /// in the editor package for it to drift back towards.
    #[test]
    fn the_editor_opens_on_the_format_the_capture_is_already_in() {
        let window = include_str!("../../src/editor/EditorWindow.tsx");
        assert!(
            window.contains("initialFormat={formatForPath(path)}"),
            "the editor window no longer opens on the capture's own format, so a save of a capture in the other format would leave the original beside it"
        );
        let editor = include_str!("../../../../packages/editor/src/Editor.tsx");
        assert!(
            !editor.contains("DEFAULT_FORMAT"),
            "the editor package states an opening format of its own again; the format a capture is in is the host's answer, not the component's"
        );
    }

    /// T1. Security. The pairing token is the one secret the bridge has, and a
    /// default is a constant: a constant here would be the same secret on every
    /// installation in the world, and pairing would prove nothing at all. The
    /// default is "none yet", and the first launch is what mints one.
    #[test]
    fn the_default_settings_carry_no_pairing_token() {
        assert!(
            Settings::default().bridge_token.is_empty(),
            "a default pairing token would be a shared secret, not a secret"
        );
    }

    /// T2. The property the whole field exists for. A token minted once stays
    /// minted, so the extension the user paired yesterday is still paired
    /// today; before this, every launch generated a fresh one and the bridge
    /// could not be paired at all.
    #[test]
    fn a_token_is_minted_once_and_then_left_alone() {
        let mut settings = Settings::default();

        let minted = mint_if_missing(|| Ok("f00d".to_string()), &mut settings)
            .expect("a mint that answers cannot fail");
        assert!(minted, "an empty token is what a first launch has to fill");
        assert_eq!(settings.bridge_token, "f00d");

        let minted_again = mint_if_missing(|| Ok("beef".to_string()), &mut settings)
            .expect("a mint that answers cannot fail");
        assert!(
            !minted_again,
            "a token already in the file is not a token to mint"
        );
        assert_eq!(
            settings.bridge_token, "f00d",
            "the second launch has to present the token the extension was paired with"
        );
    }

    /// T3. A token that is not written down is a token the next launch does not
    /// have. The file is the only place it lives.
    #[test]
    fn the_pairing_token_survives_the_file() {
        let written = Settings {
            bridge_token: "0123456789abcdef".to_string(),
            ..Settings::default()
        };
        let json = serde_json::to_string(&written).expect("render");
        let (read_back, complaint) = settings_from_json(&json);
        assert_eq!(complaint, None);
        assert_eq!(
            read_back.bridge_token, "0123456789abcdef",
            "a token the file does not carry is one the next launch mints again"
        );
    }

    /// T4. A settings file written before this field existed is a file from
    /// every user who already has Snapdeck. It has to load as it is, with an
    /// empty token that the launch then mints, and it must not cost them the
    /// folder and the template they chose.
    #[test]
    fn a_file_written_before_the_token_existed_keeps_everything_else() {
        let (settings, complaint) = settings_from_json(
            r#"{"saveDirectory": "/Volumes/Shots", "filenameTemplate": "shot-{time}"}"#,
        );
        assert_eq!(complaint, None);
        assert!(
            settings.bridge_token.is_empty(),
            "a file that names no token has none yet"
        );
        assert_eq!(
            settings.save_directory,
            Some(PathBuf::from("/Volumes/Shots")),
            "a new field must not cost the user the settings they already had"
        );
        assert_eq!(settings.filename_template, "shot-{time}");
    }
}
