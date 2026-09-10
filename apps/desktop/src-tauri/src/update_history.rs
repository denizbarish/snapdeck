//! The highest version of Snapdeck this installation has ever run, so that a
//! manifest cannot walk it backwards.
//!
//! `updater::is_upgrade` compares a manifest against the version the *running*
//! bundle reports, and that is the whole of the question only for as long as the
//! running bundle is the newest one this machine has had. The minisign signature
//! covers the archive bytes and nothing else: `version`, `notes`, `url` and
//! `pub_date` are not signed. Whoever controls the endpoint can therefore put
//! any version number they like next to a URL naming an older archive this
//! project genuinely signed, and the download verifies, because it really is
//! ours.
//!
//! The high-water mark closes the half of that which this side can close. The
//! highest version this installation has ever run is written down, it only ever
//! goes up, and every check compares a manifest against it rather than against
//! whichever bundle happens to be on disk. An installation that has been 0.5.0
//! never accepts a manifest claiming less than 0.5.0 again, however it came to
//! be running something older.
//!
//! Which is a claim about manifests, not about archives, and the difference is
//! the whole of what this buys. The comparison is against the version a manifest
//! *says* it is serving, so what the mark stops is the honest downgrade: an
//! endpoint that offers 0.1.0 as 0.1.0 is refused, once and every time after.
//! An endpoint that offers a genuinely signed 0.1.0 archive under a `99.0.0`
//! entry clears any floor, because 99.0.0 beats it, and clears it again on the
//! next check and the one after that. The mark does not make a rollback
//! unrepeatable; it makes an unlabelled one the only kind worth trying.
//!
//! What it does not buy is stated where the promise is, in the `updater` module
//! doc: a manifest that lies *upward* is not caught here. Nothing in a manifest
//! is signed, and nothing compares the unpacked bundle against the version that
//! was promised, so a `99.0.0` entry pointing at a real 0.1.0 archive still
//! passes both the comparator and the signature check. The mark is the floor,
//! not a proof of what arrived.
//!
//! Nothing here is fatal. A mark that cannot be read gives the same answer as no
//! mark at all, the running version, and a mark that cannot be written costs the
//! memory of this run rather than the update: the alternative is refusing to
//! check for updates because a file in the configuration directory is unhappy,
//! which is worse than the thing it would be protecting against.

use std::path::{Path, PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// The file inside `app_config_dir()`, beside `settings.json`.
///
/// Its own file rather than a field of `Settings`, because it is not a setting:
/// the user did not choose it, the settings window may not show it, and a hand
/// edit of `settings.json` that drops a line must not be able to drop this.
const HISTORY_FILE_NAME: &str = "update-history.json";

/// What the file holds.
///
/// The version is a string rather than a `Version` so that a file written by
/// some later build, with a value this one cannot parse, is a field to ignore
/// rather than a parse error: `floor` answers an unreadable mark with the
/// running version, which is the safe direction to be wrong in.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct History {
    highest_installed_version: String,
}

/// The version a manifest has to beat, with the running build folded in and
/// written down.
///
/// Read and write in one call, on purpose. The mark has to be recorded on some
/// path, and the check is the only path on which it can matter: a machine that
/// never checks for updates cannot be moved by a manifest, and one that does
/// records the version it was on before it is offered anything.
pub fn high_water_mark(app: &AppHandle, current: &Version) -> Version {
    let Some(path) = history_path(app) else {
        return current.clone();
    };
    let stored = read(&path);
    let mark = floor(stored.as_deref(), current);
    let rendered = mark.to_string();
    if stored.as_deref() != Some(rendered.as_str()) {
        if let Err(err) = write(&path, &rendered) {
            crate::report::append_to_log(
                app,
                &format!("Snapdeck could not record the version it is running ({err})."),
            );
        }
    }
    mark
}

/// The stored mark as written, or `None` when there is nothing usable to read.
///
/// Every failure is the same answer. A first run has no file, a corrupt file has
/// no mark in it, and neither is a reason to say anything: the caller falls back
/// to the running version, which is what a fresh installation would have written
/// anyway.
fn read(path: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    let history: History = serde_json::from_str(&contents).ok()?;
    Some(history.highest_installed_version)
}

/// The higher of a stored mark and the running version, which is the floor a
/// manifest has to clear.
///
/// A stored mark that does not parse is treated as absent rather than as zero,
/// which is the same thing here and is worth saying: the fallback has to be the
/// running version, because falling back to something lower would let a manifest
/// offer a downgrade to any machine whose history file was scribbled on.
///
/// Visible to the crate so that `updater` can state the composition it depends
/// on in a test: a floor from here, handed to `is_upgrade`, refuses the release
/// that the running bundle on its own would have accepted.
pub(crate) fn floor(stored: Option<&str>, current: &Version) -> Version {
    match stored.and_then(|raw| Version::parse(raw).ok()) {
        Some(stored) if stored > *current => stored,
        _ => current.clone(),
    }
}

