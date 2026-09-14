//! ScreenCaptureKit-backed screen recording.
//!
//! No encoder is written here. `SCRecordingOutput` is attached to the stream
//! and Apple writes the H.264/MP4 file; this module decides what the encoder is
//! told, counts what the stream delivers, and takes the whole thing apart in
//! the one order that leaves a playable file.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use screencapturekit::cm::SCFrameStatus;
use screencapturekit::prelude::*;
use screencapturekit::recording_output::{
    SCRecordingOutput, SCRecordingOutputCodec, SCRecordingOutputConfiguration,
    SCRecordingOutputFileType,
};

use crate::{AudioSources, CaptureError, Recording, RecordingProgress, RecordingSummary};

/// Frames per second a recording is capped at.
///
/// ScreenCaptureKit's own default is uncapped, which asks the encoder for
/// every composited frame on a moving screen. There is no setting for this in
/// this release; the design fixed it at 30 (5.3).
pub const RECORDING_FPS: u32 = 30;

/// The stream configuration a recording runs with.
///
/// Taking plain numbers rather than a `Resolved` so that the three things the
/// design fixed can be read back without a display and without a content
/// filter: the frame cap, the cursor, and the size.
///
/// The cursor is **on**, which is the opposite of `capture()`. In a still
/// screenshot the pointer is clutter; in a screen recording it is the content.
///
/// Sound is two flags on this same configuration and nothing more. With
/// `audio.system` on, ScreenCaptureKit takes in what the Mac is playing and
/// `SCRecordingOutput` writes it into the same movie. This process's own sound
/// is always left out, which changes nothing while there is no sound. No
/// output handler is attached for it: `tear_down` only gets the movie
/// finalised when no handler is left on the stream, and a sound handler would
/// be one more thing standing in the way of that.
pub(crate) fn recording_config(
    width: u32,
    height: u32,
    source_rect: Option<CGRect>,
    audio: AudioSources,
) -> SCStreamConfiguration {
    let config = SCStreamConfiguration::new();
    let config = match source_rect {
        Some(source_rect) => config.with_source_rect(source_rect),
        None => config,
    };
    config
        .with_width(width)
        .with_height(height)
        .with_fps(RECORDING_FPS)
        .with_shows_cursor(true)
        .with_captures_audio(audio.system)
        .with_excludes_current_process_audio(true)
}

/// What the encoder is told, which is all three things it accepts.
///
/// `None` on a system without `SCRecordingOutput`, which is every macOS before
/// 15.0.
pub(crate) fn recording_output_config(path: &Path) -> Option<SCRecordingOutputConfiguration> {
    Some(
        SCRecordingOutputConfiguration::try_new()?
            .with_output_url(path)
            .with_video_codec(SCRecordingOutputCodec::H264)
            .with_output_file_type(SCRecordingOutputFileType::MP4),
    )
}

/// The refusal a machine without `SCRecordingOutput` gets.
pub(crate) fn unsupported() -> CaptureError {
    CaptureError::Unsupported("screen recording needs macOS 15 or later".to_string())
}

/// The two things taking a recording apart does, as a trait so the order can
/// be driven without a stream.
///
/// A live `SCStream` is not something a unit test can arrange, and the claim
/// worth testing here is not the FFI, it is the order.
pub(crate) trait RecordingTeardown {
    /// Marks the recording as taken apart, and answers `true` only the first
    /// time. `stop` and `cancel` consume the recording and its `Drop` runs
    /// straight after them, so without this every stop would be two.
    fn claim_teardown(&mut self) -> bool;
    /// Detaches the frame counter. `Ok` when there was nothing to detach.
    fn detach_frame_handler(&mut self) -> Result<(), String>;
    /// Removes the recording output, which is what finalises the movie.
    fn remove_recording_output(&mut self) -> Result<(), String>;
}

