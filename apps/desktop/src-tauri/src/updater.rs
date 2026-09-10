//! Updates, from the GitHub release the manifest at `plugins.updater.endpoints`
//! points at.
//!
//! This is the only network connection Snapdeck makes. Everything else about
//! this application happens on the machine it runs on, and that is a promise
//! worth keeping literally, so three rules shape the module.
//!
//! Nothing is checked unless somebody asked. `Check for Updates…` in the menu
//! bar is the visible way, and the settings window has a checkbox for a check at
//! launch that is **off** until the user turns it on. There is no timer and no
//! first-run check: a connection the user did not ask for is the thing being
//! avoided, and one that happens once at launch instead of every hour is still
//! one they did not ask for.
//!
//! Nothing is downloaded unless somebody agreed. A check that finds an update
//! shows what it is and what the release says about it, and stops there until
//! the user presses the download button.
//!
//! Nothing is installed that this project did not sign. The manifest names a
//! signature over the `.app.tar.gz`, `tauri-plugin-updater` verifies it against
//! the public key compiled into the bundle, and a download whose signature does
//! not verify is thrown away rather than unpacked. The private half of that key
//! is in the release workflow's secrets and has never been in this repository.
//! A refusal is reported in the words `describe_failure` gives it, because a
//! silent refusal is indistinguishable from an update that never existed.
//!
//! The dialogs are the platform's own rather than a window of this
//! application's. A menu bar agent that opens a whole webview to ask a yes or
//! no question is paying for a window, a page, an entry in the capture picker
//! and an ACL capability, and the question is one `NSAlert` already answers.
//! They are driven from Rust for the same reason the folder picker is, so no
//! webview is granted the dialog plugin and no page can put one on screen.
//!
//! There is at most one alert per check, and that is a constraint the platform
//! imposed rather than a preference; `offer` says what was measured. Everything
//! that is not a question the user has to answer goes to the menu bar item and
//! the log instead, which is where this application's failures already go and
//! which needs no window to work.

use std::sync::atomic::{AtomicBool, Ordering};

use semver::Version;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::{Error as UpdaterError, RemoteRelease, Update, UpdaterExt};

use crate::{report::report_failure, state::AppState};

/// Longest release note shown in the update dialog, in characters.
///
/// An `NSAlert` grows to fit its text and has nowhere to scroll, so a long
/// changelog would push the buttons off the bottom of a small display. Counted
/// in characters rather than bytes for the reason `tray::shorten` gives.
const NOTES_LIMIT: usize = 600;

/// Whether one check is already running.
///
/// A module static rather than a field of `AppState`, which is where this
/// application's other in-flight flag lives, because nothing outside this module
/// has any use for the answer: the capture slot is shared between the tray, the
/// shortcut handler and the overlay worker, and this is read and written on one
/// path. What it prevents is two dialogs: `Check for Updates…` is a menu item,
/// and a check that is waiting on the network looks exactly like one that did
/// nothing.
static CHECK_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// Holds the check slot for as long as it lives.
///
/// Released on drop rather than by an explicit call, for the reason
/// `state::CaptureGuard` gives: an early return anywhere in the check must not
/// wedge the menu item for the rest of the session.
struct CheckGuard;

impl Drop for CheckGuard {
    fn drop(&mut self) {
        CHECK_IN_FLIGHT.store(false, Ordering::Release);
    }
}

fn begin_check() -> Option<CheckGuard> {
    CHECK_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .ok()
        .map(|_| CheckGuard)
}

/// Whether the user is owed an answer when there is nothing to report.
///
/// The whole difference between the two entry points. Somebody who chose
/// `Check for Updates…` is waiting for a reply and has to get one either way;
/// the check at launch happens while they are doing something else, so being
/// up to date is silence and a network failure is a line in the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Always,
    OnlyWhenThereIsAnUpdate,
}

