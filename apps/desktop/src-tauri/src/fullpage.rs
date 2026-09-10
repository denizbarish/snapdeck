//! Capturing a scrolling window by taking its picture repeatedly.
//!
//! The browser extension is the good path: a page that knows its own height
//! scrolls itself and hands over one picture. This is the path for everything
//! else, and it is guesswork by comparison. It photographs the frontmost
//! window, posts a synthetic scroll, photographs it again, and asks
//! `snapdeck-stitch` where the pictures overlap. Nothing here knows what is
//! being scrolled, so nothing here can promise the result is the whole of it,
//! which is why the menu item says so out loud.
//!
//! The loop is the part worth testing and it is written to be testable: `run`
//! takes the capture as a closure and the scroll as a trait, so every rule it
//! obeys can be checked without a window server, without a permission and
//! without moving anything on the user's screen.

use std::time::SystemTime;

use core_graphics::event::{CGEvent, CGEventTapLocation, ScrollEventUnit};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use snapdeck_capture::{CaptureTarget, Frame, PixelFormat, ScreenCapturer};
use snapdeck_stitch::{stitch, Image, ScrollHint, StitchError};
use tauri::{AppHandle, Manager};

use crate::bridge::intake::{self, Delivered};
use crate::editor;
use crate::recents;
use crate::report::report_failure;
use crate::settings;
use crate::state::AppState;

/// Scrolls the frontmost window, so the capture loop can be tested without a
/// window server.
pub trait ScrollDriver {
    /// Scrolls by `lines`, positive meaning downwards.
    fn scroll(&self, lines: i32) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    /// The content stopped moving. The ordinary end.
    ContentSettled,
    /// `max_steps` was reached, so the picture is probably short.
    StepLimit,
    /// A capture failed. What was already collected is still worth stitching.
    CaptureFailed(String),
}

pub struct FullPageRun {
    pub frames: Vec<Frame>,
    pub stopped: StopReason,
}

/// How far one step scrolls, in the wheel's own lines.
///
/// Small enough that consecutive pictures still share most of their content,
/// which is the only thing `stitch` has to work with: it finds where two
/// frames agree, so a step that clears the window leaves it nothing to find
/// and the run comes back as a stack of unrelated pictures.
const SCROLL_LINES: i32 = 10;

/// The most pictures one run will take.
///
/// The stop that matters is `ContentSettled`; this is the one for content that
/// never settles, such as a list that loads more of itself as it is scrolled.
/// Reaching it is reported, because the picture is then short by an unknown
/// amount rather than finished.
const MAX_STEPS: usize = 40;

/// Captures a window repeatedly, scrolling between shots.
///
/// Never fails: a partial run is a partial picture, and spec 9 says a partial
/// picture is offered with a warning rather than thrown away.
pub fn run<C, D>(mut capture: C, driver: &D, max_steps: usize) -> FullPageRun
where
    C: FnMut() -> Result<Frame, String>,
    D: ScrollDriver,
{
    let mut frames: Vec<Frame> = Vec::new();
    for step in 0..max_steps {
        // Never before the first picture. Scrolling first would move the
        // window past its own top, and nothing in the finished picture would
        // show that the beginning of it is missing.
        if step > 0 {
            if let Err(reason) = driver.scroll(SCROLL_LINES) {
                return FullPageRun {
                    frames,
                    stopped: StopReason::CaptureFailed(reason),
                };
            }
        }
        // Deliberately not `?`. What has been photographed so far is still a
        // picture, and handing it back with the reason attached is what lets
        // the caller offer it with a warning.
        let frame = match capture() {
            Ok(frame) => frame,
            Err(reason) => {
                return FullPageRun {
                    frames,
                    stopped: StopReason::CaptureFailed(reason),
                }
            }
        };
        // Byte for byte, which is the only test available from here: this
        // module does not know what it is photographing, so the one thing it
        // can tell is that the scroll changed nothing. The repeated picture is
        // dropped rather than kept, because it carries no content the frame
        // before it does not already have.
        if frames.last().is_some_and(|last| last.data == frame.data) {
            return FullPageRun {
                frames,
                stopped: StopReason::ContentSettled,
            };
        }
        frames.push(frame);
    }
    FullPageRun {
        frames,
        stopped: StopReason::StepLimit,
    }
}

