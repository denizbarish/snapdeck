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

use crate::{CaptureError, Recording, RecordingProgress, RecordingSummary};

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
pub(crate) fn recording_config(
    width: u32,
    height: u32,
    source_rect: Option<CGRect>,
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
pub(crate) fn tear_down<T: RecordingTeardown>(teardown: &mut T) -> Result<(), String> {
    let detached = teardown.detach_frame_handler();
    teardown.remove_recording_output()?;
    detached
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

/// A live ScreenCaptureKit recording.
/// Which failure a stopped recording reports, when there is more than one.
///
/// The encoder's own words win. It is the half of this that knows why: a disk
/// that filled up says so, while taking the stream apart afterwards fails with
/// something about a stream, which is true and useless to the person reading
/// it.
fn failure_to_report(encoder: Option<String>, torn_down: Result<(), String>) -> Option<String> {
    encoder.or_else(|| torn_down.err())
}

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
}

impl RecordingTeardown for MacRecording {
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
    }

    impl RecordingTeardown for FakeTeardown {
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
        let config = recording_config(800, 600, None);
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
            recording_config(640, 480, Some(source_rect)).source_rect(),
            source_rect
        );
        let uncropped = recording_config(800, 600, None).source_rect();
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
            .record(CaptureTarget::Display(unsafe { CGMainDisplayID() }), &path)
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
}