/// The updater plugin, with this application's own idea of what an update is.
///
/// `default_version_comparator` is the point of building it here rather than
/// taking the plugin's default: the rule in `is_upgrade` is the one place
/// Snapdeck decides whether a release is one to move to, and it is stricter
/// than the plugin's `remote > current` about pre-releases.
pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R, tauri_plugin_updater::Config> {
    tauri_plugin_updater::Builder::new()
        .default_version_comparator(|current, release: RemoteRelease| {
            is_upgrade(&current, &release.version)
        })
        .build()
}

/// Whether `candidate` is a release this build should offer to move to.
///
/// Two rules, and both of them are about not surprising somebody.
///
/// Strictly newer by semantic version. An equal version is not an update, and
/// an older one is not an offer to go backwards: a manifest that has been rolled
/// back to an earlier release must not push every installation down to it.
///
/// A pre-release is only offered to a build that is itself a pre-release.
/// Semantic versioning already sorts `0.2.0-rc.1` below `0.2.0`, so somebody on
/// the finished release is never sent back to the candidate that preceded it,
/// but it sorts that same candidate *above* `0.1.0`, which would move somebody
/// who installed a stable build onto a release candidate they never asked for.
/// The release workflow marks `-rc` tags as pre-releases so GitHub's
/// `releases/latest` never serves their manifest at all; this is the half of
/// that which does not depend on GitHub, a tag being spelled correctly, or a
/// manifest being published by hand.
pub fn is_upgrade(current: &Version, candidate: &Version) -> bool {
    if !candidate.pre.is_empty() && current.pre.is_empty() {
        return false;
    }
    candidate > current
}

/// `Check for Updates…` in the menu bar.
///
/// Returns immediately. The check is a network call and the caller is the menu
/// event handler on the main thread, which may not wait for one.
pub fn check_on_request(app: &AppHandle) {
    spawn_check(app, Answer::Always);
}

/// The check at launch, when the user has asked for one in Settings.
///
/// Reads the setting rather than being called conditionally, so the rule that
/// this is off unless it was turned on lives in one place.
pub fn check_at_launch(app: &AppHandle) {
    if !app
        .state::<AppState>()
        .settings()
        .check_for_updates_at_launch
    {
        return;
    }
    spawn_check(app, Answer::OnlyWhenThereIsAnUpdate);
}

fn spawn_check(app: &AppHandle, answer: Answer) {
    // A second `Check for Updates…` while the first is still waiting on the
    // network would end in two dialogs about the same release.
    let Some(guard) = begin_check() else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _guard = guard;
        run_check(&app, answer).await;
    });
}

async fn run_check(app: &AppHandle, answer: Answer) {
    match check(app).await {
        Ok(Some(update)) => offer(app, update).await,
        Ok(None) => {
            if answer == Answer::Always {
                tell(
                    app,
                    MessageDialogKind::Info,
                    "Snapdeck is up to date",
                    format!(
                        "You are running {}, which is the latest release.",
                        version(app)
                    ),
                )
                .await;
            }
        }
        Err(err) => complain(app, answer, describe_failure(&err)).await,
    }
}

/// Asks the endpoint what the latest release is.
///
/// Split from `run_check` only so that the two error sources, building the
/// updater and running the check, come back as one value: the user does not
/// care which of them failed, and `describe_failure` answers for both.
async fn check(app: &AppHandle) -> Result<Option<Update>, UpdaterError> {
    app.updater()?.check().await
}