/// The tray label, which has to say what this is.
pub const FULLPAGE_MENU_LABEL: &str = "Capture Scrolling Window (Experimental)";

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    /// `Boolean AXIsProcessTrusted(void)`, which asks and does not prompt.
    ///
    /// Taken as `u8` rather than `bool` because that is what `Boolean` is: a
    /// byte, and a Rust `bool` holding anything but 0 or 1 is undefined.
    /// Deliberately not `AXIsProcessTrustedWithOptions`, whose prompt option is
    /// the only way this application could put a second permission dialog on
    /// screen; the user is sent to the settings pane instead, where they can
    /// see what they are being asked for.
    fn AXIsProcessTrusted() -> u8;
}

/// Whether macOS will let this process post a synthetic scroll.
pub fn accessibility_is_granted() -> bool {
    // SAFETY: a nullary ApplicationServices query with no arguments to
    // validate and no ownership to transfer.
    unsafe { AXIsProcessTrusted() != 0 }
}

/// What the user is told when the grant this feature needs is not in place.
///
/// Said at the moment they ask for the feature rather than at launch: a
/// screenshot application that demands a second system permission on the way
/// in is asking for something most of its users will never use. The deep link
/// travels in the sentence for the reason `PermissionReport` carries one, and
/// the relaunch caveat is real: macOS decides what a process may post when the
/// process starts.
///
/// The link is the Accessibility pane, not the Screen Recording pane
/// `snapdeck_capture::macos::permission::SETTINGS_DEEP_LINK` points at. Two
/// grants, two panes: a user sent to the wrong one finds a list that does not
/// mention Snapdeck.
const ACCESSIBILITY_DENIED: &str = concat!(
    "Snapdeck cannot capture a scrolling window without the Accessibility permission, because scrolling one means posting a scroll the way a mouse would. ",
    "Turn Snapdeck on in System Settings > Privacy & Security > Accessibility and relaunch Snapdeck: ",
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
);

/// What the user is told when a run stopped at the limit.
const STEP_LIMIT: &str =
    "Snapdeck stopped after taking as many pictures of that window as it will take in one go, so the result may be missing the end of it.";

/// What the user is told when the frames stopped overlapping partway through.
///
/// The stitched result is offered anyway, which is spec 9: a picture that is
/// missing a piece is worth more than no picture, as long as nobody is told it
/// is complete.
const INCOMPLETE_PICTURE: &str =
    "Snapdeck lost track of where that window had scrolled to, so it joined up as much as it could and the result may be missing a piece.";

/// What the user is told when there is nothing on screen to photograph.
const NO_WINDOW: &str = "Snapdeck found no window in front to capture.";

/// The layer an ordinary application window sits on.
///
/// The menu the user just clicked is a window too, and it is in front of
/// everything: a run that started while it was still up would photograph and
/// scroll a menu. Anything above this layer is the system's furniture rather
/// than content.
const NORMAL_WINDOW_LAYER: i32 = 0;

/// Posts the scroll a real wheel would post.
///
/// Not covered by a test of its own, on purpose. Exercising it means moving
/// whatever window happens to be in front of the machine running the test,
/// which is a side effect a test suite may not have; `run` holds every rule
/// worth checking and takes this as a trait so that it can be checked without
/// one.
struct WheelDriver;

impl ScrollDriver for WheelDriver {
    fn scroll(&self, lines: i32) -> Result<(), String> {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|()| "could not open an event source to scroll with".to_string())?;
        // Negated: a wheel reports positive when it is pushed away from the
        // user, which scrolls the content up. Downwards, which is what this
        // trait means by a positive `lines`, is the other sign.
        let event = CGEvent::new_scroll_event(source, ScrollEventUnit::LINE, 1, -lines, 0, 0)
            .map_err(|()| "could not build a scroll event".to_string())?;
        event.post(CGEventTapLocation::HID);
        Ok(())
    }
}

