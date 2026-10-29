//! Application state and the glue between UI, executor and hardware.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::runtime::Runtime;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio_util::sync::CancellationToken;

use crate::audio::{ComputerSpeaker, Speaker};
use crate::camera::pose::MAX_EXAMPLES;
use crate::camera::{Camera, PoseClass};
use crate::executor::{ExecEvent, Executor, RunOutcome};
use crate::hardware::lego::LegoBleDevice;
use crate::hardware::mock::MockDevice;
use crate::hardware::{AttachedDevice, CardGroup, ConnectionState, HwResult, LegoDevice};
use crate::program::compiler;
use crate::project::{self, Project};
use crate::ui::camera_panel::{self, CameraAction, Preview};
use crate::ui::canvas::{self, CanvasInfo, Editor};
use crate::ui::device_panel::{self, HardwareView, PanelAction, StatusLine};
use crate::ui::toolbar::{self, Backend, ToolbarAction, ToolbarState};
use crate::ui::{icons::IconTextures, palette, theme};
use crate::visual::{BlockId, Workspace};

const HIGHLIGHT_LINGER: Duration = Duration::from_millis(180);
/// While Record is held, add a camera example this often.
const RECORD_INTERVAL: Duration = Duration::from_millis(120);
const LAST_PROJECT_FILE: &str = "last-project";

fn app_config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|base| base.join("lego-lab"))
}

fn read_last_project_path() -> Option<PathBuf> {
    let config = app_config_dir()?;
    let path = std::fs::read_to_string(config.join(LAST_PROJECT_FILE)).ok()?;
    let path = PathBuf::from(path.trim());
    (!path.as_os_str().is_empty()).then_some(path)
}

fn remember_last_project(path: &Path) {
    let Some(config) = app_config_dir() else { return };
    if std::fs::create_dir_all(&config).is_err() {
        return;
    }
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let _ = std::fs::write(config.join(LAST_PROJECT_FILE), path.to_string_lossy().as_bytes());
}

/// Results of background hardware work, delivered to the UI thread.
enum HwMsg {
    Connected(HwResult<()>),
    Discovered(HwResult<Vec<AttachedDevice>>),
    Scanned(HwResult<usize>),
}

struct RunHandle {
    cancel: CancellationToken,
    events: UnboundedReceiver<ExecEvent>,
}

/// What the right-hand pane shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RightPane {
    /// The simulator or the Bluetooth devices, depending on the backend.
    Devices,
    Camera,
}

pub struct LegoLabApp {
    rt: Runtime,
    mock: MockDevice,
    ble: LegoBleDevice,
    device: Arc<dyn LegoDevice>,
    backend: Backend,
    icons: IconTextures,

    editor: Editor,
    project_name: String,
    path: Option<PathBuf>,
    dirty: bool,

    run: Option<RunHandle>,
    active: HashSet<BlockId>,
    /// Blocks that finished recently stay lit briefly so fast blocks are visible.
    recent: HashMap<BlockId, Instant>,
    error_block: Option<BlockId>,
    status: Option<StatusLine>,

    devices: Vec<AttachedDevice>,
    last_discover: Instant,
    hw_tx: UnboundedSender<HwMsg>,
    hw_rx: UnboundedReceiver<HwMsg>,
    show_blocks: bool,
    show_right_panel: bool,
    right_pane: RightPane,
    device_filter: Option<CardGroup>,

    camera: Camera,
    camera_preview: Preview,
    /// Poses taught to the camera; saved with the project.
    poses: Vec<PoseClass>,
    /// The camera was switched on by RUN (not by the user) and goes off
    /// again when the program ends.
    camera_for_run: bool,
    last_record: Instant,
    /// Sound blocks set to play on this computer.
    speaker: ComputerSpeaker,
}

