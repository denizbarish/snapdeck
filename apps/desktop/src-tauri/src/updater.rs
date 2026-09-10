//! Updates, from the GitHub release the manifest at `plugins.updater.endpoints`
//! points at.
//!
//! This is the only network connection Snapdeck makes, and it reaches two
//! hosts: the endpoint the check asks, and the one the archive is fetched from.
//! Everything else about this application happens on the machine it runs on, and
//! that is a promise worth keeping literally, so four rules shape the module.
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
//! That sentence is worth reading narrowly, because it is narrower than it
//! sounds. The signature covers the archive bytes and nothing else: `version`,
//! `notes`, `url` and `pub_date` are unauthenticated, and the release notes in
//! particular are prose from the endpoint that this application shows above an
//! install button, which is why `release_notes` says where they came from.
//! What signing buys is that an endpoint which has been taken over cannot
//! install code this project did not build. What it does not buy is that such an
//! endpoint cannot name a version number of its choosing beside a URL pointing
//! at an older archive this project did sign, and so move an installation back
//! onto a release with a bug somebody already fixed. `update_history` closes the
//! part of that a client can close, by comparing every manifest against the
//! highest version this installation has ever run rather than against whichever
//! bundle happens to be on disk; nothing here compares the unpacked bundle
//! against the version that was promised, and until something does, a manifest
//! that lies upward is not caught.
//!
//! Nothing is fetched from a host this build does not name. The endpoint is
//! checked by the plugin when it is configured, but the `url` in the manifest it
//! answers with is not checked by anybody: `Update::download` builds a fresh
//! client, follows redirects and fetches whatever it was given, so a hostile
//! manifest is a way to make Snapdeck pull several megabytes from an
//! attacker-named host in cleartext. The install would still be refused, because
//! the archive would not verify, but the connection would already have happened
//! and told somebody who never asked where this machine is. `ALLOWED_UPDATE_HOSTS`
//! is what makes the promise at the top of this file true rather than nearly
//! true.
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

use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use semver::Version;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::{
    extract_path_from_executable, Error as UpdaterError, RemoteRelease, Update, UpdaterExt,
};

use crate::{report::report_failure, state::AppState, update_history};

/// Longest release note shown in the update dialog, in characters.
///
/// An `NSAlert` grows to fit its text and has nowhere to scroll, so a long
/// changelog would push the buttons off the bottom of a small display. Counted
/// in characters rather than bytes for the reason `tray::shorten` gives.
const NOTES_LIMIT: usize = 600;

/// Where the release notes in the dialog came from.
///
/// Six hundred characters of prose from the update server, rendered directly
/// above a button that installs software, read as Snapdeck's own words unless
/// something says otherwise. Nothing in a manifest is signed, so this line is
/// the difference between quoting the endpoint and speaking for it.
const NOTES_PROVENANCE: &str = "From the release notes published at the update server:";

/// The hosts a manifest may name for the archive itself.
///
/// The endpoint is `plugins.updater.endpoints` in `tauri.conf.json` and is
/// checked when the plugin is configured. This is the other half: the `url` the
/// endpoint *answers* with, which nothing else checks. GitHub serves a release
/// asset from `github.com` and redirects to `objects.githubusercontent.com`, and
/// both have to be here because the redirect is followed by the same client.
///
/// Kept as a list of hosts rather than a prefix match on the whole URL, because
/// a prefix match is the kind of check that reads as correct and is not:
/// `https://github.com.evil.example/` has the right prefix and is not GitHub.
const ALLOWED_UPDATE_HOSTS: [&str; 2] = ["github.com", "objects.githubusercontent.com"];

/// What a second `Check for Updates…` is told while the first is still running.
///
/// Not an error, and it is worded as an answer rather than as a failure: the
/// menu bar item it lands in is the only surface this application has that needs
/// no window, and the alternative was returning without saying anything at all.
const ALREADY_CHECKING: &str =
    "Snapdeck is already checking for updates. That check has not answered yet; it will say what it found when it does.";