/// Shows what the release is, and updates only if the user says so.
///
/// One dialog and no more, which is a measured constraint rather than a taste.
/// Snapdeck has no windows of its own between captures, and an `NSAlert` raised
/// by an application in that state is not reliably put on screen a second time
/// within one run: measured here, the first alert of a process appears and takes
/// focus, and a later one sometimes returns its default answer without ever
/// being drawn. A confirmation the user never saw is not a confirmation, so this
/// path asks its one question and then reports through surfaces that need no
/// window at all.
///
/// So the button says what the whole operation does, the restart happens without
/// a second question, and a failure goes to the menu bar item, which is where
/// every other failure in this application goes and is readable long after a
/// dialog would have been dismissed.
async fn offer(app: &AppHandle, update: Update) {
    let title = format!("Snapdeck {} is available", update.version);
    let body = format!(
        "You are running {}. Snapdeck will download it, replace itself and restart.\n\nmacOS will ask for Screen Recording permission again afterwards: it ties that permission to the app's signature, and an update changes it.\n\n{}",
        update.current_version,
        release_notes(update.body.as_deref())
    );
    if !ask(app, title, body, "Update and Restart", "Not Now").await {
        return;
    }
    // Downloads, verifies the signature over what arrived, and only then unpacks
    // it over the running bundle. This is where a signature that does not verify
    // is refused, which is why nothing here treats a finished download as a
    // finished update.
    if let Err(err) = update.download_and_install(|_, _| {}, || {}).await {
        report_failure(app, &describe_failure(&err));
        return;
    }
    // Never returns: the process is replaced by the version that was just
    // unpacked over it. The user asked for exactly this one line ago.
    app.restart();
}

/// What the dialog shows about the release, cut down to something a dialog can
/// hold.
///
/// A manifest with no notes is a manifest a person forgot to write notes into,
/// which is not a reason to show an empty dialog.
fn release_notes(notes: Option<&str>) -> String {
    let notes = notes.map(str::trim).filter(|notes| !notes.is_empty());
    let Some(notes) = notes else {
        return "The release has no notes.".to_string();
    };
    if notes.chars().count() <= NOTES_LIMIT {
        return notes.to_string();
    }
    let kept: String = notes.chars().take(NOTES_LIMIT).collect();
    format!("{}…", kept.trim_end())
}

/// What to tell the user about a check or an install that did not finish.
///
/// The signature cases are the ones that have to be named rather than folded
/// into "something went wrong". A download that arrives intact and is then
/// thrown away is the security property this whole module is built around
/// working, and somebody who is told only that the update failed has no way to
/// tell it from a flaky connection: they would retry, and retry, against an
/// endpoint that is serving them something Snapdeck will never install.
///
/// Exported to be tested, for the same reason `SettingsWindow.shortcutNotice`
/// is: it is the sentence that decides whether a refusal is understood.
pub fn describe_failure(err: &UpdaterError) -> String {
    match err {
        // The signature did not verify, could not be decoded, or was not the
        // base64 a signature has to be. All three mean the same thing to the
        // user: what arrived is not what this project signed.
        UpdaterError::Minisign(_) | UpdaterError::Base64(_) | UpdaterError::SignatureUtf8(_) => {
            format!(
                "The update was downloaded but its signature does not match the key Snapdeck was built with, so it was thrown away rather than installed. Nothing on your Mac was changed. If you want this release, download it from the project's releases page yourself. (Details: {err})"
            )
        }
        // The endpoint answered with something that is not a manifest this
        // build can read: no manifest at all, JSON that is not one, or a
        // manifest without this platform in it. An *unsigned* manifest lands
        // here rather than in the arm above, because a `signature` is not
        // optional in the shape the plugin parses: an entry without one is not
        // a badly signed manifest, it is not a manifest. The refusal happens
        // before anything is downloaded, which is why the sentence does not
        // mention a download.
        UpdaterError::Serialization(_)
        | UpdaterError::ReleaseNotFound
        | UpdaterError::TargetNotFound(_)
        | UpdaterError::TargetsNotFound(_)
        | UpdaterError::Semver(_) => format!(
            "Snapdeck could not understand the answer from the update server, so it does not know whether there is a newer release and has downloaded nothing. Check the project's releases page yourself. (Details: {err})"
        ),
        // Everything else: no network, a proxy, a refused connection, a
        // temporary directory that cannot be written, a bundle that cannot be
        // replaced.
        _ => format!("Snapdeck could not check for updates. (Details: {err})"),
    }
}