/// Starts a scrolling capture, or says why it cannot.
///
/// Returns immediately. This runs inside the tray's menu event handler, which
/// is the main thread, and a run is dozens of full-window captures with a
/// scroll between each pair.
pub fn request_scrolling_capture(app: &AppHandle) {
    // The one place the grant is asked about, and it is here rather than at
    // launch: the user has just said they want the feature, so this is the
    // moment the permission is about something they can see the point of.
    if !accessibility_is_granted() {
        report_failure(app, ACCESSIBILITY_DENIED);
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || capture_scrolling_window(&app));
}

/// One whole run: find the window, photograph it down its length, join the
/// pictures, and hand the result to the path every other capture takes.
fn capture_scrolling_window(app: &AppHandle) {
    let state = app.state::<AppState>();
    // The same slot region capture claims. Two runs at once would post their
    // scrolls into the same window and each would photograph the other's.
    let Some(_guard) = state.begin_capture() else {
        return;
    };

    let window = match frontmost_window(&state.capturer) {
        Ok(window) => window,
        Err(reason) => {
            report_failure(app, &reason);
            return;
        }
    };

    let capturer = &state.capturer;
    let outcome = run(
        || {
            capturer
                .capture(CaptureTarget::Window(window))
                .map_err(|err| err.to_string())
        },
        &WheelDriver,
        MAX_STEPS,
    );

    match &outcome.stopped {
        StopReason::ContentSettled => {}
        StopReason::StepLimit => report_failure(app, STEP_LIMIT),
        StopReason::CaptureFailed(reason) => report_failure(
            app,
            &format!("Snapdeck stopped capturing that window ({reason})."),
        ),
    }
    if outcome.frames.is_empty() {
        // Nothing was photographed at all, and the reason has just been said.
        return;
    }

    let (picture, incomplete) = match join(&outcome.frames) {
        Ok(joined) => joined,
        Err(reason) => {
            report_failure(app, &reason);
            return;
        }
    };
    deliver(app, picture, incomplete);
}

/// The window a run photographs, and the complaint when there is not one.
fn frontmost_window(capturer: &impl ScreenCapturer) -> Result<u32, String> {
    let windows = capturer.windows().map_err(|err| err.to_string())?;
    windows
        .iter()
        .find(|window| window.layer == NORMAL_WINDOW_LAYER)
        .map(|window| window.id)
        .ok_or_else(|| NO_WINDOW.to_string())
}

/// Joins a run's frames, falling back to the part of it that did line up.
///
/// The second half of the answer is whether anything was dropped, which is
/// what the user has to be told. `NoOverlap { index }` names the frame that
/// did not follow on from the one before it, so everything under that index is
/// still a run that joins.
fn join(frames: &[Frame]) -> Result<(Image, bool), String> {
    match stitch(frames, ScrollHint::default()) {
        Ok(picture) => Ok((picture, false)),
        Err(StitchError::NoOverlap { index }) => stitch(&frames[..index], ScrollHint::default())
            .map(|picture| (picture, true))
            .map_err(|err| format!("Snapdeck could not join the pictures of that window ({err}).")),
        Err(err) => Err(format!(
            "Snapdeck could not join the pictures of that window ({err})."
        )),
    }
}

/// Puts a finished picture through everything a capture goes through.
///
/// `intake::deliver` rather than a save of this module's own, which is the
/// whole point: the user's format, filename template and save folder apply to
/// a scrolling capture because it is saved by the same code that saves a page
/// delivered over the bridge and a region taken from the overlay.
fn deliver(app: &AppHandle, picture: Image, incomplete: bool) {
    let frame = frame_of(
        picture.data,
        picture.width,
        picture.height,
        picture.scale_factor,
    );
    // Read once, so a save landing in the settings window halfway through
    // cannot split this delivery between two folders.
    let settings = app.state::<AppState>().settings();
    let (directory, complaint) = settings::resolve_save_directory(app, &settings);
    if let Some(complaint) = complaint {
        report_failure(app, &complaint);
    }

    let Delivered { path, complaint } = intake::deliver(&frame, &directory, &settings, |frame| {
        crate::commands::copy_to_clipboard(app, frame)
    });

    if let Some(path) = &path {
        recents::record(app, path);
    }
    // Said even when everything else worked, for the reason a truncated page
    // is: handing the user a picture with a piece missing and saying nothing
    // is the one outcome this feature must not have.
    if incomplete {
        report_failure(app, INCOMPLETE_PICTURE);
    }
    if let Some(complaint) = &complaint {
        report_failure(app, complaint);
    }

    if let Some(path) = path {
        if settings.open_editor_after_capture {
            editor::open_editor(app, &path, frame.width, frame.height, frame.scale_factor);
        }
    }
}