/// Takes a recording apart in the one order that leaves a playable file.
///
/// **This order is not a style choice.** `screencapturekit` 9.0.1's
/// `SCStream::remove_recording_output` only stops the capture and waits for the
/// movie to reach a terminal state when three things hold at the moment it is
/// called: the stream is still capturing, **no output handlers are attached**,
/// and this is the last recording output. Read from the crate's own source:
///
/// ```text
/// if context.capturing.load(..) && !context.has_handlers()
///     && context.recording_outputs.load(..) == 1 { stop_capture(); wait_until_terminal(); }
/// ```
///
/// Miss any one of the three and the call returns without waiting, the stream
/// is dropped under a movie that was never finalised, and the `.mp4` has no
/// `moov` atom: QuickTime refuses to open it and the user's recording is gone.
///
/// So the frame counter comes off first. And `stop_capture` is deliberately
/// **never called here**: `capturing == false` is the second way to skip the
/// same wait, and a well-meaning "stop it cleanly first" line is exactly how
/// this gets broken.
///
/// A failed detach does not skip the removal. The movie has to be finalised
/// even when the handler will not come off; the worst a stuck handler costs is
/// a few discarded samples, and the alternative costs the whole recording.
///
/// Once per recording. A second call answers `Ok` and touches nothing: the
/// output is already gone, and whatever the first call said is what counted.
pub(crate) fn tear_down<T: RecordingTeardown>(teardown: &mut T) -> Result<(), String> {
    if !teardown.claim_teardown() {
        return Ok(());
    }
    let detached = teardown.detach_frame_handler();
    teardown.remove_recording_output()?;
    detached
}

/// Takes apart a recording that is going away, for its `Drop`.
///
/// Nobody is left to hear a failure, so it is not reported. What matters is
/// that the stream stops writing into a file nobody will ever rename.
fn release<T: RecordingTeardown>(teardown: &mut T) {
    let _ = tear_down(teardown);
}

/// What the stream has delivered, shared between the handler that counts and
/// the recording that is asked.
///
/// Two atomics rather than a lock: the handler runs on a ScreenCaptureKit
/// dispatch queue on every delivered sample, and `progress` is read from the
/// menu bar's timer.
#[derive(Debug, Default)]
pub(super) struct FrameCounts {
    frames: AtomicU64,
    incomplete: AtomicU64,
}

impl FrameCounts {
    fn progress(&self) -> RecordingProgress {
        RecordingProgress {
            frames: self.frames.load(Ordering::Relaxed),
            incomplete: self.incomplete.load(Ordering::Relaxed),
        }
    }
}

/// The stream output handler that counts samples and does nothing else.
///
/// It never touches `image_buffer()` and never copies the `CMSampleBuffer`:
/// the frames belong to `SCRecordingOutput`, which takes the `IOSurface`
/// directly, and pulling one into this process would be the 2 GB/s memcpy the
/// design refused (4.2).
pub(super) struct FrameCounter {
    counts: Arc<FrameCounts>,
}

impl FrameCounter {
    pub(super) fn new(counts: Arc<FrameCounts>) -> Self {
        Self { counts }
    }
}