/// How long the check may wait on the endpoint before giving up.
///
/// Without one the future is pending forever when an endpoint accepts the
/// connection and never answers, `CHECK_IN_FLIGHT` is never released, and every
/// later `Check for Updates…` is a silent no-op for the rest of the session.
/// With automatic checking off by default, that menu item is the only way to get
/// an update at all, so a wedged check costs more here than in an application
/// that also checks on its own.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the download of the archive may take.
///
/// A separate budget, and the reason the check's own timeout is not simply left
/// in place: the plugin hands the `Updater`'s timeout to the `Update` it
/// produces, where it becomes a deadline for the whole download rather than for
/// a request that answers with a few hundred bytes of JSON. Thirty seconds is
/// generous for the manifest and would fail a bundle-sized download on a slow
/// connection.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

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
        // Said out loud rather than dropped. A menu item that does nothing at
        // all when clicked is indistinguishable from a broken one, and the
        // guard is claimed for as long as a check waits on the network. The
        // menu bar rather than an alert, for the reason `offer` gives: the
        // check that is holding the slot may have an alert on screen right
        // now, and a second one raised behind it is the failure this module is
        // built to avoid.
        report_failure(app, ALREADY_CHECKING);
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
        Ok(Some(update)) => match refuse_update(&update) {
            // The check itself succeeded, so this is not reported in the words
            // a failed check gets: the user is being told that there is an
            // update and that Snapdeck will not take it.
            Some(message) => {
                complain(app, answer, "Snapdeck did not take this update", message).await
            }
            None => offer(app, update).await,
        },
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
        Err(err) => {
            complain(
                app,
                answer,
                "Snapdeck could not check for updates",
                describe_failure(Stage::Check, &err),
            )
            .await
        }
    }
}

/// Asks the endpoint what the latest release is.
///
/// Split from `run_check` only so that the two error sources, building the
/// updater and running the check, come back as one value: the user does not
/// care which of them failed, and `describe_failure` answers for both.
///
/// Built here rather than taken from `app.updater()` for two things the plugin's
/// own default cannot supply. A timeout, because `CHECK_TIMEOUT` says what a
/// check without one costs. And a comparator that knows the high-water mark:
/// `updater_builder` carries the one `plugin` installed, which is the same rule
/// against the version this bundle reports, and this narrows it to the same rule
/// against the highest version this installation has ever run. The rule itself
/// is still written once, in `is_upgrade`; only the floor it is asked about
/// differs.
async fn check(app: &AppHandle) -> Result<Option<Update>, UpdaterError> {
    let floor = update_history::high_water_mark(app, &app.package_info().version);
    app.updater_builder()
        .timeout(CHECK_TIMEOUT)
        // The version the plugin offers is ignored rather than compared: it is
        // this bundle's own, and `floor` already includes it.
        .version_comparator(move |_current, release: RemoteRelease| {
            is_upgrade(&floor, &release.version)
        })
        .build()?
        .check()
        .await
}

/// Why an update that was found will not be offered, or `None` when it will be.
///
/// Both answers are about the perimeter rather than about the release. The
/// archive has not been fetched yet at this point and its signature has not been
/// looked at; what is being decided is whether Snapdeck is willing to make the
/// connection the manifest asks for, and whether it is in a position to replace
/// itself if it does.
fn refuse_update(update: &Update) -> Option<String> {
    foreign_download(update).or_else(unreplaceable_bundle)
}

/// Refuses a manifest that points the download somewhere this build does not
/// name.
///
/// Before the dialog rather than before `download_and_install`, which is the
/// later of the two places it could go: an update Snapdeck will not fetch is not
/// one to ask the user about, and asking would spend their answer on an
/// operation that cannot happen.
fn foreign_download(update: &Update) -> Option<String> {
    let url = &update.download_url;
    if allowed_download(url.scheme(), url.host_str()) {
        return None;
    }
    Some(format!(
        "Snapdeck found an update but will not download it: the update server pointed at {}, which is not where this project publishes its releases. Nothing was downloaded and nothing on your Mac was changed. Check the project's releases page yourself.",
        url.host_str().unwrap_or("no host at all")
    ))
}