impl LegoLabApp {
    pub fn new(cc: &eframe::CreationContext<'_>, open_path: Option<PathBuf>, bluetooth: bool) -> Self {
        theme::apply(&cc.egui_ctx);
        let icons = IconTextures::load(&cc.egui_ctx);
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("lego-lab-hw")
            .build()
            .expect("failed to start the Tokio runtime");
        let mock = MockDevice::new();
        let (hw_tx, hw_rx) = unbounded_channel();
        let mut app = Self {
            rt,
            device: Arc::new(mock.clone()),
            mock,
            ble: LegoBleDevice::new(),
            backend: Backend::Simulator,
            icons,
            editor: Editor::new(Workspace::new()),
            project_name: "My Robot".into(),
            path: None,
            dirty: false,
            run: None,
            active: HashSet::new(),
            recent: HashMap::new(),
            error_block: None,
            status: None,
            devices: Vec::new(),
            last_discover: Instant::now(),
            hw_tx,
            hw_rx,
            show_blocks: true,
            show_right_panel: true,
            right_pane: RightPane::Devices,
            device_filter: None,
            camera: Camera::new(),
            camera_preview: Preview::default(),
            poses: Vec::new(),
            camera_for_run: false,
            last_record: Instant::now(),
            speaker: ComputerSpeaker::new(),
        };
        let explicit_path = open_path.is_some();
        if let Some(path) = open_path.or_else(read_last_project_path) {
            if explicit_path || path.exists() {
                app.open_path(path);
            } else {
                app.info("Last project file was not found; started with an empty canvas.");
            }
        }
        if bluetooth {
            app.select_backend(Backend::Bluetooth, &cc.egui_ctx);
        } else {
            // The simulator is always available, so connect straight away.
            app.connect(&cc.egui_ctx);
        }
        app
    }

    fn info(&mut self, text: impl Into<String>) {
        self.status = Some(StatusLine { text: text.into(), color: theme::TEXT_MUTED });
    }

    fn error(&mut self, text: impl Into<String>) {
        self.status = Some(StatusLine { text: text.into(), color: theme::ERROR });
    }

    fn running(&self) -> bool {
        self.run.is_some()
    }

    // ---- hardware -------------------------------------------------------

    fn connect(&mut self, ctx: &egui::Context) {
        if self.backend == Backend::Bluetooth {
            self.info("Searching for LEGO devices…");
        }
        let device = self.device.clone();
        let tx = self.hw_tx.clone();
        ctx.request_repaint_after(Duration::from_millis(50));
        let ctx = ctx.clone();
        self.rt.spawn(async move {
            let result = device.connect().await;
            let ok = result.is_ok();
            let _ = tx.send(HwMsg::Connected(result));
            if ok {
                let _ = tx.send(HwMsg::Discovered(device.discover().await));
            }
            ctx.request_repaint();
        });
    }

    fn disconnect(&mut self) {
        self.stop();
        let device = self.device.clone();
        self.rt.spawn(async move {
            let _ = device.stop_all().await;
            let _ = device.disconnect().await;
        });
        self.devices.clear();
    }

    fn select_backend(&mut self, backend: Backend, ctx: &egui::Context) {
        self.disconnect();
        self.backend = backend;
        self.device = match backend {
            Backend::Simulator => Arc::new(self.mock.clone()),
            Backend::Bluetooth => Arc::new(self.ble.clone()),
        };
        self.show_right_panel = true;
        self.right_pane = RightPane::Devices;
        match backend {
            // The simulator is always there: connect straight away.
            Backend::Simulator => self.connect(ctx),
            // Real hardware: show what's nearby and let the user choose.
            Backend::Bluetooth => self.scan(ctx),
        }
    }

    // ---- real hardware: scan / pick devices ------------------------------

    fn spawn_hw<F>(&self, ctx: &egui::Context, job: F)
    where
        F: std::future::Future<Output = Vec<HwMsg>> + Send + 'static,
    {
        ctx.request_repaint_after(Duration::from_millis(50));
        let tx = self.hw_tx.clone();
        let ctx = ctx.clone();
        self.rt.spawn(async move {
            for msg in job.await {
                let _ = tx.send(msg);
            }
            ctx.request_repaint();
        });
    }