/// The version this build reports, which is what `tauri.conf.json` says.
fn version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Puts a question on screen and waits for the answer.
///
/// `spawn_blocking` for the reason `commands::choose_save_directory` gives:
/// `blocking_show` waits on the main thread to answer, so it may not be called
/// from a thread the main thread is waiting on, and blocking one of the async
/// runtime's workers for as long as somebody takes to read a dialog is worse
/// than moving it aside.
/// The buttons are named by the caller rather than being Yes and No, because a
/// macOS button says what it does: nobody should have to read the body twice to
/// find out what `Yes` was about.
async fn ask(
    app: &AppHandle,
    title: String,
    body: String,
    confirm: &'static str,
    cancel: &'static str,
) -> bool {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .message(body)
            .title(title)
            .kind(MessageDialogKind::Info)
            .buttons(MessageDialogButtons::OkCancelCustom(
                confirm.to_string(),
                cancel.to_string(),
            ))
            .blocking_show()
    })
    .await
    // A dialog that could not be shown is not consent. Treated as "no", which
    // is the answer that changes nothing.
    .unwrap_or(false)
}

/// Says something the user does not have to answer.
///
/// Waits for the dismissal it does not need, which is the point: the check slot
/// stays claimed until the message has been read, so a second
/// `Check for Updates…` cannot raise an alert on top of this one. Alerts from an
/// application with no windows do not stack well, and the shape that failure
/// takes is an alert that answers itself without being drawn; see `offer`.
async fn tell(app: &AppHandle, kind: MessageDialogKind, title: &str, body: String) {
    let app = app.clone();
    let title = title.to_string();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .message(body)
            .title(title)
            .kind(kind)
            .buttons(MessageDialogButtons::Ok)
            .blocking_show()
    })
    .await;
}