/// A frame the way this module builds one, for the tests below and for turning
/// a stitched picture back into something the save path knows.
fn frame_of(data: Vec<u8>, width: u32, height: u32, scale_factor: f32) -> Frame {
    Frame {
        data,
        width,
        height,
        // Tightly packed: nothing in this module produces row padding, and a
        // stride that disagrees with the width is a frame the save path
        // cannot read.
        stride: width as usize * 4,
        pixel_format: PixelFormat::Rgba8,
        scale_factor,
        captured_at: SystemTime::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    /// What the loop did, in the order it did it.
    ///
    /// The order is the claim D3 is about: a scroll before the first capture
    /// loses the top of whatever is being photographed, and there is no way to
    /// tell from the finished picture that it happened.
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Step {
        Capture,
        Scroll(i32),
    }

    type Log = Rc<RefCell<Vec<Step>>>;

    fn log() -> Log {
        Rc::new(RefCell::new(Vec::new()))
    }

    /// Which kind of step each entry was, so a sequence can be asserted
    /// without the assertion quietly restating `SCROLL_LINES`.
    fn kinds(log: &Log) -> Vec<&'static str> {
        log.borrow()
            .iter()
            .map(|step| match step {
                Step::Capture => "capture",
                Step::Scroll(_) => "scroll",
            })
            .collect()
    }

    /// A picture whose every byte is `marker`, so two frames are the same
    /// picture exactly when their markers are.
    fn picture(marker: u8) -> Frame {
        frame_of(vec![marker; 4 * 4 * 4], 4, 4, 2.0)
    }

    /// A driver that writes down what it was asked for, and fails on the
    /// `fails_on`th call when it is given one, counting from one.
    struct FakeDriver {
        log: Log,
        fails_on: Option<usize>,
        calls: Cell<usize>,
    }

    impl FakeDriver {
        fn new(log: &Log) -> Self {
            Self {
                log: Rc::clone(log),
                fails_on: None,
                calls: Cell::new(0),
            }
        }

        fn failing_on(log: &Log, call: usize) -> Self {
            Self {
                fails_on: Some(call),
                ..Self::new(log)
            }
        }
    }

    impl ScrollDriver for FakeDriver {
        fn scroll(&self, lines: i32) -> Result<(), String> {
            self.calls.set(self.calls.get() + 1);
            if self.fails_on == Some(self.calls.get()) {
                return Err("the window would not take the scroll".to_string());
            }
            self.log.borrow_mut().push(Step::Scroll(lines));
            Ok(())
        }
    }

    /// A capture answering with `distinct` different pictures and then with the
    /// last of them for ever, which is what content that has stopped moving
    /// looks like from here.
    fn settling_capture(log: &Log, distinct: u8) -> impl FnMut() -> Result<Frame, String> {
        let log = Rc::clone(log);
        let mut taken = 0_u8;
        move || {
            log.borrow_mut().push(Step::Capture);
            taken += 1;
            Ok(picture(taken.min(distinct)))
        }
    }

    /// A capture answering with a different picture every time, which is what
    /// content that never stops moving looks like from here.
    fn endless_capture(log: &Log) -> impl FnMut() -> Result<Frame, String> {
        settling_capture(log, u8::MAX)
    }

    /// A capture that fails on the `fails_on`th call, counting from one.
    fn failing_capture(log: &Log, fails_on: usize) -> impl FnMut() -> Result<Frame, String> {
        let log = Rc::clone(log);
        let mut taken = 0_usize;
        move || {
            taken += 1;
            if taken == fails_on {
                return Err("the window went away".to_string());
            }
            log.borrow_mut().push(Step::Capture);
            Ok(picture(taken as u8))
        }
    }

    /// D1. Content that has stopped moving ends the loop, and the picture that
    /// merely repeated the one before it is not kept: four pictures of a
    /// window that stopped after four are four frames, not five and not
    /// `max_steps`.
    #[test]
    fn a_window_that_stops_moving_ends_the_run() {
        let log = log();
        let driver = FakeDriver::new(&log);
        let run = run(settling_capture(&log, 4), &driver, 20);

        assert_eq!(run.frames.len(), 4);
        assert_eq!(run.stopped, StopReason::ContentSettled);
    }

    /// D2. Content that never stops moving is stopped by the limit instead.
    /// Without one this test does not fail, it hangs.
    #[test]
    fn content_that_never_settles_is_stopped_by_the_limit() {
        let log = log();
        let driver = FakeDriver::new(&log);
        let run = run(endless_capture(&log), &driver, 5);

        assert_eq!(run.frames.len(), 5);
        assert_eq!(run.stopped, StopReason::StepLimit);
    }

    /// D3. One scroll between consecutive pictures, and none at all before the
    /// first one: a scroll before the first capture is how the top of the
    /// window is lost, silently and unrecoverably.
    #[test]
    fn the_first_picture_is_taken_before_anything_scrolls() {
        let log = log();
        let driver = FakeDriver::new(&log);
        let run = run(settling_capture(&log, 3), &driver, 20);

        assert_eq!(run.frames.len(), 3);
        assert_eq!(
            kinds(&log),
            ["capture", "scroll", "capture", "scroll", "capture", "scroll", "capture"]
        );
        assert!(
            log.borrow()
                .iter()
                .all(|step| !matches!(step, Step::Scroll(lines) if *lines <= 0)),
            "every scroll goes downwards: {:?}",
            log.borrow()
        );
    }

    /// D4. A capture that fails partway through leaves the pictures already
    /// taken in hand. Spec 9 offers a partial picture with a warning; an early
    /// return would throw the whole run away over its last step.
    #[test]
    fn a_failed_capture_keeps_the_pictures_already_taken() {
        let log = log();
        let driver = FakeDriver::new(&log);
        let run = run(failing_capture(&log, 3), &driver, 20);

        assert_eq!(run.frames.len(), 2);
        let StopReason::CaptureFailed(reason) = run.stopped else {
            panic!("a failed capture has to say so: {:?}", run.stopped);
        };
        assert!(reason.contains("the window went away"), "{reason}");
    }

    /// D5. A driver that fails is the same kind of partial run: the pictures
    /// survive and the loop stops rather than photographing the same place
    /// until the limit runs out.
    #[test]
    fn a_failed_scroll_keeps_the_pictures_already_taken() {
        let log = log();
        let driver = FakeDriver::failing_on(&log, 2);
        let run = run(endless_capture(&log), &driver, 20);

        assert_eq!(run.frames.len(), 2);
        let StopReason::CaptureFailed(reason) = run.stopped else {
            panic!("a failed scroll has to say so: {:?}", run.stopped);
        };
        assert!(reason.contains("would not take the scroll"), "{reason}");
    }

    /// D6. The user has to be told this is guesswork, and the place they are
    /// told is the only place they meet the feature. The second half is what
    /// stops the label and the menu drifting apart: a tray built from its own
    /// string literal would keep saying whatever it said the day it was
    /// written.
    #[test]
    fn the_tray_item_says_it_is_experimental() {
        const TRAY: &str = include_str!("tray.rs");

        assert!(
            FULLPAGE_MENU_LABEL.contains("Experimental"),
            "{FULLPAGE_MENU_LABEL}"
        );
        assert!(
            TRAY.contains("FULLPAGE_MENU_LABEL"),
            "the tray has to build the item from the constant"
        );
        assert!(
            !TRAY.contains("Capture Scrolling Window"),
            "and not from a string literal of its own"
        );
    }
}