/// Whether an archive may be fetched from this scheme and host.
///
/// The whole of the decision, taken apart from the `Url` so that it can be
/// tested without one. `host_str` has already stripped any credentials, so
/// `https://github.com@evil.example/` answers `evil.example` here rather than
/// the host somebody wrote it to look like.
fn allowed_download(scheme: &str, host: Option<&str>) -> bool {
    if !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    let Some(host) = host else {
        return false;
    };
    ALLOWED_UPDATE_HOSTS
        .iter()
        .any(|allowed| host.eq_ignore_ascii_case(allowed))
}

/// What replacing this installation would cost, as far as it can be known
/// before trying.
///
/// The install is destructive and has no way back, and that is a property of
/// `tauri-plugin-updater` rather than of anything here, so the only place to act
/// on it is before agreeing to it. On macOS the plugin renames the running
/// `.app` into a `TempDir`, unpacks the new one, and renames that into place. If
/// the second rename fails the error is returned, the `TempDir` is dropped, and
/// the backup is deleted with it: the user is left with no Snapdeck at all and a
/// message about an update that did not work.
///
/// Two things make that second rename fail, and both can be seen in advance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallSite {
    /// The bundle can be replaced where it is.
    Replaceable,
    /// The folder holding the bundle refuses a write. The plugin's answer to
    /// this is to `rm -rf` the app with administrator privileges, which is both
    /// a password prompt nobody was told about and the deletion of the only copy
    /// of Snapdeck before the replacement is in place.
    Unwritable,
    /// The folder holding the bundle is on a different volume from the
    /// temporary directory. `rename` does not cross volumes, so the install
    /// fails at exactly the step after the running app has been moved aside.
    /// The ordinary way to arrive here is running Snapdeck from an external
    /// disk, since `$TMPDIR` is on the boot volume.
    OtherVolume,
}

/// Refuses an update this installation is in no position to survive.
///
/// A bundle whose location cannot be worked out is not refused. That is the
/// development build, where the binary is not in a `.app` at all, and treating
/// an unknown location as a hazard would make the update path untestable
/// without proving anything about a real installation.
fn unreplaceable_bundle() -> Option<String> {
    let bundle = std::env::current_exe()
        .ok()
        .and_then(|executable| extract_path_from_executable(&executable).ok())?;
    let parent = bundle.parent()?;
    describe_install_site(inspect_install_site(parent), parent)
}

/// Looks at the folder Snapdeck is installed in.
fn inspect_install_site(parent: &Path) -> InstallSite {
    if !takes_a_write(parent) {
        return InstallSite::Unwritable;
    }
    if !same_volume(parent, &std::env::temp_dir()) {
        return InstallSite::OtherVolume;
    }
    InstallSite::Replaceable
}

/// What to tell the user about a folder Snapdeck cannot replace itself in.
///
/// Split from the two probes above so that the wording is testable without a
/// folder that has to be arranged to refuse a write.
fn describe_install_site(site: InstallSite, parent: &Path) -> Option<String> {
    match site {
        InstallSite::Replaceable => None,
        InstallSite::Unwritable => Some(format!(
            "Snapdeck found an update but will not install it from where it is running. {} cannot be written to, so replacing Snapdeck there means deleting it with administrator privileges first, and if anything then goes wrong there is no copy left to go back to. Move Snapdeck into your Applications folder and check again.",
            parent.display()
        )),
        InstallSite::OtherVolume => Some(format!(
            "Snapdeck found an update but will not install it from where it is running. {} is on a different volume from the folder the update is unpacked into, and the last step of the install cannot move files between volumes: it would fail after the running copy of Snapdeck had already been moved aside, and that copy is deleted when it fails. Move Snapdeck into your Applications folder and check again.",
            parent.display()
        )),
    }
}

/// Whether a file can actually be created in `directory`.
///
/// A real file rather than a permissions bit, for the reason
/// `settings::ensure_writable` gives: `/Applications` is owned by root with the
/// owner write bit set, so a metadata read calls it writable for every user on
/// the machine, including the ones who cannot write a byte into it.
///
/// Creates nothing that is not removed again, and creates no directory: a
/// missing folder is not writable, and the answer to that is not to make one.
fn takes_a_write(directory: &Path) -> bool {
    let probe = directory.join(crate::settings::probe_name());
    let created = std::fs::File::options()
        .write(true)
        .create_new(true)
        .open(&probe)
        .is_ok();
    if created {
        let _ = std::fs::remove_file(&probe);
    }
    created
}