    fn scan(&mut self, ctx: &egui::Context) {
        self.info("Searching for LEGO devices…");
        let ble = self.ble.clone();
        self.spawn_hw(ctx, async move { vec![HwMsg::Scanned(ble.scan_devices().await.map(|d| d.len()))] });
    }

    fn panel_action(&mut self, action: PanelAction, ctx: &egui::Context) {
        let ble = self.ble.clone();
        match action {
            PanelAction::Scan => self.scan(ctx),
            PanelAction::SetDeviceFilter(filter) => self.device_filter = filter,
            PanelAction::ConnectMatching(filter) => {
                self.info(if filter.is_some() { "Connecting to matching devices…" } else { "Connecting to all found devices…" });
                self.spawn_hw(ctx, async move {
                    let connected = ble.connect_matching_found(filter).await;
                    vec![HwMsg::Connected(connected), HwMsg::Discovered(ble.discover().await)]
                });
            }
            PanelAction::Connect(address) => {
                self.info("Connecting…");
                self.spawn_hw(ctx, async move {
                    let result = ble.connect_device(&address).await;
                    vec![HwMsg::Connected(result), HwMsg::Discovered(ble.discover().await)]
                });
            }
            PanelAction::Disconnect(address) => {
                self.spawn_hw(ctx, async move {
                    let _ = ble.disconnect_device(&address).await;
                    vec![HwMsg::Discovered(ble.discover().await)]
                });
            }
        }
    }

    /// Re-run discovery once a second so the status bar tracks devices
    /// being switched on/off.
    fn poll_discovery(&mut self, ctx: &egui::Context) {
        if self.device.connection() != ConnectionState::Connected {
            self.devices.clear();
            return;
        }
        if self.last_discover.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_discover = Instant::now();
        let device = self.device.clone();
        let tx = self.hw_tx.clone();
        let ctx = ctx.clone();
        self.rt.spawn(async move {
            let _ = tx.send(HwMsg::Discovered(device.discover().await));
            ctx.request_repaint();
        });
    }

    fn handle_hw_messages(&mut self) {
        while let Ok(msg) = self.hw_rx.try_recv() {
            match msg {
                HwMsg::Connected(Ok(())) => {
                    if self.backend == Backend::Simulator {
                        self.info("Connected to the simulator");
                    } else {
                        // Re-announce device names after the next discovery.
                        self.devices.clear();
                    }
                }
                HwMsg::Connected(Err(e)) => self.error(format!("Could not connect: {e}")),
                HwMsg::Discovered(Ok(devices)) => {
                    if self.backend == Backend::Bluetooth && self.devices.is_empty() && !devices.is_empty() {
                        let names: Vec<String> =
                            devices.iter().map(|d| format!("{} ({})", d.kind.label(), d.name)).collect();
                        self.info(format!("Connected: {}", names.join(", ")));
                    }
                    self.devices = devices;
                }
                HwMsg::Discovered(Err(_)) => self.devices.clear(),
                HwMsg::Scanned(Ok(0)) => self.error(
                    "No LEGO devices found. Switch them on and check their light is blinking, then Scan again.",
                ),
                HwMsg::Scanned(Ok(n)) => self.info(format!(
                    "Found {n} device{}. Connect individually or choose Connect all.",
                    if n == 1 { "" } else { "s" }
                )),
                HwMsg::Scanned(Err(e)) => self.error(format!("Could not scan: {e}")),
            }
        }
    }

    // ---- run / stop -----------------------------------------------------

