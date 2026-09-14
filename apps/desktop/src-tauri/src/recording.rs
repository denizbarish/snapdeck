//! Where a recording's bytes live while it runs, where they land when it
//! finishes, and what is swept up when it never finishes.
//!
//! `SCRecordingOutput` writes an MP4 progressively and puts the `moov` atom in
//! at finalisation. A process that dies mid-recording therefore leaves a file
//! that cannot be played and cannot be repaired. Nothing here can change that;
//! what it can change is what the user is left holding. A recording writes to a
//! hidden name in the save folder, the real name is claimed as a zero-byte
//! placeholder up front so no later capture can take it, and only a successful
//! finalisation renames the one onto the other.
//!
//! No Tauri here: no `AppHandle`, no settings. The folder and the filename stem
//! arrive as arguments, which is what lets every rule below be tested without an
//! application around it.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use snapdeck_capture::{CaptureError, RecordingSummary};
use tauri::AppHandle;

use crate::output;

/// The extension every recording is written under.
pub const RECORDING_EXTENSION: &str = "mp4";

/// What the user is told when a recording ended without a single frame in it.
///
/// A file with no frames is one no player will open, so the honest answer is
/// that there is no movie rather than a movie that fails when they click it.
const NOTHING_RECORDED: &str =
    "Snapdeck did not record a single frame, so there was no movie to save.";

/// The middle of every temporary recording file's name.
///
/// What a sweep recognises as this application's litter, and nothing else in
/// the user's folder may look like it.
const RECORDING_TEMP_MARKER: &str = ".snapdeck-recording-";

/// Source of temporary recording names, unique for the life of the process.
static NEXT_TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Where a recording writes while it runs and where it lands when it finishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingFiles {
    /// The hidden file ScreenCaptureKit writes into. It does not exist yet
    /// when this is built: `AVAssetWriter` refuses a URL that is already
    /// taken, so creating it here would stop every recording from starting.
    pub temporary: PathBuf,
    /// The name the finished movie takes, already claimed with `create_new`
    /// and holding zero bytes until the rename.
    pub final_path: PathBuf,
}

/// The hidden name a recording for `final_name` writes into.
///
/// Derived from the final name rather than random, and that is what makes a
/// sweep precise: a leftover names the placeholder it belongs to, so the sweep
/// can take both away without guessing which zero-byte `.mp4` in the user's
/// folder is its own. The leading dot, the process id and the counter are the
/// shape `commands::temporary_name` already uses, for the same reasons.
fn temporary_name(final_name: &OsStr) -> OsString {
    let sequence = NEXT_TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    // Assembled from bytes rather than through `format!`, because
    // `to_string_lossy` would put replacement characters where a name's
    // non-Unicode bytes were and hand back a name for a file that does not
    // exist. `tray::recent_capture_id` works over `OsStr` for the same reason.
    let mut name = OsString::from(".");
    name.push(final_name);
    name.push(RECORDING_TEMP_MARKER);
    name.push(format!(
        "{}-{sequence}.{RECORDING_EXTENSION}",
        std::process::id()
    ));
    name
}

/// The final name a leftover was going to become, or `None` when the name is
/// not one this application wrote.
fn final_name_of_leftover(leftover: &OsStr) -> Option<OsString> {
    let rest = leftover.as_bytes().strip_prefix(b".")?;
    let rest = rest.strip_suffix(format!(".{RECORDING_EXTENSION}").as_bytes())?;
    // The last marker, not the first: the final name is the user's and may
    // carry the marker inside it, while the one this module appended is always
    // the rightmost.
    let marker = last_index_of(rest, RECORDING_TEMP_MARKER.as_bytes())?;
    let (final_name, tail) = rest.split_at(marker);
    // A name with nothing in front of the marker names no capture, and one with
    // nothing behind it carries no process id and no counter, so neither is a
    // name this module wrote.
    if final_name.is_empty() || tail.len() <= RECORDING_TEMP_MARKER.len() {
        return None;
    }
    Some(OsString::from_vec(final_name.to_vec()))
}

/// Where `needle` last sits in `haystack`.
fn last_index_of(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let last_start = haystack.len().checked_sub(needle.len())?;
    (0..=last_start)
        .rev()
        .find(|&start| &haystack[start..start + needle.len()] == needle)
}

