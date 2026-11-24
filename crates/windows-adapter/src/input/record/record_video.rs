// Part of Agent Record Adapter (see NOTICE).
//! The video of a `record` session.
//!
//! A thread copies the recorded area of the screen with GDI, draws the
//! pointer on it, and writes each frame to an H.264 MP4 through a Media
//! Foundation sink writer. Each frame carries the time it was taken, so the
//! video and the recording's `t_ms` share one clock (`offset_ms` apart).
//! The MP4 is fragmented, so a session that is killed (its caller's
//! process tree closed) still leaves a playable video up to that moment.
//! GDI reads the composed screen, so the video shows what the person saw
//! without the border a Windows Graphics Capture session draws.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use agent_desktop_core::{AdapterError, ErrorCode, RecordedVideo};

use super::record_video_area::Area;
use super::record_video_frame::Grabber;

const FRAMES_PER_SECOND: u32 = 12;
const BITS_PER_PIXEL_SECOND: u32 = 3;
const HUNDRED_NS_PER_MS: i64 = 10_000;

/// A video being written.
pub(super) struct VideoRecorder {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<u64, AdapterError>>>,
    path: PathBuf,
    area: Area,
    offset_ms: u64,
}

impl VideoRecorder {
    /// Starts writing `path` once its encoder is ready; `session_start` is
    /// the recording's clock.
    pub(super) fn start(
        path: &Path,
        area: Area,
        session_start: Instant,
    ) -> Result<Self, AdapterError> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready, started) = channel();
        let thread = {
            let stop = Arc::clone(&stop);
            let path = path.to_path_buf();
            std::thread::Builder::new()
                .name("record-video".into())
                .spawn(move || write_video(&path, area, &stop, &ready))
                .map_err(|e| video_error(format!("could not start the video thread: {e}")))?
        };
        let first = match started.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(first)) => first,
            Ok(Err(error)) => return Err(error),
            Err(_) => {
                stop.store(true, Ordering::SeqCst);
                return Err(video_error("the video encoder did not start".into()));
            }
        };
        let offset_ms = first.saturating_duration_since(session_start).as_millis() as u64;
        Ok(Self {
            stop,
            thread: Some(thread),
            path: path.to_path_buf(),
            area,
            offset_ms,
        })
    }

    /// Stops and finishes the file.
    pub(super) fn finish(mut self) -> Result<RecordedVideo, AdapterError> {
        self.stop.store(true, Ordering::SeqCst);
        let thread = self
            .thread
            .take()
            .ok_or_else(|| video_error("the video already finished".into()))?;
        let frames = thread
            .join()
            .map_err(|_| video_error("the video thread panicked".into()))??;
        Ok(RecordedVideo {
            path: self.path.to_string_lossy().into_owned(),
            offset_ms: self.offset_ms,
            width: self.area.width,
            height: self.area.height,
            frames,
        })
    }
}

impl Drop for VideoRecorder {
    /// A session that ends in an error still stops its video thread.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn write_video(
    path: &Path,
    area: Area,
    stop: &AtomicBool,
    ready: &Sender<Result<Instant, AdapterError>>,
) -> Result<u64, AdapterError> {
    let encoder = match Encoder::open(path, area.width, area.height) {
        Ok(encoder) => encoder,
        Err(error) => {
            let _ = ready.send(Err(error.clone()));
            return Err(error);
        }
    };
    let mut grabber = Grabber::new(area);
    let first = Instant::now();
    let _ = ready.send(Ok(first));
    let tick = Duration::from_millis(1000 / u64::from(FRAMES_PER_SECOND));
    let mut frames = 0;
    let mut previous: Option<i64> = None;
    while !stop.load(Ordering::SeqCst) {
        let due = first + tick * frames as u32;
        let wait = due.saturating_duration_since(Instant::now());
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
        let at =
            Instant::now().saturating_duration_since(first).as_millis() as i64 * HUNDRED_NS_PER_MS;
        let pixels = grabber.grab()?;
        if let Some(before) = previous {
            encoder.write(pixels, before, at - before)?;
        }
        previous = Some(at);
        frames += 1;
    }
    if let Some(before) = previous {
        let pixels = grabber.grab()?;
        let last = tick.as_millis() as i64 * HUNDRED_NS_PER_MS;
        encoder.write(pixels, before, last)?;
    }
    encoder.close()?;
    Ok(frames)
}

/// An MP4 file being encoded.
struct Encoder {
    writer: windows::Win32::Media::MediaFoundation::IMFSinkWriter,
    stream: u32,
    frame_bytes: u32,
}