    fn run_program(&mut self, ctx: &egui::Context) {
        if self.running() {
            return;
        }
        self.error_block = None;
        let compiled = match compiler::compile(&self.editor.ws) {
            Ok(c) => c,
            Err(errors) => {
                let first = &errors[0];
                self.error_block = first.block;
                let msg = first.message.clone();
                self.error(msg);
                return;
            }
        };
        let needs_lego = !compiled.devices.is_empty() || !compiled.groups.is_empty();
        if needs_lego && self.device.connection() != ConnectionState::Connected {
            self.error("Not connected. Press Connect first.");
            return;
        }
        if let Some(missing) = compiled
            .devices
            .iter()
            .find(|target| {
                !self
                    .devices
                    .iter()
                    .any(|d| d.kind == target.kind && target.group.matches_device_group(d.group))
            })
        {
            self.error(format!(
                "Your program uses the {} ({}) group, but it isn't connected.",
                missing.kind.label(),
                missing.group.label()
            ));
            return;
        }
        if let Some(group) = compiled
            .groups
            .iter()
            .find(|group| !self.devices.iter().any(|d| group.matches_device_group(d.group)))
        {
            self.error(format!("Your program beeps the {} group, but no device in that group is connected.", group.label()));
            return;
        }

        let trained = self.poses.iter().filter(|p| !p.examples.is_empty()).count();
        for (&pose, &block) in &compiled.custom_poses {
            let problem = match self.poses.get(pose as usize) {
                None => Some("This block looks for a camera pose that doesn't exist. Pick another one.".to_string()),
                Some(p) if p.examples.is_empty() => {
                    Some(format!("“{}” has no examples yet. Record some in the Camera pane.", p.name))
                }
                Some(_) if trained < 2 => {
                    Some("Teach the camera at least two poses, so it can tell them apart.".to_string())
                }
                Some(_) => None,
            };
            if let Some(message) = problem {
                self.error_block = Some(block);
                self.error(message);
                return;
            }
        }
        if compiled.uses_camera && !self.camera.is_on() {
            self.camera.start(ctx.clone());
            self.camera_for_run = true;
        }

        let cancel = CancellationToken::new();
        let (tx, rx) = unbounded_channel();
        let executor = Executor::new(self.device.clone())
            .with_camera(Arc::new(self.camera.clone()))
            .with_speaker(Arc::new(self.speaker.clone()));
        let program = compiled.program;
        let token = cancel.clone();
        let ctx2 = ctx.clone();
        self.rt.spawn(async move {
            // Forward events with a repaint so highlighting is immediate.
            let (inner_tx, mut inner_rx) = unbounded_channel();
            let forward = tokio::spawn(async move {
                while let Some(e) = inner_rx.recv().await {
                    if tx.send(e).is_err() {
                        break;
                    }
                    ctx2.request_repaint();
                }
            });
            executor.run(program, token, inner_tx).await;
            let _ = forward.await;
        });
        self.run = Some(RunHandle { cancel, events: rx });
        self.active.clear();
        self.info("Running…");
    }

    /// STOP: cancel, stop every motor, clear highlights. Never blocks.
    fn stop(&mut self) {
        if let Some(run) = self.run.take() {
            run.cancel.cancel();
            self.info("Stopped");
        }
        self.active.clear();
        self.recent.clear();
        self.release_camera_after_run();
        self.speaker.stop();
        let device = self.device.clone();
        self.rt.spawn(async move {
            let _ = device.stop_all().await;
        });
    }

    /// Switch off a camera that RUN switched on, unless the Camera pane is
    /// showing it.
    fn release_camera_after_run(&mut self) {
        if std::mem::take(&mut self.camera_for_run) && !self.camera_pane_visible() {
            self.camera.stop();
        }
    }

    fn camera_pane_visible(&self) -> bool {
        self.show_right_panel && self.right_pane == RightPane::Camera
    }

    // ---- camera ---------------------------------------------------------

    fn set_poses(&mut self, poses: Vec<PoseClass>) {
        self.poses = poses;
        self.camera.set_classes(&self.poses);
    }

    fn poses_changed(&mut self) {
        self.camera.set_classes(&self.poses);
        self.dirty = true;
    }