/// Claims both names under `directory`.
///
/// `stem` is what the user's filename template rendered; the ` 2`, ` 3` suffix
/// rule is the still capture's, through `output::claim_free_path`.
pub fn reserve(directory: &Path, stem: &str) -> Result<RecordingFiles, String> {
    let (placeholder, final_path) = output::claim_free_path(directory, stem, RECORDING_EXTENSION)?;
    // Closed straight away and left at zero bytes. The name is what this holds;
    // the movie arrives by rename.
    drop(placeholder);
    let final_name = final_path
        .file_name()
        .ok_or_else(|| format!("{} has no file name", final_path.display()))?;
    Ok(RecordingFiles {
        temporary: directory.join(temporary_name(final_name)),
        final_path,
    })
}

/// Puts the finished movie at its final name.
pub fn commit(files: &RecordingFiles) -> Result<PathBuf, String> {
    // `rename`, never a copy: the two names are in one directory, so this is a
    // directory entry being moved rather than a film being written twice, and
    // the user never sees a half-written `.mp4` under the name they will look
    // for. An error is returned rather than swallowed, because a placeholder
    // left empty is a recording the user is told was lost, while an empty file
    // reported as saved is one they find out about later.
    std::fs::rename(&files.temporary, &files.final_path).map_err(|err| {
        format!(
            "failed to put the recording at {}: {err}",
            files.final_path.display()
        )
    })?;
    Ok(files.final_path.clone())
}

/// Takes both files away. Quiet when either is already gone.
pub fn abandon(files: &RecordingFiles) {
    // Both removals are attempted whatever the first one answers: a cancel
    // arriving before the recording wrote anything, and a second call after a
    // sweep already took the pair, are ordinary rather than exceptional.
    let _ = std::fs::remove_file(&files.temporary);
    let _ = std::fs::remove_file(&files.final_path);
}