impl Encoder {
    fn open(path: &Path, width: u32, height: u32) -> Result<Self, AdapterError> {
        use windows::Win32::Media::MediaFoundation::*;
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
        let url = windows::core::HSTRING::from(path.as_os_str());
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(mf("start Media Foundation"))?;
            let mut attributes = None;
            MFCreateAttributes(&mut attributes, 2).map_err(mf("create attributes"))?;
            let attributes = attributes.ok_or_else(|| video_error("no attributes".into()))?;
            attributes
                .SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)
                .map_err(mf("ask for hardware encoding"))?;
            attributes
                .SetGUID(
                    &MF_TRANSCODE_CONTAINERTYPE,
                    &MFTranscodeContainerType_FMPEG4,
                )
                .map_err(mf("ask for a fragmented MP4"))?;
            let writer = MFCreateSinkWriterFromURL(&url, None, &attributes)
                .map_err(mf("create the video file"))?;
            let frame_size = (u64::from(width) << 32) | u64::from(height);
            let frame_rate = (u64::from(FRAMES_PER_SECOND) << 32) | 1;
            let output = MFCreateMediaType().map_err(mf("describe the video"))?;
            output
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(mf("set the type"))?;
            output
                .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
                .map_err(mf("set H.264"))?;
            let bitrate = width * height * BITS_PER_PIXEL_SECOND;
            output
                .SetUINT32(&MF_MT_AVG_BITRATE, bitrate)
                .map_err(mf("set the bitrate"))?;
            output
                .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                .map_err(mf("set progressive frames"))?;
            output
                .SetUINT64(&MF_MT_FRAME_SIZE, frame_size)
                .map_err(mf("set the size"))?;
            output
                .SetUINT64(&MF_MT_FRAME_RATE, frame_rate)
                .map_err(mf("set the frame rate"))?;
            output
                .SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1 << 32) | 1)
                .map_err(mf("set the pixel shape"))?;
            let stream = writer
                .AddStream(&output)
                .map_err(mf("add the video stream"))?;
            let input = MFCreateMediaType().map_err(mf("describe the frames"))?;
            input
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(mf("set the type"))?;
            input
                .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
                .map_err(mf("set RGB32"))?;
            input
                .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                .map_err(mf("set progressive frames"))?;
            input
                .SetUINT64(&MF_MT_FRAME_SIZE, frame_size)
                .map_err(mf("set the size"))?;
            input
                .SetUINT64(&MF_MT_FRAME_RATE, frame_rate)
                .map_err(mf("set the frame rate"))?;
            input
                .SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1 << 32) | 1)
                .map_err(mf("set the pixel shape"))?;
            input
                .SetUINT32(&MF_MT_DEFAULT_STRIDE, width * 4)
                .map_err(mf("set top-down frames"))?;
            writer
                .SetInputMediaType(stream, &input, None)
                .map_err(mf("accept screen frames"))?;
            writer.BeginWriting().map_err(mf("begin writing"))?;
            Ok(Self {
                writer,
                stream,
                frame_bytes: width * height * 4,
            })
        }
    }

    fn write(&self, pixels: &[u8], at: i64, duration: i64) -> Result<(), AdapterError> {
        use windows::Win32::Media::MediaFoundation::*;
        unsafe {
            let buffer = MFCreateMemoryBuffer(self.frame_bytes).map_err(mf("allocate a frame"))?;
            let mut data = std::ptr::null_mut();
            buffer
                .Lock(&mut data, None, None)
                .map_err(mf("lock a frame"))?;
            let length = pixels.len().min(self.frame_bytes as usize);
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), data, length);
            buffer.Unlock().map_err(mf("unlock a frame"))?;
            buffer
                .SetCurrentLength(length as u32)
                .map_err(mf("size a frame"))?;
            let sample = MFCreateSample().map_err(mf("create a sample"))?;
            sample.AddBuffer(&buffer).map_err(mf("fill a sample"))?;
            sample.SetSampleTime(at).map_err(mf("time a sample"))?;
            sample
                .SetSampleDuration(duration.max(1))
                .map_err(mf("time a sample"))?;
            self.writer
                .WriteSample(self.stream, &sample)
                .map_err(mf("write a frame"))
        }
    }

    fn close(self) -> Result<(), AdapterError> {
        use windows::Win32::Media::MediaFoundation::MFShutdown;
        unsafe {
            let finished = self.writer.Finalize().map_err(mf("finish the video file"));
            drop(self.writer);
            let _ = MFShutdown();
            finished
        }
    }
}

fn mf(what: &'static str) -> impl Fn(windows::core::Error) -> AdapterError {
    move |error| video_error(format!("could not {what}: {error}"))
}

fn video_error(message: String) -> AdapterError {
    AdapterError::new(ErrorCode::ActionFailed, format!("Video: {message}"))
}