    fn camera_action(&mut self, action: CameraAction, ctx: &egui::Context) {
        match action {
            CameraAction::TurnOn => {
                self.camera.start(ctx.clone());
                self.camera_for_run = false;
            }
            CameraAction::TurnOff => {
                self.camera.stop();
                self.camera_for_run = false;
            }
            CameraAction::AddPose => {
                let n = (1..).find(|n| !self.poses.iter().any(|p| p.name == format!("pose {n}"))).unwrap_or(1);
                self.poses.push(PoseClass::new(format!("pose {n}")));
                self.poses_changed();
            }
            CameraAction::Renamed => self.poses_changed(),
            CameraAction::ClearPose(i) => {
                if let Some(p) = self.poses.get_mut(i) {
                    p.examples.clear();
                    self.poses_changed();
                }
            }
            CameraAction::DeletePose(i) => {
                if i < self.poses.len() {
                    let removed = self.poses.remove(i);
                    if self.editor.ws.remove_camera_pose(i as u8) {
                        self.info(format!("Deleted “{}”. Blocks that used it now look for a person.", removed.name));
                    }
                    self.poses_changed();
                }
            }
            CameraAction::Record(i) => {
                if self.last_record.elapsed() < RECORD_INTERVAL {
                    return;
                }
                let Some(class) = self.poses.get_mut(i) else { return };
                match self.camera.current_pose() {
                    Some(pose) if class.examples.len() < MAX_EXAMPLES => {
                        class.examples.push(pose);
                        self.last_record = Instant::now();
                        self.poses_changed();
                    }
                    Some(_) => {}
                    None => self.info("Step in front of the camera so it can see you."),
                }
            }
        }
    }

    fn handle_exec_events(&mut self) {
        let Some(run) = &mut self.run else { return };
        let mut finished = None;
        while let Ok(event) = run.events.try_recv() {
            match event {
                ExecEvent::BlockStarted(id) => {
                    self.active.insert(id);
                }
                ExecEvent::BlockFinished(id) => {
                    self.active.remove(&id);
                    self.recent.insert(id, Instant::now());
                }
                ExecEvent::Finished(outcome) => finished = Some(outcome),
            }
        }
        if let Some(outcome) = finished {
            self.run = None;
            self.active.clear();
            self.recent.clear();
            self.release_camera_after_run();
            match outcome {
                RunOutcome::Completed => self.info("Program finished"),
                RunOutcome::Stopped => self.info("Stopped"),
                RunOutcome::Failed(e) => self.error(format!("Program stopped: {e}")),
            }
        }
    }

    // ---- files ----------------------------------------------------------

    fn new_project(&mut self) {
        self.stop();
        self.editor = Editor::new(Workspace::new());
        self.set_poses(Vec::new());
        self.project_name = "My Robot".into();
        self.path = None;
        self.dirty = false;
        self.error_block = None;
        self.info("New project");
    }