/// Whether two paths are on the same mounted volume.
///
/// A question that cannot be answered is answered `true`. Refusing an update
/// because a `stat` failed would turn an unreadable temporary directory into a
/// permanent refusal, and the install would fail with a message of its own
/// anyway.
fn same_volume(one: &Path, other: &Path) -> bool {
    let (Ok(one), Ok(other)) = (std::fs::metadata(one), std::fs::metadata(other)) else {
        return true;
    };
    one.dev() == other.dev()
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
async fn offer(app: &AppHandle, mut update: Update) {
    let title = format!("Snapdeck {} is available", update.version);
    let body = format!(
        "You are running {}. Snapdeck will download it, replace itself and restart.\n\nmacOS will ask for Screen Recording permission again afterwards: it ties that permission to the app's signature, and an update changes it. If macOS will not let Snapdeck replace itself where it is installed, it asks for an administrator password.\n\n{}",
        update.current_version,
        release_notes(update.body.as_deref())
    );
    if !ask(app, title, body, "Update and Restart", "Not Now").await {
        return;
    }
    // The check's deadline is not the download's. The plugin hands the
    // `Updater`'s timeout to the `Update`, where it would become a deadline for
    // several megabytes rather than for a few hundred bytes of JSON.
    update.timeout = Some(DOWNLOAD_TIMEOUT);
    // Downloads, verifies the signature over what arrived, and only then unpacks
    // it over the running bundle. This is where a signature that does not verify
    // is refused, which is why nothing here treats a finished download as a
    // finished update.
    if let Err(err) = update.download_and_install(|_, _| {}, || {}).await {
        report_failure(app, &describe_failure(Stage::Install, &err));
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
/// The notes are attributed rather than shown bare. They are not covered by the
/// signature and they are not this application's words, and six hundred
/// characters of unauthenticated prose above an install button reads as
/// Snapdeck's own unless the dialog says whose they are.
fn release_notes(notes: Option<&str>) -> String {
    let notes = notes.map(str::trim).filter(|notes| !notes.is_empty());
    let Some(notes) = notes else {
        return "The release has no notes.".to_string();
    };
    if notes.chars().count() <= NOTES_LIMIT {
        return format!("{NOTES_PROVENANCE}\n\n{notes}");
    }
    let kept: String = notes.chars().take(NOTES_LIMIT).collect();
    format!("{NOTES_PROVENANCE}\n\n{}…", kept.trim_end())
}

/// Which half of the update a failure came out of.
///
/// The two are not interchangeable and used not to be told apart, which put the
/// worst available sentence in front of the worst available situation: an
/// install that fails after the plugin has moved the running `.app` aside was
/// reported as "Snapdeck could not check for updates", to a user whose
/// application had just been removed from disk. By then the check had succeeded,
/// the download had succeeded and the signature had verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Asking the endpoint what the latest release is. Nothing has been
    /// downloaded and nothing on this machine has been touched.
    Check,
    /// Downloading the archive, verifying it and putting it in place of the
    /// running bundle.
    Install,
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
/// The catch-all is the arm that needs the stage. Everything ordinary lands
/// there, an `Io` error and a refused authorisation among them, and the same
/// error means "the network was not there" during a check and "your application
/// may not be where it was" during an install.
///
/// Exported to be tested, for the same reason `SettingsWindow.shortcutNotice`
/// is: it is the sentence that decides whether a refusal is understood.
pub fn describe_failure(stage: Stage, err: &UpdaterError) -> String {
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
        _ => match stage {
            Stage::Check => format!("Snapdeck could not check for updates. (Details: {err})"),
            // Says what is true of the machine rather than what failed, because
            // the plugin renames the running application into a temporary
            // directory before this can fail and deletes that directory on the
            // way out. Whoever reads this may have no Snapdeck left, and the
            // one thing they need is the way back.
            Stage::Install => format!(
                "The update was downloaded and its signature verified, but Snapdeck could not put it in place. (Details: {err}) Snapdeck may have been moved out of the folder it was installed in. If it does not open again, download the latest release from the project's releases page and install it by hand."
            ),
        },
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

/// Reports a check that produced no update to whoever asked for it.
///
/// Always logged, so a check that failed is never invisible afterwards. A check
/// the user started gets a dialog on top of that, because they are waiting for
/// an answer.
///
/// A check at launch gets the log alone, and deliberately not the menu bar
/// marker `report_failure` also raises: that marker is cleared by the next
/// capture, so a Wi-Fi network that was not up yet at login would sit in the
/// menu bar looking like a capture had failed.
///
/// The title is the caller's, because the two things reported through here are
/// not the same event. A check that could not be made and an update that was
/// found and then refused have different first lines, and the refusal must not
/// claim that the check failed.
async fn complain(app: &AppHandle, answer: Answer, title: &str, message: String) {
    eprintln!("snapdeck: {message}");
    crate::report::append_to_log(app, &message);
    if answer == Answer::Always {
        tell(app, MessageDialogKind::Error, title, message).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use base64::Engine as _;
    use minisign_verify::{PublicKey, Signature};

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

    /// What `check` composes, and the reason it composes it.
    ///
    /// A manifest's `version` is not covered by the signature, so an endpoint
    /// that has been taken over can put any number it likes beside a URL naming
    /// an older archive this project really did sign. Once that has landed, the
    /// bundle on disk says 0.1.0 and `is_upgrade` against the bundle would
    /// happily accept 0.4.0 as an upgrade, walking the installation back again
    /// and again. Measured against the mark instead, only a release higher than
    /// anything this machine has ever run is an update.
    #[test]
    fn a_manifest_is_measured_against_the_mark_rather_than_the_bundle_on_disk() {
        let running = v("0.1.0");
        let floor = update_history::floor(Some("0.5.0"), &running);
        assert_eq!(floor, v("0.5.0"));

        assert!(
            is_upgrade(&running, &v("0.4.0")),
            "the bundle on disk on its own would accept this"
        );
        assert!(
            !is_upgrade(&floor, &v("0.4.0")),
            "and the mark is what refuses it"
        );
        assert!(
            !is_upgrade(&floor, &v("0.5.0")),
            "nor the same release again"
        );
        assert!(
            is_upgrade(&floor, &v("0.6.0")),
            "a genuinely newer release is still an update"
        );
    }

    /// The archive is fetched from where this project publishes and from
    /// nowhere else. `validate_endpoints` covers the endpoint that is asked;
    /// this covers the URL it answers with, which nothing else looks at.
    #[test]
    fn the_archive_is_fetched_only_from_where_this_project_publishes() {
        assert!(allowed_download("https", Some("github.com")));
        assert!(allowed_download(
            "https",
            Some("objects.githubusercontent.com")
        ));
    }

    /// Cleartext is refused even to the right host: the point of the check is
    /// that nobody who never asked learns this machine's address, and `http`
    /// tells every hop on the way.
    #[test]
    fn a_download_over_anything_but_https_is_refused() {
        assert!(!allowed_download("http", Some("github.com")));
        assert!(!allowed_download("file", Some("github.com")));
        assert!(!allowed_download("ftp", Some("github.com")));
    }

    /// The refusals that matter, and the one that reads as correct and is not.
    /// A prefix or suffix match on the host would accept the first three of
    /// these.
    #[test]
    fn a_download_from_any_other_host_is_refused() {
        for host in [
            "github.com.evil.example",
            "evil-github.com",
            "notgithub.com",
            "raw.githubusercontent.com",
            "evil.example",
        ] {
            assert!(
                !allowed_download("https", Some(host)),
                "{host} is not where this project publishes"
            );
        }
        assert!(
            !allowed_download("https", None),
            "a URL with no host at all"
        );
    }

    /// Host comparison is case-insensitive, because DNS is and the check must
    /// not be a spelling test.
    #[test]
    fn the_host_is_matched_without_regard_to_case() {
        assert!(allowed_download("HTTPS", Some("GitHub.com")));
    }

    /// A folder Snapdeck can be replaced in is not a reason to refuse
    /// anything, and the temporary directory is such a folder: it is writable
    /// and it is on the same volume as itself.
    #[test]
    fn an_ordinary_writable_folder_is_replaceable() {
        let directory = std::env::temp_dir();
        assert_eq!(inspect_install_site(&directory), InstallSite::Replaceable);
        assert_eq!(
            describe_install_site(InstallSite::Replaceable, &directory),
            None
        );
    }

    /// The case the plugin answers with `rm -rf` under administrator
    /// privileges. A metadata read would call this folder writable when it is
    /// owned by somebody else, which is why `takes_a_write` writes a file.
    #[test]
    fn a_folder_that_refuses_a_write_is_not_replaceable() {
        use std::os::unix::fs::PermissionsExt;

        let directory = std::env::temp_dir().join(format!(
            "snapdeck-install-site-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&directory).expect("the test's own temporary directory");
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555))
            .expect("make it read-only");

        // Root ignores the permission bits, which would make this test
        // meaningless rather than failing. Measured rather than assumed: if the
        // probe got in anyway, this run is not one that can prove anything.
        let refused = !takes_a_write(&directory);
        if refused {
            assert_eq!(inspect_install_site(&directory), InstallSite::Unwritable);
        }

        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).ok();
        std::fs::remove_dir_all(&directory).ok();
        assert!(
            refused || std::env::var_os("USER").is_none_or(|user| user == "root"),
            "a folder with mode 0555 must refuse a write to anybody but root"
        );
    }

    /// The probe leaves nothing behind. It runs on every check, in the folder
    /// the user keeps their applications in.
    #[test]
    fn the_write_probe_removes_itself() {
        let directory = std::env::temp_dir().join(format!(
            "snapdeck-install-site-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&directory).expect("the test's own temporary directory");

        assert!(takes_a_write(&directory));
        let left_behind: Vec<_> = std::fs::read_dir(&directory)
            .expect("read the directory back")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        assert!(left_behind.is_empty(), "{left_behind:?}");

        std::fs::remove_dir_all(&directory).ok();
    }

    /// A folder that is not there is not writable, and the answer to that is
    /// not to create it: `ensure_writable` creates folders because the user
    /// just named one, and nothing here has been named by anybody.
    #[test]
    fn the_write_probe_creates_no_folder() {
        let directory = std::env::temp_dir().join(format!(
            "snapdeck-install-site-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::remove_dir_all(&directory).ok();

        assert!(!takes_a_write(&directory));
        assert!(!directory.exists(), "the probe must not have made it");
    }

    /// Both refusals name the folder and both name the way out. Somebody whose
    /// update was refused has to be able to act on the message.
    #[test]
    fn a_site_that_cannot_be_replaced_says_where_and_says_what_to_do() {
        for site in [InstallSite::Unwritable, InstallSite::OtherVolume] {
            let message = describe_install_site(site, Path::new("/Volumes/Stick"))
                .unwrap_or_else(|| panic!("{site:?} has to be refused"));
            assert!(message.contains("/Volumes/Stick"), "{message}");
            assert!(message.contains("Applications folder"), "{message}");
            assert!(
                !message.contains("could not check"),
                "the check succeeded: {message}"
            );
        }
    }

    /// The message for the volume case has to say what is actually at stake,
    /// which is the running copy of Snapdeck rather than the update.
    #[test]
    fn a_bundle_on_another_volume_is_refused_before_it_can_be_lost() {
        let message = describe_install_site(InstallSite::OtherVolume, Path::new("/Volumes/Stick"))
            .expect("a cross-volume install has to be refused");
        assert!(message.contains("moved aside"), "{message}");
        assert!(message.contains("deleted when it fails"), "{message}");
    }

    /// A refused signature has to be named as one. Somebody told only that the
    /// update failed retries against an endpoint serving them something this
    /// build will never install.
    #[test]
    fn a_refused_signature_is_reported_as_a_refused_signature() {
        let message = describe_failure(
            Stage::Install,
            &UpdaterError::SignatureUtf8("not base64".to_string()),
        );
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
        let message = describe_failure(
            Stage::Check,
            &UpdaterError::Network("connection refused".to_string()),
        );
        assert!(!message.contains("signature"), "{message}");
        assert!(message.contains("connection refused"), "{message}");
    }

    /// A manifest this build cannot read is its own case: nothing is wrong with
    /// the signature, and nothing is wrong with the connection.
    #[test]
    fn an_unreadable_manifest_says_so_without_blaming_the_signature() {
        let message = describe_failure(Stage::Check, &UpdaterError::ReleaseNotFound);
        assert!(!message.contains("signature"), "{message}");
        assert!(message.contains("could not understand"), "{message}");
    }

    /// The worst available sentence in front of the worst available situation.
    /// By the time an install fails the check has succeeded, the download has
    /// succeeded, the signature has verified, and the plugin may already have
    /// renamed the running application into a temporary directory it is about
    /// to delete. Telling that user that Snapdeck could not check for updates
    /// leaves them with no idea that their application is gone.
    #[test]
    fn an_install_failure_is_not_reported_as_a_failed_check() {
        for err in [
            UpdaterError::Io(std::io::Error::new(
                std::io::ErrorKind::CrossesDevices,
                "Invalid cross-device link",
            )),
            UpdaterError::AuthenticationFailed,
        ] {
            let message = describe_failure(Stage::Install, &err);
            assert!(
                !message.contains("could not check for updates"),
                "the check succeeded: {message}"
            );
            assert!(message.contains("signature verified"), "{message}");
            assert!(
                message.contains("releases page"),
                "the way back has to be in the message: {message}"
            );
        }
    }

    /// The same error at the other end of the operation keeps the old words,
    /// because at that point they are true.
    #[test]
    fn the_same_error_during_a_check_still_says_the_check_failed() {
        let message = describe_failure(
            Stage::Check,
            &UpdaterError::Io(std::io::Error::other("no route to host")),
        );
        assert!(message.contains("could not check for updates"), "{message}");
    }

    #[test]
    fn notes_that_fit_are_shown_as_written() {
        assert_eq!(
            release_notes(Some("Fixes the editor.")),
            format!("{NOTES_PROVENANCE}\n\nFixes the editor.")
        );
    }

    /// The notes are the endpoint's words and are not covered by the signature.
    /// Rendered bare, directly above a button that installs software, they read
    /// as Snapdeck's own.
    #[test]
    fn the_notes_say_whose_words_they_are() {
        let shown = release_notes(Some(
            "Download the important security patch at evil.example.",
        ));
        assert!(shown.starts_with(NOTES_PROVENANCE), "{shown}");
        assert!(
            NOTES_PROVENANCE.contains("update server"),
            "the line has to name the source: {NOTES_PROVENANCE}"
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
        let shown = release_notes(Some(&"é".repeat(NOTES_LIMIT + 50)));
        let quoted = shown
            .strip_prefix(NOTES_PROVENANCE)
            .expect("the provenance line comes first")
            .trim_start();
        assert_eq!(quoted.chars().count(), NOTES_LIMIT + 1);
        assert!(quoted.ends_with('…'));
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

    /// And a refusal is said rather than swallowed. `Check for Updates…` is the
    /// only way to get an update at all with the launch check off, so a click
    /// that produces nothing at all is indistinguishable from a broken menu.
    #[test]
    fn a_refused_check_has_something_to_say() {
        assert!(
            ALREADY_CHECKING.contains("already checking"),
            "{ALREADY_CHECKING}"
        );
        assert!(
            !ALREADY_CHECKING.contains("could not"),
            "it is not a failure: {ALREADY_CHECKING}"
        );
    }

    // The refusal itself, rather than the sentence it is reported in.
    //
    // `tauri-plugin-updater` keeps its `verify_signature` private, so this
    // calls what that function calls, in the order it calls it: the manifest's
    // `signature` and the config's `pubkey` are both base64 wrappers around a
    // minisign file, and the check is `PublicKey::verify` over the archive
    // bytes. No network, no bundle and no key material in the repository: the
    // fixture below was signed with a throwaway key that exists nowhere else.

    /// An archive as it would arrive, and a signature over exactly those bytes.
    const FIXTURE_ARCHIVE: &[u8] = b"snapdeck update archive fixture, not a real bundle\n";

    /// The signature over `FIXTURE_ARCHIVE`, in the form a manifest carries it.
    const FIXTURE_SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUVjZXa0ZxdFk4akVRN29NbEtiV0FESE5wdFZhSGNYVklUT0FjZzRud29IS2dCS2YraGNMN29kQUIwWnFEbFhwUGNEZjZybHZPbm1OQU14RFV0WDUwUzZXclNPMjVMK0FnPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzg5MDQzNDk3CWZpbGU6Zml4dHVyZS5hcHAudGFyLmd6CmdHNjVaVWc5WDhtdUd3N1BPWXJUOUxIVmIwNlV0WHI2UVRNMUVFSlF6MXFNTE1id3pXc3RoR2UxU1ZuOGZrVWhGZlRiVFRmbWRZNSt4anNIQ24zZ0JnPT0K";

    /// The public half of the throwaway key, in the form `tauri.conf.json`
    /// carries one.
    const FIXTURE_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDhDM0NENkFBMDU2OUU5RDUKUldUVjZXa0ZxdFk4ak93TE83UVVMa3lybERDRTc5UjkzRjg5YkQ1UlVhSEVkWFJrOTNqT3QvVU4K";

    /// The key this build actually ships, read from the file that ships it.
    fn shipped_pubkey() -> String {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("the config is JSON");
        conf["plugins"]["updater"]["pubkey"]
            .as_str()
            .expect("the config states an updater public key")
            .to_string()
    }

    /// What `tauri_plugin_updater::verify_signature` does, in the same order,
    /// through the same crate.
    fn verify(archive: &[u8], signature: &str, pubkey: &str) -> Result<(), String> {
        let decode = |value: &str| -> Result<String, String> {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(value)
                .map_err(|err| format!("not base64: {err}"))?;
            String::from_utf8(bytes).map_err(|err| format!("not text: {err}"))
        };
        let key = PublicKey::decode(&decode(pubkey)?).map_err(|err| format!("bad key: {err}"))?;
        let signature = Signature::decode(&decode(signature)?)
            .map_err(|err| format!("bad signature: {err}"))?;
        key.verify(archive, &signature, true)
            .map_err(|err| format!("refused: {err}"))
    }

    /// The harness first: a signature over these exact bytes verifies, or the
    /// two refusals below would prove nothing.
    #[test]
    fn a_signature_over_the_archive_verifies() {
        verify(FIXTURE_ARCHIVE, FIXTURE_SIGNATURE, FIXTURE_PUBKEY)
            .expect("the fixture signature is over the fixture archive");
    }

    /// The property the whole module is built around. One byte different in
    /// what arrived, and the archive is refused rather than unpacked.
    #[test]
    fn an_archive_that_is_not_what_was_signed_is_refused() {
        let mut tampered = FIXTURE_ARCHIVE.to_vec();
        tampered[0] ^= 0x01;
        let refused = verify(&tampered, FIXTURE_SIGNATURE, FIXTURE_PUBKEY)
            .expect_err("a signature over other bytes must not verify");

        // And the refusal reaches the user as a refusal, not as a failed check.
        let message = describe_failure(
            Stage::Install,
            &UpdaterError::SignatureUtf8(refused.clone()),
        );
        assert!(
            message.contains("thrown away rather than installed"),
            "{message}"
        );
    }

    /// The other half: a perfectly good signature made by somebody else's key
    /// is refused by the key this build ships. This is what an endpoint that
    /// has been taken over and signs its own archives runs into.
    #[test]
    fn a_signature_from_another_key_is_refused_by_the_key_this_build_ships() {
        verify(FIXTURE_ARCHIVE, FIXTURE_SIGNATURE, &shipped_pubkey())
            .expect_err("only this project's key may sign an update for this build");
    }

    /// And the shipped key is a key at all. A truncated or garbled `pubkey` in
    /// `tauri.conf.json` builds, ships, and refuses every update ever published
    /// with a tooltip on the user's machine as the only symptom.
    #[test]
    fn the_shipped_public_key_is_a_minisign_key() {
        let pubkey = shipped_pubkey();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&pubkey)
            .expect("the pubkey in tauri.conf.json is base64");
        let text = String::from_utf8(decoded).expect("and base64 over text");
        PublicKey::decode(&text).expect("and that text is a minisign public key");
    }
}