/// Deletes this application's recording litter from `directory`, and answers
/// with how many files it took away.
///
/// A crash between the create and the rename leaves a hidden, unfinalised
/// movie in the user's save folder and a zero-byte placeholder next to it
/// under a name that looks like a real capture. Both are this application's
/// mess and neither is something the user should have to recognise.
///
/// `keep` is the live recording's own pair, which a sweep started by the next
/// recording must not touch. A placeholder is only removed when it is **still
/// zero bytes**: a name that has grown a real movie belongs to the user.
pub fn sweep(directory: &Path, keep: Option<&RecordingFiles>) -> usize {
    let Ok(entries) = std::fs::read_dir(directory) else {
        // A save folder that cannot be read is a problem the next recording
        // reports properly; a sweep is housekeeping and says nothing.
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        // `DirEntry::file_type` does not follow a symlink, so a link planted
        // under a leftover's name is not a file here and is left alone. One
        // level only: nothing this module writes goes into a subdirectory.
        if !entry.file_type().is_ok_and(|file_type| file_type.is_file()) {
            continue;
        }
        let Some(final_name) = final_name_of_leftover(&entry.file_name()) else {
            continue;
        };
        let temporary = entry.path();
        if keep.is_some_and(|live| live.temporary == temporary) {
            continue;
        }
        if std::fs::remove_file(&temporary).is_ok() {
            removed += 1;
        }
        let placeholder = directory.join(final_name);
        if keep.is_some_and(|live| live.final_path == placeholder) {
            continue;
        }
        // Still a zero-byte regular file, or it is not a placeholder any more:
        // a name that has grown a movie is the user's capture, and
        // `symlink_metadata` keeps a link at that name from answering for
        // whatever it points at.
        let Ok(metadata) = std::fs::symlink_metadata(&placeholder) else {
            continue;
        };
        if metadata.is_file() && metadata.len() == 0 && std::fs::remove_file(&placeholder).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// What a stopped recording did to the disk.
#[derive(Debug, PartialEq, Eq)]
pub struct Finished {
    /// The movie, or `None` when there is not one to give.
    pub path: Option<PathBuf>,
    /// What the user has to be told, if anything.
    pub complaint: Option<String>,
}

/// Turns a platform result and a pair of files into what the user gets.
///
/// Split out for the reason `bridge::intake::deliver` is: the claim worth
/// testing is what happens to the two files in each outcome, and everything
/// around it needs a live stream and a screen recording grant.
pub fn finish(summary: Result<RecordingSummary, CaptureError>, files: &RecordingFiles) -> Finished {
    match summary {
        // The stream did not come apart cleanly, so whatever is under the
        // temporary name was never finalised and the placeholder names a
        // capture that will not arrive. Both go, and the platform's own words
        // are what the user is given: they are the only description of what
        // went wrong that exists.
        Err(error) => {
            abandon(files);
            Finished {
                path: None,
                complaint: Some(error.to_string()),
            }
        }
        // A recording that took nothing in is a file no player can open. Handing
        // it over would be worse than saying there is nothing: the user finds
        // out at the moment they try to watch it, long after the recording they
        // cannot take again.
        Ok(summary) if summary.frames == 0 => {
            abandon(files);
            Finished {
                path: None,
                complaint: Some(NOTHING_RECORDED.to_string()),
            }
        }
        Ok(_) => match commit(files) {
            Ok(path) => Finished {
                path: Some(path),
                complaint: None,
            },
            // The rename is the only step that makes the movie the user's, so a
            // rename that did not happen leaves nothing worth keeping: the
            // placeholder is an empty file under a name that looks like a
            // capture, which is a thing they would have to open to discover.
            Err(error) => {
                abandon(files);
                Finished {
                    path: None,
                    complaint: Some(error),
                }
            }
        },
    }
}

/// Ends a cancelled recording and leaves nothing on disk.
///
/// The files go whether or not the platform managed to take the stream apart:
/// a cancel that leaves a half-written movie in the user's folder is not a
/// cancel. Answers with what to tell the user, or `None` when there is nothing
/// to tell.
pub fn discard(result: Result<(), CaptureError>, files: &RecordingFiles) -> Option<String> {
    // First, and before the result is even looked at. An early return on a
    // failed tear-down is the one mistake this function exists to not make.
    abandon(files);
    result.err().map(|error| error.to_string())
}

/// Stops the running recording and puts the movie where the user's settings
/// say. Returns immediately; the work is on a blocking worker.
///
/// Body pending: the menu item that calls this is built disabled and nothing
/// in this build can start a recording for it to stop, so the item cannot be
/// clicked. The signature exists now because the tray is wired now.
pub fn request_stop(app: &AppHandle) {
    let _ = app;
}

/// Stops the running recording and leaves nothing on disk. Returns
/// immediately.
///
/// Body pending, as `request_stop` is and for the same reason.
pub fn request_cancel(app: &AppHandle) {
    let _ = app;
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::{SystemTime, UNIX_EPOCH};

    /// A directory of this test's own, so a sweep only ever meets the files the
    /// test wrote itself.
    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after the epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "snapdeck-recording-{}-{unique}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("create the temporary directory");
        directory
    }

    fn file_name(path: &Path) -> &OsStr {
        path.file_name().expect("the path names a file")
    }

    /// R1
    #[test]
    fn a_temporary_name_carries_its_final_name_back() {
        let names = [
            OsString::from("Recording 2026-09-14 at 10.00.00.mp4"),
            OsString::from("Kayıt 2026-09-14 saat 10.00.00.mp4"),
            // Not Unicode at all. macOS accepts every byte but `/` and NUL in a
            // file name, so a name that only survives `to_string_lossy` is a
            // name for a file that does not exist.
            OsString::from_vec(b"Recording \xff\xfe \x80.mp4".to_vec()),
        ];
        for final_name in names {
            let temporary = temporary_name(&final_name);
            assert_eq!(
                final_name_of_leftover(&temporary),
                Some(final_name.clone()),
                "{temporary:?} should carry {final_name:?} back"
            );
        }
    }

    /// R2
    #[test]
    fn a_name_this_application_did_not_write_is_not_litter() {
        for name in [
            "shot.mp4",
            ".hidden.mp4",
            ".snapdeck-recording-.mp4",
            ".a.mp4.snapdeck-recording-1-1.png",
        ] {
            assert_eq!(
                final_name_of_leftover(OsStr::new(name)),
                None,
                "{name} is not this application's litter"
            );
        }
    }

    /// R3
    #[test]
    fn a_taken_final_name_grows_the_still_capture_s_suffix() {
        let directory = temp_dir("reserve-suffix");
        let first = reserve(&directory, "Recording 2026-09-14 at 10.00.00").expect("first reserve");
        assert_eq!(
            file_name(&first.final_path),
            OsStr::new("Recording 2026-09-14 at 10.00.00.mp4")
        );
        let second =
            reserve(&directory, "Recording 2026-09-14 at 10.00.00").expect("second reserve");
        assert_eq!(
            file_name(&second.final_path),
            OsStr::new("Recording 2026-09-14 at 10.00.00 2.mp4")
        );
    }

    /// R4
    #[test]
    fn reserve_leaves_a_zero_byte_placeholder_and_no_temporary_file() {
        let directory = temp_dir("reserve-placeholder");
        let files = reserve(&directory, "Recording").expect("reserve");
        let placeholder = std::fs::metadata(&files.final_path).expect("the placeholder exists");
        assert_eq!(placeholder.len(), 0, "the placeholder holds no bytes");
        assert!(
            !files.temporary.exists(),
            "the temporary file is AVAssetWriter's to create"
        );
    }

    /// R5
    #[test]
    fn commit_moves_the_bytes_to_the_final_name() {
        let directory = temp_dir("commit-moves");
        let files = reserve(&directory, "Recording").expect("reserve");
        std::fs::write(&files.temporary, b"a finished movie").expect("write the temporary movie");

        let committed = commit(&files).expect("commit");

        assert_eq!(committed, files.final_path);
        assert!(!files.temporary.exists(), "the temporary file is gone");
        assert_eq!(
            std::fs::read(&files.final_path).expect("read the committed movie"),
            b"a finished movie"
        );
    }

    /// R6
    #[test]
    fn commit_without_a_temporary_file_leaves_the_placeholder_alone() {
        let directory = temp_dir("commit-missing");
        let files = reserve(&directory, "Recording").expect("reserve");

        assert!(commit(&files).is_err(), "there is nothing to commit");

        let placeholder =
            std::fs::metadata(&files.final_path).expect("the placeholder is still there");
        assert_eq!(placeholder.len(), 0, "and is still empty");
    }

    /// R7
    #[test]
    fn abandon_takes_both_files_away_and_is_quiet_the_second_time() {
        let directory = temp_dir("abandon");
        let files = reserve(&directory, "Recording").expect("reserve");
        std::fs::write(&files.temporary, b"half a movie").expect("write the temporary movie");

        abandon(&files);

        assert!(!files.temporary.exists(), "the temporary file is gone");
        assert!(!files.final_path.exists(), "the placeholder is gone");
        abandon(&files);
    }

    /// R8
    #[test]
    fn sweep_takes_this_application_s_litter_and_nothing_else() {
        let directory = temp_dir("sweep");

        // A crashed recording: an unfinalised movie and its empty placeholder.
        let crashed = reserve(&directory, "Crashed").expect("reserve the crashed recording");
        std::fs::write(&crashed.temporary, b"half a movie").expect("write the crashed movie");

        // A leftover whose placeholder has since grown a real movie. The name is
        // the user's now.
        let grown = reserve(&directory, "Grown").expect("reserve the grown name");
        std::fs::write(&grown.temporary, b"half a movie").expect("write the grown leftover");
        std::fs::write(&grown.final_path, b"a whole movie").expect("write the user's movie");

        // The recording running right now.
        let live = reserve(&directory, "Live").expect("reserve the live recording");
        std::fs::write(&live.temporary, b"still writing").expect("write the live movie");

        // Files that were never ours.
        std::fs::write(directory.join("shot.png"), b"a picture").expect("write shot.png");
        std::fs::write(directory.join("notes.mp4"), b"a movie").expect("write notes.mp4");

        let removed = sweep(&directory, Some(&live));

        assert_eq!(removed, 3);
        assert!(!crashed.temporary.exists(), "the crashed movie is gone");
        assert!(!crashed.final_path.exists(), "its placeholder is gone");
        assert!(
            !grown.temporary.exists(),
            "the grown name's leftover is gone"
        );
        assert_eq!(
            std::fs::read(&grown.final_path).expect("the user's movie is still there"),
            b"a whole movie"
        );
        assert!(live.temporary.exists(), "the live recording still writes");
        assert!(live.final_path.exists(), "and still holds its name");
        assert!(directory.join("shot.png").exists());
        assert!(directory.join("notes.mp4").exists());
    }

    /// R9
    #[test]
    fn sweep_leaves_a_directory_that_is_named_like_a_leftover() {
        let directory = temp_dir("sweep-directory");
        let files = reserve(&directory, "Recording").expect("reserve");
        std::fs::create_dir(&files.temporary).expect("create a directory under the leftover name");

        let removed = sweep(&directory, None);

        assert_eq!(removed, 0);
        assert!(files.temporary.is_dir(), "the directory is untouched");
        assert!(files.final_path.exists(), "and so is the placeholder");
    }

    /// What the platform hands back about a recording that wrote `frames`
    /// frames into `files`.
    fn summary(files: &RecordingFiles, frames: u64) -> RecordingSummary {
        RecordingSummary {
            path: files.temporary.clone(),
            bytes: 1024,
            frames,
        }
    }

    /// F1
    #[test]
    fn a_recording_that_took_frames_lands_under_its_final_name() {
        let directory = temp_dir("finish-happy");
        let files = reserve(&directory, "Recording").expect("reserve");
        std::fs::write(&files.temporary, b"a finished movie").expect("write the movie");

        let finished = finish(Ok(summary(&files, 12)), &files);

        assert_eq!(finished.path, Some(files.final_path.clone()));
        assert_eq!(
            finished.complaint, None,
            "there is nothing to complain about"
        );
        assert!(!files.temporary.exists(), "the temporary file is gone");
        assert_eq!(
            std::fs::read(&files.final_path).expect("read the finished movie"),
            b"a finished movie"
        );
    }

    /// F2
    #[test]
    fn a_recording_that_took_no_frames_is_not_handed_to_the_user() {
        let directory = temp_dir("finish-empty");
        let files = reserve(&directory, "Recording").expect("reserve");
        std::fs::write(&files.temporary, b"a file no player can open")
            .expect("write the unplayable file");

        let finished = finish(Ok(summary(&files, 0)), &files);

        assert_eq!(finished.path, None, "there is no movie to give");
        let complaint = finished
            .complaint
            .expect("the user is told why nothing was saved");
        assert!(
            complaint.contains("not record a single frame"),
            "{complaint:?} has to say that no frames were recorded"
        );
        assert!(!files.temporary.exists(), "the temporary file is gone");
        assert!(!files.final_path.exists(), "and so is the placeholder");
    }

    /// F3
    #[test]
    fn a_platform_failure_reaches_the_user_and_leaves_nothing_behind() {
        let directory = temp_dir("finish-failure");
        let files = reserve(&directory, "Recording").expect("reserve");
        std::fs::write(&files.temporary, b"half a movie").expect("write the half movie");

        let finished = finish(Err(CaptureError::Platform("disk full".to_string())), &files);

        assert_eq!(finished.path, None);
        let complaint = finished
            .complaint
            .expect("the platform's own reason reaches the user");
        assert!(
            complaint.contains("disk full"),
            "{complaint:?} has to carry what the platform said"
        );
        assert!(!files.temporary.exists(), "the temporary file is gone");
        assert!(!files.final_path.exists(), "and so is the placeholder");
    }

    /// F4
    #[test]
    fn a_finished_recording_with_no_file_leaves_no_placeholder_behind() {
        let directory = temp_dir("finish-nothing-written");
        let files = reserve(&directory, "Recording").expect("reserve");

        let finished = finish(Ok(summary(&files, 12)), &files);

        assert_eq!(finished.path, None);
        let complaint = finished
            .complaint
            .expect("the user is told the movie did not arrive");
        assert!(!complaint.is_empty());
        assert!(
            !files.final_path.exists(),
            "an empty placeholder left under a capture's name is a file that cannot be opened"
        );
    }

    /// F5
    #[test]
    fn a_cancelled_recording_leaves_nothing_on_disk() {
        let directory = temp_dir("discard-clean");
        let files = reserve(&directory, "Recording").expect("reserve");
        std::fs::write(&files.temporary, b"half a movie").expect("write the half movie");

        assert_eq!(discard(Ok(()), &files), None, "there is nothing to report");

        assert!(!files.temporary.exists(), "the temporary file is gone");
        assert!(!files.final_path.exists(), "and so is the placeholder");
    }

    /// F6
    #[test]
    fn a_cancel_the_platform_failed_is_still_a_cancel() {
        let directory = temp_dir("discard-failure");
        let files = reserve(&directory, "Recording").expect("reserve");
        std::fs::write(&files.temporary, b"half a movie").expect("write the half movie");

        let complaint = discard(Err(CaptureError::Platform("boom".to_string())), &files)
            .expect("a stream that would not come apart is worth saying out loud");
        assert!(
            complaint.contains("boom"),
            "{complaint:?} has to carry what the platform said"
        );

        assert!(
            !files.temporary.exists(),
            "a cancel that leaves a half-written movie in the user's folder is not a cancel"
        );
        assert!(!files.final_path.exists(), "and the placeholder goes too");
    }
}