    fn file_dialog(&self) -> rfd::FileDialog {
        let mut dialog = rfd::FileDialog::new().add_filter("LEGO Lab project", &[project::EXTENSION]);
        if let Some(dir) = self.path.as_ref().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(dir);
        }
        dialog
    }

    fn open(&mut self) {
        if let Some(path) = self.file_dialog().pick_file() {
            self.open_path(path);
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        match Project::load(&path) {
            Ok(p) => {
                self.stop();
                self.editor = Editor::new(p.workspace);
                self.set_poses(p.poses);
                self.project_name = p.name;
                self.info(format!("Opened {}", path.display()));
                remember_last_project(&path);
                self.path = Some(path);
                self.dirty = false;
                self.error_block = None;
            }
            Err(e) => self.error(format!("Could not open: {e:#}")),
        }
    }

    fn save(&mut self, save_as: bool) {
        let path = match (&self.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let name = format!("{}.{}", self.project_name.trim(), project::EXTENSION);
                match self.file_dialog().set_file_name(name).save_file() {
                    Some(mut p) => {
                        if p.extension().is_none() {
                            p.set_extension(project::EXTENSION);
                        }
                        p
                    }
                    None => return,
                }
            }
        };
        let project = Project { poses: self.poses.clone(), ..Project::new(self.project_name.clone(), self.editor.ws.clone()) };
        match project.save(&path) {
            Ok(()) => {
                self.info(format!("Saved {}", path.display()));
                remember_last_project(&path);
                self.path = Some(path);
                self.dirty = false;
            }
            Err(e) => self.error(format!("Could not save: {e:#}")),
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) -> Vec<ToolbarAction> {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let mut out = Vec::new();
        let save_as = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
        let save = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        let open = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
        let new = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
        let toggle_blocks = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num1);
        let toggle_right_panel = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num2);
        let toggle_panes = KeyboardShortcut::new(Modifiers::COMMAND, Key::F);
        let show_camera = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num3);
        let run = KeyboardShortcut::new(Modifiers::NONE, Key::F5);
        ctx.input_mut(|i| {
            if i.consume_shortcut(&save_as) {
                out.push(ToolbarAction::SaveAs);
            }
            if i.consume_shortcut(&save) {
                out.push(ToolbarAction::Save);
            }
            if i.consume_shortcut(&open) {
                out.push(ToolbarAction::Open);
            }
            if i.consume_shortcut(&new) {
                out.push(ToolbarAction::New);
            }
            if i.consume_shortcut(&toggle_blocks) {
                out.push(ToolbarAction::ToggleBlocks);
            }
            if i.consume_shortcut(&toggle_right_panel) {
                out.push(ToolbarAction::TogglePanel);
            }
            if i.consume_shortcut(&toggle_panes) {
                out.push(ToolbarAction::TogglePanes);
            }
            if i.consume_shortcut(&show_camera) {
                out.push(ToolbarAction::ShowCamera);
            }
            if i.consume_shortcut(&run) {
                out.push(ToolbarAction::Run);
            }
            if i.key_pressed(Key::Escape) && self.run.is_some() {
                out.push(ToolbarAction::Stop);
            }
        });
        out
    }

    fn apply(&mut self, action: ToolbarAction, ctx: &egui::Context) {
        match action {
            ToolbarAction::New => self.new_project(),
            ToolbarAction::Open => self.open(),
            ToolbarAction::Save => self.save(false),
            ToolbarAction::SaveAs => self.save(true),
            ToolbarAction::Connect => self.connect(ctx),
            ToolbarAction::Disconnect => self.disconnect(),
            ToolbarAction::SelectBackend(b) => self.select_backend(b, ctx),
            ToolbarAction::Run => self.run_program(ctx),
            ToolbarAction::Stop => self.stop(),
            ToolbarAction::ToggleBlocks => self.show_blocks = !self.show_blocks,
            ToolbarAction::TogglePanel => self.show_right_panel = !self.show_right_panel,
            ToolbarAction::TogglePanes => {
                // Treat Ctrl/Cmd+F as one combined toggle: if either pane is
                // open, hide both; if both are hidden, show both.
                let show = !(self.show_blocks || self.show_right_panel);
                self.show_blocks = show;
                self.show_right_panel = show;
            }
            ToolbarAction::ShowCamera => {
                if self.camera_pane_visible() {
                    self.show_right_panel = false;
                } else {
                    self.show_right_panel = true;
                    self.right_pane = RightPane::Camera;
                }
            }
        }
    }
}

