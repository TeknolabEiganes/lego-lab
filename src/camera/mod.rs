//! The computer's camera as a sensor: body poses (like LEGO's Coding Canvas
//! "Pose Classification").
//!
//! A background thread captures webcam pictures, runs the MoveNet pose
//! model on each one and publishes what it sees. Programs read the latest
//! [`Detection`] through [`PoseSource`]; the UI shows the picture and lets
//! children record examples of their own poses.

pub mod body;
pub mod capture;
pub mod movenet;
pub mod pose;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use capture::{Frame, Webcam};
use movenet::MoveNet;
pub use body::{Axis, BodyPoint};
pub use pose::{CameraPose, Detection, Pose, PoseClass};

/// Readings older than this are not trusted (the camera has stalled).
const STALE_AFTER: Duration = Duration::from_secs(1);
/// How much each new frame moves the points (the rest is the previous frame).
const SMOOTHING: f32 = 0.6;

#[derive(Debug, Clone, PartialEq)]
pub enum CameraStatus {
    Off,
    Starting,
    On { name: String },
    Failed(String),
}

/// What a program sees when it asks the camera.
#[derive(Debug, Clone, PartialEq)]
pub enum CameraReading {
    Off,
    /// Switching on; nothing seen yet.
    Starting,
    Failed(String),
    Live(Detection),
}

/// Where programs get camera readings from (the real camera, or a fake in
/// tests).
pub trait PoseSource: Send + Sync {
    fn reading(&self) -> CameraReading;
}

/// The latest picture and what was seen in it.
#[derive(Default)]
pub struct CameraView {
    pub frame: Option<Arc<Frame>>,
    /// Increases with every new picture.
    pub frame_number: u64,
    pub pose: Option<Pose>,
    pub detection: Detection,
    seen_at: Option<Instant>,
}

struct Shared {
    status: CameraStatus,
    view: CameraView,
    classes: Arc<Vec<PoseClass>>,
    /// Stop flag of the running capture thread.
    running: Option<Arc<AtomicBool>>,
}

#[derive(Clone)]
pub struct Camera {
    shared: Arc<Mutex<Shared>>,
}

impl Default for Camera {
    fn default() -> Self {
        Self::new()
    }
}

impl Camera {
    pub fn new() -> Self {
        Camera {
            shared: Arc::new(Mutex::new(Shared {
                status: CameraStatus::Off,
                view: CameraView::default(),
                classes: Arc::new(Vec::new()),
                running: None,
            })),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> CameraStatus {
        self.lock().status.clone()
    }

    pub fn is_on(&self) -> bool {
        self.lock().running.is_some()
    }

    /// The poses the project has trained, used for every following frame.
    pub fn set_classes(&self, classes: &[PoseClass]) {
        self.lock().classes = Arc::new(classes.to_vec());
    }

    /// Look at the latest picture and pose.
    pub fn with_view<R>(&self, f: impl FnOnce(&CameraView) -> R) -> R {
        f(&self.lock().view)
    }

    /// The current pose, if a person is in view and the picture is fresh.
    pub fn current_pose(&self) -> Option<Pose> {
        let s = self.lock();
        let fresh = s.view.seen_at.is_some_and(|t| t.elapsed() < STALE_AFTER);
        s.view.pose.clone().filter(|p| fresh && p.has_person())
    }

    /// Switch the camera on (if it isn't). `repaint` is asked to redraw the
    /// UI after every picture.
    pub fn start(&self, repaint: egui::Context) {
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut s = self.lock();
            if s.running.is_some() {
                return;
            }
            s.running = Some(stop.clone());
            s.status = CameraStatus::Starting;
            s.view = CameraView::default();
        }
        let camera = self.clone();
        let spawned = std::thread::Builder::new().name("lego-lab-camera".into()).spawn(move || {
            let result = camera.capture_loop(&stop, &repaint);
            let mut s = camera.lock();
            if s.running.as_ref().is_some_and(|r| Arc::ptr_eq(r, &stop)) {
                s.running = None;
                s.status = match result {
                    Ok(()) => CameraStatus::Off,
                    Err(e) => CameraStatus::Failed(e),
                };
                s.view = CameraView::default();
            }
            drop(s);
            repaint.request_repaint();
        });
        if let Err(e) = spawned {
            let mut s = self.lock();
            s.running = None;
            s.status = CameraStatus::Failed(format!("could not start the camera ({e})"));
        }
    }

    pub fn stop(&self) {
        let mut s = self.lock();
        if let Some(stop) = s.running.take() {
            stop.store(true, Ordering::Relaxed);
        }
        s.status = CameraStatus::Off;
        s.view = CameraView::default();
    }

    fn capture_loop(&self, stop: &AtomicBool, repaint: &egui::Context) -> Result<(), String> {
        let model = MoveNet::shared()?;
        let mut webcam = Webcam::open();
        // Just after switching off and on, the previous capture may still
        // hold the camera for a moment.
        for _ in 0..5 {
            if webcam.is_ok() || stop.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(300));
            webcam = Webcam::open();
        }
        let mut webcam = webcam?;
        let mut smoothed: Option<Pose> = None;
        while !stop.load(Ordering::Relaxed) {
            let frame = webcam.next_frame()?;
            let raw = model.detect(&frame).map_err(|e| format!("the pose model failed ({e})"))?;
            let pose = match smoothed.take() {
                Some(previous) if raw.has_person() && previous.has_person() => previous.smoothed(&raw, SMOOTHING),
                _ => raw,
            };
            smoothed = Some(pose.clone());
            let mut s = self.lock();
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let detection = Detection::from_pose(Some(&pose), &s.classes);
            s.status = CameraStatus::On { name: webcam.name.clone() };
            s.view.frame = Some(Arc::new(frame));
            s.view.frame_number += 1;
            s.view.pose = Some(pose);
            s.view.detection = detection;
            s.view.seen_at = Some(Instant::now());
            drop(s);
            repaint.request_repaint();
        }
        Ok(())
    }
}

impl PoseSource for Camera {
    fn reading(&self) -> CameraReading {
        let s = self.lock();
        match &s.status {
            CameraStatus::Off => CameraReading::Off,
            CameraStatus::Failed(e) => CameraReading::Failed(e.clone()),
            CameraStatus::Starting => CameraReading::Starting,
            CameraStatus::On { .. } => match s.view.seen_at {
                Some(t) if t.elapsed() < STALE_AFTER => CameraReading::Live(s.view.detection.clone()),
                _ => CameraReading::Starting,
            },
        }
    }
}