/// Reports a failed check to whoever asked for it.
///
/// Always logged, so a check that failed is never invisible afterwards. A check
/// the user started gets a dialog on top of that, because they are waiting for
/// an answer.
///
/// A check at launch gets the log alone, and deliberately not the menu bar
/// marker `report_failure` also raises: that marker is cleared by the next
/// capture, so a Wi-Fi network that was not up yet at login would sit in the
/// menu bar looking like a capture had failed.
async fn complain(app: &AppHandle, answer: Answer, message: String) {
    eprintln!("snapdeck: {message}");
    crate::report::append_to_log(app, &message);
    if answer == Answer::Always {
        tell(
            app,
            MessageDialogKind::Error,
            "Snapdeck could not check for updates",
            message,
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(raw: &str) -> Version {
        Version::parse(raw).expect("the test's own version literal must parse")
    }

    /// The version this build reports is a version the comparator can read.
    /// A `tauri.conf.json` version that semver refuses would make every check
    /// fail with an error about the local build rather than the remote one.
    #[test]
    fn the_bundled_version_is_a_semantic_version() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("the config is JSON");
        let version = conf["version"]
            .as_str()
            .expect("the config states a version");
        Version::parse(version).expect("the version in tauri.conf.json must be a semantic version");
    }

    #[test]
    fn the_same_version_is_not_an_update() {
        assert!(!is_upgrade(&v("0.1.0"), &v("0.1.0")));
        assert!(!is_upgrade(&v("0.2.0-rc.1"), &v("0.2.0-rc.1")));
    }

    #[test]
    fn a_newer_version_is_an_update() {
        assert!(is_upgrade(&v("0.1.0"), &v("0.1.1")));
        assert!(is_upgrade(&v("0.1.0"), &v("0.2.0")));
        assert!(is_upgrade(&v("0.9.9"), &v("1.0.0")));
    }

    /// A manifest rolled back to an earlier release must not push every
    /// installation down to it.
    #[test]
    fn an_older_version_is_not_an_update() {
        assert!(!is_upgrade(&v("0.2.0"), &v("0.1.0")));
        assert!(!is_upgrade(&v("1.0.0"), &v("0.9.9")));
    }

    /// The rule that is stricter than semantic versioning on its own. A build
    /// that is not itself a pre-release is never moved onto one, however the
    /// two sort.
    #[test]
    fn a_stable_build_is_never_offered_a_pre_release() {
        assert!(v("0.2.0-rc.1") > v("0.1.0"), "semver alone would offer it");
        assert!(!is_upgrade(&v("0.1.0"), &v("0.2.0-rc.1")));
        assert!(!is_upgrade(&v("0.2.0"), &v("0.3.0-alpha.1")));
    }

    /// The other half: somebody who installed a release candidate is on the
    /// pre-release track, and both the next candidate and the finished release
    /// are updates for them.
    #[test]
    fn a_pre_release_build_moves_along_the_pre_release_track_and_off_it() {
        assert!(is_upgrade(&v("0.2.0-rc.1"), &v("0.2.0-rc.2")));
        assert!(is_upgrade(&v("0.2.0-rc.2"), &v("0.2.0")));
        assert!(!is_upgrade(&v("0.2.0-rc.2"), &v("0.2.0-rc.1")));
    }

    /// A refused signature has to be named as one. Somebody told only that the
    /// update failed retries against an endpoint serving them something this
    /// build will never install.
    #[test]
    fn a_refused_signature_is_reported_as_a_refused_signature() {
        let message = describe_failure(&UpdaterError::SignatureUtf8("not base64".to_string()));
        assert!(message.contains("signature"), "{message}");
        assert!(
            message.contains("thrown away rather than installed"),
            "the user has to be told nothing was installed: {message}"
        );
        assert!(
            message.contains("Nothing on your Mac was changed"),
            "{message}"
        );
    }

    /// And the other side of the same rule: an ordinary network failure must
    /// not be dressed up as a security event.
    #[test]
    fn a_network_failure_is_not_reported_as_a_signature_problem() {
        let message = describe_failure(&UpdaterError::Network("connection refused".to_string()));
        assert!(!message.contains("signature"), "{message}");
        assert!(message.contains("connection refused"), "{message}");
    }

    /// A manifest this build cannot read is its own case: nothing is wrong with
    /// the signature, and nothing is wrong with the connection.
    #[test]
    fn an_unreadable_manifest_says_so_without_blaming_the_signature() {
        let message = describe_failure(&UpdaterError::ReleaseNotFound);
        assert!(!message.contains("signature"), "{message}");
        assert!(message.contains("could not understand"), "{message}");
    }

    #[test]
    fn notes_that_fit_are_shown_as_written() {
        assert_eq!(
            release_notes(Some("Fixes the editor.")),
            "Fixes the editor."
        );
    }

    #[test]
    fn a_missing_note_is_a_sentence_rather_than_an_empty_dialog() {
        assert_eq!(release_notes(None), "The release has no notes.");
        assert_eq!(release_notes(Some("   ")), "The release has no notes.");
    }

    /// A dialog cannot scroll, so a long changelog is cut. Characters rather
    /// than bytes, because release notes carry punctuation that is not ASCII.
    #[test]
    fn a_long_note_is_cut_to_something_a_dialog_can_hold() {
        let cut = release_notes(Some(&"é".repeat(NOTES_LIMIT + 50)));
        assert_eq!(cut.chars().count(), NOTES_LIMIT + 1);
        assert!(cut.ends_with('…'));
    }

    /// The slot exists so that a check that returns early, or unwinds, does not
    /// leave the menu item dead for the rest of the session.
    #[test]
    fn a_second_check_is_refused_while_the_first_is_running() {
        let first = begin_check().expect("the first check claims the slot");
        assert!(begin_check().is_none(), "two checks would be two dialogs");
        drop(first);
        assert!(
            begin_check().is_some(),
            "the slot has to be reusable once the check finishes"
        );
    }
}