impl eframe::App for LegoLabApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_hw_messages();
        self.handle_exec_events();
        self.poll_discovery(&ctx);

        let mut actions = self.shortcuts(&ctx);
        let connection = self.device.connection();
        let running = self.running();

        let bar = |fill| egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(8, 6));

        egui::Panel::top("toolbar")
            .exact_size(56.0)
            .frame(bar(theme::PANEL))
            .show(ui, |ui| {
                let name_before = self.project_name.clone();
                actions.extend(toolbar::show(
                    ui,
                    ToolbarState {
                        project_name: &mut self.project_name,
                        dirty: self.dirty,
                        connection,
                        backend: self.backend,
                        running,
                    },
                ));
                if self.project_name != name_before {
                    self.dirty = true;
                }
            });

        egui::Panel::bottom("status")
            .exact_size(34.0)
            .frame(bar(theme::PANEL))
            .show(ui, |ui| {
                actions.extend(device_panel::status_bar(
                    ui,
                    self.device.backend_name(),
                    connection,
                    &self.devices,
                    self.status.as_ref(),
                    self.backend,
                    self.show_blocks,
                    self.show_right_panel,
                    self.camera.is_on(),
                    &self.icons,
                ));
            });

        if self.show_blocks {
            let palette_resp = egui::Panel::left("palette")
                .exact_size(210.0)
                .resizable(false)
                .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(10))
                .show(ui, |ui| palette::show(ui, &mut self.editor, &self.icons));
            self.editor.palette_rect = palette_resp.response.rect;
        } else {
            self.editor.palette_rect = egui::Rect::NOTHING;
        }

        let mut panel_actions = Vec::new();
        let mut camera_actions = Vec::new();
        if self.show_right_panel {
            egui::Panel::right("simulator")
                .default_size(300.0)
                .size_range(260.0..=420.0)
                .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let devices = if self.backend == Backend::Simulator { "Simulator" } else { "Devices" };
                        ui.selectable_value(&mut self.right_pane, RightPane::Devices, devices)
                            .on_hover_text("Ctrl/Cmd+2");
                        ui.selectable_value(&mut self.right_pane, RightPane::Camera, "📷 Camera")
                            .on_hover_text("Ctrl/Cmd+3");
                    });
                    ui.separator();
                    match (self.right_pane, self.backend) {
                        (RightPane::Camera, _) => {
                            camera_actions = camera_panel::camera_panel(ui, &self.camera, &mut self.camera_preview, &mut self.poses);
                        }
                        (RightPane::Devices, Backend::Simulator) => device_panel::simulator_panel(ui, &self.mock),
                        (RightPane::Devices, Backend::Bluetooth) => {
                            let view = HardwareView {
                                connection,
                                busy: self.ble.is_busy(),
                                found: &self.ble.last_scan(),
                                connected: &self.ble.connected_addresses(),
                                live: &self.device.snapshot(),
                                filter: self.device_filter,
                            };
                            if let Some(action) = device_panel::hardware_panel(ui, view, &self.icons) {
                                panel_actions.push(action);
                            }
                        }
                    }
                });
        }

        egui::CentralPanel::no_frame().show(ui, |ui| {
            self.recent.retain(|_, t| t.elapsed() < HIGHLIGHT_LINGER);
            let mut lit = self.active.clone();
            lit.extend(self.recent.keys().copied());
            let pose_names: Vec<String> = self.poses.iter().map(|p| p.name.clone()).collect();
            let info = CanvasInfo { active: &lit, error_block: self.error_block, poses: &pose_names };
            if canvas::show(ui, &mut self.editor, &info) {
                self.dirty = true;
                self.error_block = None;
            }
        });

        for action in actions {
            self.apply(action, &ctx);
        }
        for action in panel_actions {
            self.panel_action(action, &ctx);
        }
        for action in camera_actions {
            self.camera_action(action, &ctx);
        }

        let title = format!(
            "{}{} — LEGO Lab",
            if self.dirty { "• " } else { "" },
            self.project_name
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        if running || connection == ConnectionState::Connecting {
            ctx.request_repaint_after(Duration::from_millis(33));
        } else {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    /// Safety: never leave motors running when the app closes.
    fn on_exit(&mut self) {
        if let Some(run) = self.run.take() {
            run.cancel.cancel();
        }
        self.camera.stop();
        self.speaker.stop();
        let device = self.device.clone();
        let _ = self.rt.block_on(async move {
            tokio::time::timeout(Duration::from_secs(2), async {
                let _ = device.stop_all().await;
                let _ = device.disconnect().await;
            })
            .await
        });
    }
}