impl SCStreamOutputTrait for FrameCounter {
    fn did_output_sample_buffer(
        &self,
        sample_buffer: CMSampleBuffer,
        _of_type: SCStreamOutputType,
    ) {
        self.counts.frames.fetch_add(1, Ordering::Relaxed);
        if sample_buffer.frame_status() != Some(SCFrameStatus::Complete) {
            self.counts.incomplete.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Which failure a stopped recording reports, when there is more than one.
///
/// The encoder's own words win. It is the half of this that knows why: a disk
/// that filled up says so, while taking the stream apart afterwards fails with
/// something about a stream, which is true and useless to the person reading
/// it.
fn failure_to_report(encoder: Option<String>, torn_down: Result<(), String>) -> Option<String> {
    encoder.or_else(|| torn_down.err())
}

/// A live ScreenCaptureKit recording.
pub struct MacRecording {
    /// The stream the movie and the counter are attached to.
    pub(super) stream: SCStream,
    /// The movie output. Removing it is what finalises the file.
    pub(super) movie: SCRecordingOutput,
    /// The frame counter's registration, or `None` when the stream refused to
    /// take one. A counter is a diagnostic, never a precondition.
    pub(super) handler: Option<usize>,
    /// What the counter has counted.
    pub(super) counts: Arc<FrameCounts>,
    /// What the encoder's delegate reported, which is the only way a full disk
    /// or a failed encode reaches the user.
    pub(super) failure: Arc<Mutex<Option<String>>>,
    /// The file this was told to write, which is never a name it chose.
    pub(super) path: PathBuf,
    /// Whether `tear_down` has run, so `Drop` after `stop` or `cancel` does not
    /// run it again.
    pub(super) torn_down: bool,
}

/// A safety net, not the way a recording ends. A recording dropped without
/// `stop` or `cancel`, by a `?`, a panic or a forgotten early return in whoever
/// held it, would otherwise leave its stream capturing into a hidden file.
impl Drop for MacRecording {
    fn drop(&mut self) {
        release(self);
    }
}

impl RecordingTeardown for MacRecording {
    fn claim_teardown(&mut self) -> bool {
        !std::mem::replace(&mut self.torn_down, true)
    }

    fn detach_frame_handler(&mut self) -> Result<(), String> {
        let Some(handler) = self.handler.take() else {
            return Ok(());
        };
        self.stream
            .try_remove_output_handler(handler, SCStreamOutputType::Screen)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn remove_recording_output(&mut self) -> Result<(), String> {
        self.stream
            .remove_recording_output(&self.movie)
            .map_err(|error| error.to_string())
    }
}

impl Recording for MacRecording {
    fn progress(&self) -> RecordingProgress {
        self.counts.progress()
    }

    fn stop(mut self: Box<Self>) -> Result<RecordingSummary, CaptureError> {
        // Taken apart first and always: the stream comes apart whatever the
        // encoder had to say.
        let torn_down = tear_down(&mut *self);
        let encoder = self
            .failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(failure) = failure_to_report(encoder, torn_down) {
            return Err(CaptureError::Platform(failure));
        }
        let bytes = std::fs::metadata(&self.path)
            .map_err(|error| {
                CaptureError::Platform(format!("the recording was not written: {error}"))
            })?
            .len();
        Ok(RecordingSummary {
            path: self.path.clone(),
            bytes,
            frames: self.counts.progress().frames,
        })
    }

    fn cancel(mut self: Box<Self>) -> Result<(), CaptureError> {
        // The file is left where it is: this crate was handed a path and the
        // caller that chose the name is the one that takes it back.
        tear_down(&mut *self).map_err(CaptureError::Platform)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::process::Command;
    use std::rc::Rc;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use core_graphics::display::CGMainDisplayID;

    use super::*;
    use crate::macos::MacCapturer;
    use crate::{CaptureTarget, ScreenCapturer};

    /// This file's own source, so a test can claim that a call is nowhere in
    /// it and not only that some other call computes the right thing.
    const SOURCE: &str = include_str!("recording.rs");

    /// The capturer's source, for the one claim about where `record` asks its
    /// first question.
    const CAPTURER_SOURCE: &str = include_str!("mod.rs");

    /// Everything in this file that is not a test.
    fn production_source() -> &'static str {
        let end = SOURCE
            .find("#[cfg(test)]")
            .expect("the test module marker is in this file");
        &SOURCE[..end]
    }

    /// The text of the item `marker` opens in the capturer's source, from the
    /// marker to the next item at the same nesting or the end of the enclosing
    /// block.
    ///
    /// The same deliberately textual slicing `mod.rs`'s own tests use.
    fn capturer_body(marker: &str) -> &'static str {
        let start = CAPTURER_SOURCE
            .find(marker)
            .unwrap_or_else(|| panic!("{marker} is in the capturer's source"));
        let rest = &CAPTURER_SOURCE[start..];
        let next_method = rest.find("\n    fn ");
        let closing_brace = rest.find("\n}");
        let end = match (next_method, closing_brace) {
            (Some(method), Some(brace)) if method < brace => method,
            (_, Some(brace)) => brace + "\n}".len(),
            (Some(method), None) => method,
            (None, None) => rest.len(),
        };
        let body = rest[..end].trim_end();
        // An empty or truncated slice would let the claim below pass without
        // reading anything, so the slicing itself is asserted first.
        assert!(
            body.len() > marker.len() && body.ends_with('}'),
            "{marker} did not slice to a body, got {body:?}"
        );
        body
    }

    /// A teardown that records the order it was driven in and can be made to
    /// fail at either step.
    #[derive(Default)]

    struct FakeTeardown {
        calls: Vec<&'static str>,
        detach_error: Option<String>,
        removal_error: Option<String>,
        torn_down: bool,
    }

    impl RecordingTeardown for FakeTeardown {
        fn claim_teardown(&mut self) -> bool {
            !std::mem::replace(&mut self.torn_down, true)
        }

        fn detach_frame_handler(&mut self) -> Result<(), String> {
            self.calls.push("detach_frame_handler");
            match &self.detach_error {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
        }

        fn remove_recording_output(&mut self) -> Result<(), String> {
            self.calls.push("remove_recording_output");
            match &self.removal_error {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
        }
    }

    /// C1
    #[test]
    fn the_frame_counter_comes_off_before_the_movie_is_finalised() {
        let mut teardown = FakeTeardown::default();
        tear_down(&mut teardown).expect("both steps succeed");
        assert_eq!(
            teardown.calls,
            vec!["detach_frame_handler", "remove_recording_output"]
        );
    }

    /// C2
    #[test]
    fn a_handler_that_will_not_come_off_does_not_skip_the_finalisation() {
        let mut teardown = FakeTeardown {
            detach_error: Some("the handler is stuck".to_string()),
            ..FakeTeardown::default()
        };
        assert_eq!(
            tear_down(&mut teardown),
            Err("the handler is stuck".to_string())
        );
        assert_eq!(
            teardown.calls,
            vec!["detach_frame_handler", "remove_recording_output"]
        );
    }

    /// C3
    #[test]
    fn a_failed_finalisation_is_reported_even_when_the_counter_came_off() {
        let mut teardown = FakeTeardown {
            removal_error: Some("the movie never reached a terminal state".to_string()),
            ..FakeTeardown::default()
        };
        assert_eq!(
            tear_down(&mut teardown),
            Err("the movie never reached a terminal state".to_string())
        );
        assert_eq!(
            teardown.calls,
            vec!["detach_frame_handler", "remove_recording_output"]
        );
    }

    /// A teardown owned the way `MacRecording` owns its stream: dropping it
    /// runs the same `release` `MacRecording`'s `Drop` runs, and the calls land
    /// in a log that outlives it.
    struct DroppedTeardown {
        calls: Rc<RefCell<Vec<&'static str>>>,
        torn_down: bool,
    }

    impl RecordingTeardown for DroppedTeardown {
        fn claim_teardown(&mut self) -> bool {
            !std::mem::replace(&mut self.torn_down, true)
        }

        fn detach_frame_handler(&mut self) -> Result<(), String> {
            self.calls.borrow_mut().push("detach_frame_handler");
            Ok(())
        }

        fn remove_recording_output(&mut self) -> Result<(), String> {
            self.calls.borrow_mut().push("remove_recording_output");
            Ok(())
        }
    }

    impl Drop for DroppedTeardown {
        fn drop(&mut self) {
            release(self);
        }
    }

    /// D1. A recording dropped without `stop` or `cancel`, by a `?`, a panic or
    /// an early return in whoever held it, is taken apart exactly once, and
    /// `MacRecording` really is dropped that way.
    #[test]
    fn a_recording_nobody_took_apart_is_taken_apart_once_when_dropped() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        drop(DroppedTeardown {
            calls: Rc::clone(&calls),
            torn_down: false,
        });
        assert_eq!(
            *calls.borrow(),
            vec!["detach_frame_handler", "remove_recording_output"]
        );

        let drop_impl = production_source()
            .split_once("impl Drop for MacRecording")
            .expect("a dropped MacRecording takes its stream apart")
            .1
            .split_once("\n}\n")
            .expect("the Drop impl ends at a closing brace in column zero")
            .0;
        assert!(
            drop_impl.contains("release(self)"),
            "MacRecording's Drop has to release the stream"
        );
    }

    /// D2. `stop` and `cancel` consume the recording, so its `Drop` runs right
    /// after they took it apart; a second removal of an output that is already
    /// gone is not a teardown.
    #[test]
    fn a_recording_already_taken_apart_is_not_taken_apart_again_when_dropped() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut recording = DroppedTeardown {
            calls: Rc::clone(&calls),
            torn_down: false,
        };
        tear_down(&mut recording).expect("both steps succeed");
        drop(recording);
        assert_eq!(
            *calls.borrow(),
            vec!["detach_frame_handler", "remove_recording_output"]
        );
    }

    /// C4
    #[test]
    fn nothing_in_this_file_stops_the_capture_itself() {
        // Built from two halves so that this line is not the very thing it
        // goes looking for.
        let forbidden = concat!("stop_", "capture");
        // Comment lines are exempt on purpose: `tear_down`'s documentation
        // names this trap out loud, which is the whole reason the trap is not
        // fallen into, and prose cannot call anything.
        let code: Vec<&str> = production_source()
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect();
        assert!(
            code.len() > 20,
            "the production source did not slice, so this claim reads nothing"
        );
        for line in code {
            assert!(
                !line.contains(forbidden),
                "this file stops the capture itself, which skips the wait that finalises the movie: {line}"
            );
        }
    }

    /// C5
    #[test]
    fn a_recording_is_capped_at_thirty_frames_a_second() {
        assert_eq!(RECORDING_FPS, 30);
    }

    /// C6
    #[test]
    fn the_stream_configuration_carries_the_three_fixed_decisions() {
        let config = recording_config(800, 600, None, AudioSources::default());
        assert_eq!(config.width(), 800);
        assert_eq!(config.height(), 600);
        assert_eq!(config.fps(), 30);
        // The opposite of `capture()`: in a recording the pointer is content.
        assert!(config.shows_cursor());
    }

    /// C7
    #[test]
    fn only_a_region_recording_crops() {
        let source_rect = CGRect {
            origin: CGPoint { x: 300.0, y: 100.0 },
            size: CGSize {
                width: 640.0,
                height: 480.0,
            },
        };
        assert_eq!(
            recording_config(640, 480, Some(source_rect), AudioSources::default()).source_rect(),
            source_rect
        );
        let uncropped = recording_config(800, 600, None, AudioSources::default()).source_rect();
        assert_eq!(uncropped.size.width, 0.0);
        assert_eq!(uncropped.size.height, 0.0);
    }

    /// C8
    #[test]
    fn the_encoder_is_told_h264_and_mp4_and_where_to_write() {
        assert!(
            SCRecordingOutput::is_available(),
            "this test needs macOS 15.0 or later; a silently skipped test is a test that does not exist"
        );
        let config = recording_output_config(Path::new("/tmp/a.mp4"))
            .expect("recording output is available on this system");
        assert_eq!(config.video_codec().identifier(), "avc1");
        assert_eq!(config.output_file_type().identifier(), "public.mpeg-4");
        assert_eq!(config.output_url(), Some(PathBuf::from("/tmp/a.mp4")));
    }

    /// C9
    #[test]
    fn a_machine_that_cannot_record_is_told_which_version_it_needs() {
        match unsupported() {
            CaptureError::Unsupported(message) => assert!(
                message.contains("15"),
                "the refusal does not say which version is needed: {message}"
            ),
            other => panic!("a machine without recording output got {other:?}"),
        }
    }

    /// C10
    #[test]
    fn record_asks_whether_it_can_record_before_it_touches_the_framework() {
        let body = capturer_body("fn record(");
        let asked = body
            .find("is_available()")
            .expect("record never asks whether recording output is available");
        let framework = ["SCRecordingOutputConfiguration", "SCStream"]
            .iter()
            .filter_map(|marker| body.find(marker))
            .min()
            .expect("record never names the framework, so this claim reads nothing");
        assert!(
            asked < framework,
            "record touches the framework before asking whether it is there, which panics on macOS 14"
        );
    }

    /// A scratch directory of this test's own, so a recording leaves nothing
    /// behind anywhere a person looks.
    fn scratch_directory() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("snapdeck-c11-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a scratch directory can be created");
        directory
    }

    /// Whether the movie carries a `moov` atom, which is the difference
    /// between a file a player opens and a file it refuses.
    ///
    /// Head and tail only: `AVAssetWriter` puts the atom at one end or the
    /// other, and scanning the whole movie would find the string in compressed
    /// video data by chance.
    fn has_moov_atom(movie: &[u8]) -> bool {
        const WINDOW: usize = 64 * 1024;
        let head = &movie[..movie.len().min(WINDOW)];
        let tail = &movie[movie.len().saturating_sub(WINDOW)..];
        [head, tail]
            .iter()
            .any(|part| part.windows(4).any(|bytes| bytes == b"moov"))
    }

    /// C11
    #[test]
    #[ignore = "records the real screen, so it needs the screen recording grant"]
    fn a_real_recording_leaves_a_file_a_player_can_open() {
        assert!(
            SCRecordingOutput::is_available(),
            "this test needs macOS 15.0 or later; a silently skipped test is a test that does not exist"
        );
        let directory = scratch_directory();
        let path = directory.join("recording.mp4");
        let recording = MacCapturer::new()
            .record(
                CaptureTarget::Display(unsafe { CGMainDisplayID() }),
                AudioSources::default(),
                &path,
            )
            .expect("the primary display can be recorded");
        std::thread::sleep(Duration::from_secs(2));
        let summary = recording.stop().expect("the recording stops");
        let movie = std::fs::read(&path).expect("the recording was written");
        // Cleaned up before the claims, so a failing claim still leaves the
        // user's temporary directory as it found it.
        std::fs::remove_dir_all(&directory).expect("the scratch directory can be removed");

        assert_eq!(summary.path, path);
        assert!(summary.bytes > 0, "the recording is empty");
        assert!(summary.frames > 0, "the stream delivered nothing");
        assert!(
            has_moov_atom(&movie),
            "the movie has no moov atom, so no player will open it"
        );
    }
    #[test]
    fn the_encoder_says_why_a_recording_failed_rather_than_the_teardown() {
        // Both went wrong at once, which is what a full disk looks like: the
        // encoder stopped because there was no room, and taking the stream
        // apart afterwards failed too. Only one of them can be shown, and the
        // one that knows about the disk is not the one about the stream.
        assert_eq!(
            failure_to_report(
                Some("the disk is full".to_string()),
                Err("the stream could not be taken apart".to_string()),
            ),
            Some("the disk is full".to_string())
        );
        // With nothing from the encoder, the teardown is all there is.
        assert_eq!(
            failure_to_report(None, Err("the stream could not be taken apart".to_string())),
            Some("the stream could not be taken apart".to_string())
        );
        // And a recording that ended cleanly reports nothing at all.
        assert_eq!(failure_to_report(None, Ok(())), None);
    }

    /// Everything in the capturer's source that is not a test.
    fn capturer_production_source() -> &'static str {
        let end = CAPTURER_SOURCE
            .find("#[cfg(test)]")
            .expect("the test module marker is in the capturer's source");
        &CAPTURER_SOURCE[..end]
    }

    /// AU1
    #[test]
    fn a_recording_asked_for_system_audio_takes_it_in_and_leaves_its_own_out() {
        let config = recording_config(800, 600, None, AudioSources { system: true });
        assert!(
            config.captures_audio(),
            "system audio was asked for and the stream does not take it in"
        );
        assert!(
            config.excludes_current_process_audio(),
            "the stream would record Snapdeck's own sound"
        );
        // Adding sound did not move any of the picture's decisions.
        assert_eq!(config.width(), 800);
        assert_eq!(config.height(), 600);
        assert_eq!(config.fps(), 30);
        assert!(config.shows_cursor());
    }

    /// AU2
    #[test]
    fn a_recording_asked_for_silence_stays_silent() {
        let config = recording_config(800, 600, None, AudioSources { system: false });
        assert!(
            !config.captures_audio(),
            "silence was asked for and the stream takes in sound"
        );
        assert!(config.excludes_current_process_audio());
    }

    /// AU3
    #[test]
    fn a_caller_that_says_nothing_about_sound_gets_a_silent_recording() {
        assert_eq!(AudioSources::default(), AudioSources { system: false });
    }

    /// AU4
    #[test]
    fn record_hands_the_callers_sound_to_the_stream_configuration() {
        let body = capturer_body("fn record(");
        let call = body
            .split_once("recording_config(")
            .expect("record never builds a stream configuration, so this claim reads nothing")
            .1;
        let mut depth = 0usize;
        let end = call
            .char_indices()
            .find(|&(_, character)| match character {
                '(' => {
                    depth += 1;
                    false
                }
                ')' if depth == 0 => true,
                ')' => {
                    depth -= 1;
                    false
                }
                _ => false,
            })
            .map(|(at, _)| at)
            .expect("the recording_config call closes");
        let arguments = &call[..end];
        assert!(
            arguments
                .split(',')
                .any(|argument| argument.trim() == "audio"),
            "record does not pass the caller's sound to the stream configuration: {arguments}"
        );
        assert!(
            !body.contains("AudioSources::default()"),
            "record replaces the caller's sound with silence"
        );
    }

    /// AU5
    #[test]
    fn no_sound_handler_is_attached_to_a_recording_stream() {
        // Built from halves so that this test is not the very thing it goes
        // looking for.
        let forbidden = [
            concat!("SCStreamOutputType::", "Audio"),
            concat!("SCStreamOutputType::", "Microphone"),
        ];
        for (file, source) in [
            ("recording.rs", production_source()),
            ("mod.rs", capturer_production_source()),
        ] {
            let code: Vec<&str> = source
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect();
            assert!(
                code.len() > 20,
                "the production source of {file} did not slice, so this claim reads nothing"
            );
            for line in code {
                for handler in forbidden {
                    assert!(
                        !line.contains(handler),
                        "{file} attaches a sound handler, and a handler left on the stream skips the wait that finalises the movie: {line}"
                    );
                }
            }
        }
    }

    /// Where a `hdlr` box names its handler, counted from the start of its
    /// tag: the tag itself, version and flags, and `pre_defined` are four
    /// bytes each, and the handler type is the four after them.
    const HANDLER_TYPE_AFTER_TAG: usize = 12;

    /// How many sound tracks a movie carries: every `hdlr` box whose handler
    /// type is `soun` is one.
    fn sound_track_count(movie: &[u8]) -> usize {
        movie
            .windows(4)
            .enumerate()
            .filter(|(at, tag)| {
                *tag == b"hdlr"
                    && movie.get(at + HANDLER_TYPE_AFTER_TAG..at + HANDLER_TYPE_AFTER_TAG + 4)
                        == Some(b"soun".as_slice())
            })
            .count()
    }

    /// A `hdlr` box as an MP4 writer lays it out: size, tag, version and
    /// flags, `pre_defined`, handler type, three reserved words, empty name.
    fn handler_box(handler_type: &[u8; 4]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(b"hdlr");
        body.extend_from_slice(&[0; 4]);
        body.extend_from_slice(&[0; 4]);
        body.extend_from_slice(handler_type);
        body.extend_from_slice(&[0; 12]);
        body.push(0);
        let size = u32::try_from(body.len() + 4).expect("a handler box fits in its size field");
        [size.to_be_bytes().to_vec(), body].concat()
    }

    /// HD1
    #[test]
    fn the_sound_track_counter_counts_sound_handlers_and_nothing_else() {
        assert_eq!(sound_track_count(&handler_box(b"soun")), 1);
        assert_eq!(sound_track_count(&handler_box(b"vide")), 0);
        let movie = [
            handler_box(b"soun"),
            handler_box(b"soun"),
            handler_box(b"vide"),
        ]
        .concat();
        assert_eq!(sound_track_count(&movie), 2);
        // A video handler with `soun` written eight bytes after the tag, in
        // `pre_defined`, where no handler type is.
        let mut malformed = handler_box(b"vide");
        malformed[4 + 8..4 + 12].copy_from_slice(b"soun");
        assert_eq!(sound_track_count(&malformed), 0);
    }

    /// A sound every Mac ships with, played while C12 records.
    const TEST_SOUND: &str = "/System/Library/Sounds/Glass.aiff";

    /// The system's own command line player.
    const PLAYER: &str = "/usr/bin/afplay";

    /// How often C12 starts the sound again, so it is playing the whole time.
    const SOUND_INTERVAL: Duration = Duration::from_millis(500);

    /// How long each half of a C12 recording runs: the progress is read after
    /// the first, the recording stops after the second.
    const HALF_RECORDING: Duration = Duration::from_secs(2);

    /// What C12 read off one recording before its file was removed.
    struct SoundedRecording {
        frames_mid: u64,
        summary: Result<RecordingSummary, CaptureError>,
        movie: std::io::Result<Vec<u8>>,
    }

    /// Records the primary display for two halves while the test sound plays
    /// over and over, and removes the file before anything is claimed.
    fn record_while_a_sound_plays(audio: AudioSources) -> SoundedRecording {
        let directory = scratch_directory();
        let path = directory.join("recording.mp4");
        let recording = MacCapturer::new()
            .record(
                CaptureTarget::Display(unsafe { CGMainDisplayID() }),
                audio,
                &path,
            )
            .expect("the primary display can be recorded");
        let playing = Arc::new(AtomicBool::new(true));
        let player = {
            let playing = Arc::clone(&playing);
            std::thread::spawn(move || {
                let mut players = Vec::new();
                while playing.load(Ordering::Relaxed) {
                    if let Ok(child) = Command::new(PLAYER).arg(TEST_SOUND).spawn() {
                        players.push(child);
                    }
                    std::thread::sleep(SOUND_INTERVAL);
                }
                // Every player is waited for, so none outlives the test.
                for mut child in players {
                    let _ = child.wait();
                }
            })
        };
        std::thread::sleep(HALF_RECORDING);
        let frames_mid = recording.progress().frames;
        std::thread::sleep(HALF_RECORDING);
        let summary = recording.stop();
        playing.store(false, Ordering::Relaxed);
        let joined = player.join();
        let movie = std::fs::read(&path);
        // Cleaned up before the claims, so a failing claim still leaves the
        // user's temporary directory as it found it.
        std::fs::remove_dir_all(&directory).expect("the scratch directory can be removed");
        assert!(joined.is_ok(), "the sound player thread panicked");
        SoundedRecording {
            frames_mid,
            summary,
            movie,
        }
    }

    /// C12
    #[test]
    #[ignore = "records the real screen and sound, so it needs the screen and system audio recording grant"]
    fn a_real_recording_takes_in_system_audio_only_when_asked() {
        assert!(
            SCRecordingOutput::is_available(),
            "this test needs macOS 15.0 or later; a silently skipped test is a test that does not exist"
        );
        assert!(
            Path::new(TEST_SOUND).exists(),
            "{TEST_SOUND} is not on this machine, so nothing would be playing; a silent test of sound is a test that does not exist"
        );
        let runs = [
            (
                "system audio",
                record_while_a_sound_plays(AudioSources { system: true }),
            ),
            (
                "silence",
                record_while_a_sound_plays(AudioSources::default()),
            ),
        ];
        let mut sound_tracks = Vec::new();
        for (name, run) in runs {
            // Started first: a recording that never ran proves nothing about
            // what it would have taken in.
            assert!(
                run.frames_mid > 0,
                "the {name} recording had delivered nothing two seconds in"
            );
            let summary = run.summary.expect("the recording stops");
            let movie = run.movie.expect("the recording was written");
            let tracks = sound_track_count(&movie);
            eprintln!(
                "C12 {name}: frames_mid={} frames={} bytes={} sound_tracks={tracks}",
                run.frames_mid, summary.frames, summary.bytes
            );
            assert!(summary.frames > 0, "the {name} stream delivered nothing");
            assert!(summary.bytes > 0, "the {name} recording is empty");
            assert!(
                has_moov_atom(&movie),
                "the {name} movie has no moov atom, so no player will open it"
            );
            sound_tracks.push(tracks);
        }
        assert_eq!(
            sound_tracks[0], 1,
            "the recording asked for system audio does not carry one sound track"
        );
        assert_eq!(
            sound_tracks[1], 0,
            "the recording asked for silence carries a sound track"
        );
    }
}