/// Replaces the mark, or leaves the one that is there.
///
/// `commands::write_atomically` rather than `std::fs::write`, which truncates:
/// it opens the file with `O_TRUNC`, so the recorded mark is gone before the
/// replacement is written and a crash, a full disk or a power cut in between
/// leaves an empty or half-written file. `read` answers that with `None`, `floor`
/// answers `None` with the running version, and the running version is the one
/// number a rollback has already lowered. This is the one file in the
/// application where losing the old contents has a security consequence rather
/// than an inconvenient one, and a temporary file renamed into place is what
/// makes the mark either the old one or the whole new one.
fn write(path: &Path, mark: &str) -> Result<(), String> {
    let history = History {
        highest_installed_version: mark.to_string(),
    };
    let directory = path
        .parent()
        .ok_or_else(|| format!("{} has no directory", path.display()))?;
    std::fs::create_dir_all(directory)
        .map_err(|err| format!("failed to create {}: {err}", directory.display()))?;
    let json = serde_json::to_string_pretty(&history)
        .map_err(|err| format!("failed to render the update history: {err}"))?;
    crate::commands::write_atomically(path, json.as_bytes())
}

fn history_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|directory| directory.join(HISTORY_FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(raw: &str) -> Version {
        Version::parse(raw).expect("the test's own version literal must parse")
    }

    /// The first run: nothing has been recorded, so the running build is the
    /// floor and there is nothing else this could sensibly be.
    #[test]
    fn no_stored_mark_is_the_running_version() {
        assert_eq!(floor(None, &v("0.1.0")), v("0.1.0"));
    }

    /// The case the mark exists for. The bundle on disk says 0.1.0 because a
    /// manifest already walked this installation back to it; the mark says the
    /// machine has been 0.5.0, and 0.5.0 is what the next manifest has to beat.
    #[test]
    fn a_higher_stored_mark_wins_over_the_running_version() {
        assert_eq!(floor(Some("0.5.0"), &v("0.1.0")), v("0.5.0"));
    }

    /// The ordinary case, and the one that keeps the file from freezing an
    /// installation: an update that has been installed raises the mark rather
    /// than being held under it.
    #[test]
    fn the_running_version_wins_when_it_is_the_higher_one() {
        assert_eq!(floor(Some("0.1.0"), &v("0.5.0")), v("0.5.0"));
        assert_eq!(floor(Some("0.5.0"), &v("0.5.0")), v("0.5.0"));
    }

    /// A mark that cannot be read is the same answer as no mark. It may not be
    /// treated as zero: a scribbled-on history file would then be a way to make
    /// an installation accept a downgrade.
    #[test]
    fn an_unreadable_mark_falls_back_to_the_running_version() {
        for scribble in ["", "not a version", "0.1", "v0.5.0", "{}"] {
            assert_eq!(
                floor(Some(scribble), &v("0.3.0")),
                v("0.3.0"),
                "{scribble:?} must not lower the floor"
            );
        }
    }

    /// A corrupt file is a missing file, all the way through `read`. This is the
    /// half `higher_of` cannot see: a file that is not JSON, or JSON without the
    /// field in it, must not be a reason to fail a check.
    #[test]
    fn a_corrupt_file_reads_as_nothing_recorded() {
        let directory = std::env::temp_dir().join(format!(
            "snapdeck-update-history-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&directory).expect("the test's own temporary directory");
        let path = directory.join(HISTORY_FILE_NAME);

        assert_eq!(read(&path), None, "a file that is not there yet");

        for corrupt in ["", "{", "not json at all", r#"{"somethingElse": "0.5.0"}"#] {
            std::fs::write(&path, corrupt).expect("write the corrupt file");
            assert_eq!(read(&path), None, "{corrupt:?}");
        }

        // And the other half: a file this build wrote reads back as what it
        // wrote, or the mark would never survive a restart.
        write(&path, "0.5.0").expect("write the mark");
        assert_eq!(read(&path), Some("0.5.0".to_string()));

        std::fs::remove_dir_all(&directory).ok();
    }

    /// The name the file is written under. Renaming a field silently is how a
    /// recorded mark stops being read back, which would quietly remove the
    /// floor from every installation that upgrades past it.
    #[test]
    fn the_file_is_written_in_camel_case() {
        let json = serde_json::to_string(&History {
            highest_installed_version: "0.5.0".to_string(),
        })
        .expect("render");
        assert!(json.contains("highestInstalledVersion"), "{json}");
    }
}
