//! Asynchronous program interpreter.
//!
//! Runs on the Tokio runtime, never on the GUI thread. Reports progress via
//! [`ExecEvent`]s so the UI can highlight the running blocks.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::audio::{self, Speaker};
use crate::camera::{CameraReading, PoseSource};
use crate::hardware::{BeepPattern, HardwareError, LegoDevice, Tone};
use crate::program::{Command, Condition, Program, Script, Step};
use crate::visual::BlockId;

#[derive(Debug, Clone, PartialEq)]
pub enum ExecEvent {
    BlockStarted(BlockId),
    BlockFinished(BlockId),
    Finished(RunOutcome),
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunOutcome {
    Completed,
    Stopped,
    Failed(String),
}

enum ScriptError {
    Cancelled,
    Hardware(HardwareError),
    /// Camera or computer sound problem, with a message for the child.
    Failed(String),
}

pub struct Executor {
    device: Arc<dyn LegoDevice>,
    camera: Option<Arc<dyn PoseSource>>,
    speaker: Option<Arc<dyn Speaker>>,
}

impl Executor {
    pub fn new(device: Arc<dyn LegoDevice>) -> Self {
        Self { device, camera: None, speaker: None }
    }

    /// Where sound blocks set to "this computer" play.
    pub fn with_speaker(mut self, speaker: Arc<dyn Speaker>) -> Self {
        self.speaker = Some(speaker);
        self
    }

    /// Where camera conditions get their readings.
    pub fn with_camera(mut self, camera: Arc<dyn PoseSource>) -> Self {
        self.camera = Some(camera);
        self
    }

    /// Run every script concurrently until all finish, one fails, or
    /// `cancel` fires. On anything but normal completion all motors are
    /// stopped before returning. Motors started by a program that completes
    /// normally keep running (like "Run Motor" as the last block).
    pub async fn run(
        &self,
        program: Program,
        cancel: CancellationToken,
        events: UnboundedSender<ExecEvent>
    ) -> RunOutcome {
        // "Motor position" counts from 0 at RUN, not from power-on. Best
        // effort: a missing device is reported by the first command using it.
        let _ = self.device.reset_motor_positions().await;

        let mut tasks = JoinSet::new();
        for script in program.scripts {
            let device = self.device.clone();
            let sources = Sources { camera: self.camera.clone(), speaker: self.speaker.clone() };
            let cancel = cancel.clone();
            let events = events.clone();
            tasks.spawn(async move { run_script(&*device, &sources, script, &cancel, &events).await });
        }

        let mut outcome = RunOutcome::Completed;
        while let Some(result) = tasks.join_next().await {
            let failure = match result {
                Ok(Ok(())) => None,
                Ok(Err(ScriptError::Cancelled)) => {
                    if outcome == RunOutcome::Completed {
                        outcome = RunOutcome::Stopped;
                    }
                    None
                }
                Ok(Err(ScriptError::Hardware(e))) => Some(e.to_string()),
                Ok(Err(ScriptError::Failed(e))) => Some(e),
                Err(join_error) => Some(format!("the program crashed ({join_error})")),
            };
            if let Some(message) = failure {
                if !matches!(outcome, RunOutcome::Failed(_)) {
                    outcome = RunOutcome::Failed(message);
                }
                cancel.cancel();
            }
        }

        if outcome != RunOutcome::Completed {
            if let Some(speaker) = &self.speaker {
                speaker.stop();
            }
            let _ = self.device.stop_all().await;
        }
        let _ = events.send(ExecEvent::Finished(outcome.clone()));
        outcome
    }
}

/// Pause between loop iterations so a Forever loop of instant blocks
/// doesn't spin the CPU or flood the hardware (roughly Scratch's pace).
const LOOP_TICK: Duration = Duration::from_millis(20);
/// How often Wait Until re-checks its condition.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Inputs and outputs besides the LEGO devices.
struct Sources {
    camera: Option<Arc<dyn PoseSource>>,
    speaker: Option<Arc<dyn Speaker>>,
}

async fn run_script(
    device: &dyn LegoDevice,
    sources: &Sources,
    script: Script,
    cancel: &CancellationToken,
    events: &UnboundedSender<ExecEvent>
) -> Result<(), ScriptError> {
    let _ = events.send(ExecEvent::BlockStarted(script.hat));
    let _ = events.send(ExecEvent::BlockFinished(script.hat));
    let ctx = Ctx { device, camera: sources.camera.as_deref(), speaker: sources.speaker.as_deref(), cancel, events };
    ctx.steps(&script.steps).await
}

struct Ctx<'a> {
    device: &'a dyn LegoDevice,
    camera: Option<&'a dyn PoseSource>,
    speaker: Option<&'a dyn Speaker>,
    cancel: &'a CancellationToken,
    events: &'a UnboundedSender<ExecEvent>,
}

impl Ctx<'_> {
    fn steps<'s>(&'s self, steps: &'s [Step]) -> Pin<Box<dyn Future<Output = Result<(), ScriptError>> + Send + 's>> {
        Box::pin(async move {
            for step in steps {
                self.step(step).await?;
            }
            Ok(())
        })
    }

    async fn step(&self, step: &Step) -> Result<(), ScriptError> {
        if self.cancel.is_cancelled() {
            return Err(ScriptError::Cancelled);
        }
        let _ = self.events.send(ExecEvent::BlockStarted(step.source));
        let result = tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(ScriptError::Cancelled),
            r = self.command(&step.command) => r,
        };
        let _ = self.events.send(ExecEvent::BlockFinished(step.source));
        result
    }

    async fn tick(&self, duration: Duration) -> Result<(), ScriptError> {
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(ScriptError::Cancelled),
            _ = tokio::time::sleep(duration) => Ok(()),
        }
    }

    /// Start a sound on the computer.
    fn speak(&self, notes: &[(u16, u32)], volume: u32) -> Result<(), ScriptError> {
        let Some(speaker) = self.speaker else {
            return Err(ScriptError::Failed("this computer can't play sound".into()));
        };
        speaker.play(notes, volume).map_err(ScriptError::Failed)
    }

    async fn check(&self, condition: &Condition) -> Result<bool, ScriptError> {
        if condition.is_camera() {
            return match self.camera.map(|c| c.reading()) {
                Some(CameraReading::Live(seen)) => Ok(condition.evaluate_camera(&seen)),
                // Nothing seen yet: not true, so Wait Until keeps waiting.
                Some(CameraReading::Starting) => Ok(false),
                Some(CameraReading::Failed(e)) => Err(ScriptError::Failed(format!("camera: {e}"))),
                Some(CameraReading::Off) | None => Err(ScriptError::Failed("the camera is off".into())),
            };
        }
        let (Some(query), Some(group)) = (condition.query(), condition.group()) else { return Ok(false) };
        let values = self.device.read_sensors(query, group).await.map_err(ScriptError::Hardware)?;
        Ok(condition.evaluate_any(values))
    }

    async fn command(&self, command: &Command) -> Result<(), ScriptError> {
        let hw = |r: Result<(), HardwareError>| r.map_err(ScriptError::Hardware);
        let device = self.device;
        match command {
            &Command::MotorRun { motor, group, direction, speed } => hw(device.motor_run(motor, group, direction, speed).await),
            &Command::MotorRunForTime { motor, group, direction, speed, milliseconds } => hw(device
                .motor_run_for_time(motor, group, direction, speed, Duration::from_millis(milliseconds))
                .await),
            &Command::MotorRunForDegrees { motor, group, direction, speed, degrees } =>
                hw(device.motor_run_for_degrees(motor, group, direction, speed, degrees).await),
            &Command::MotorStop { motor, group } => hw(device.motor_stop(motor, group).await),
            &Command::MotorRunPower { motor, group, direction, power } => {
                hw(device.motor_run_power(motor, group, direction, power).await)
            }
            &Command::MotorRunPowerForTime { motor, group, direction, power, milliseconds } => hw(device
                .motor_run_power_for_time(motor, group, direction, power, Duration::from_millis(milliseconds))
                .await),
            &Command::MotorSetAcceleration { motor, group, acceleration, deceleration } => {
                hw(device.motor_set_acceleration(motor, group, acceleration, deceleration).await)
            }
            &Command::MotorResetPosition { motor, group } => hw(device.motor_reset_position(motor, group).await),
            &Command::Beep { group, tone, output, volume } => {
                let started = tokio::time::Instant::now();
                let mut computer = Duration::ZERO;
                if output.computer() {
                    let notes = audio::beep_notes(tone);
                    self.speak(&notes, volume)?;
                    computer = audio::length(&notes);
                }
                if output.devices() {
                    hw(device.beep(group, tone).await)?;
                }
                // Finish when the computer's beep has finished too.
                let left = computer.saturating_sub(started.elapsed());
                if !left.is_zero() {
                    self.tick(left).await?;
                }
                Ok(())
            }
            &Command::PlayTune { group, ref notes, output, volume } => {
                for note in notes {
                    if output.computer() {
                        // A short gap between notes, like the devices' beeps.
                        self.speak(&[(note.frequency, (note.milliseconds * 85 / 100) as u32)], volume)?;
                    }
                    if output.devices() {
                        let tone = Tone { pattern: BeepPattern::Single, frequency: note.frequency, count: 1 };
                        hw(device.beep_start(group, tone).await)?;
                    }
                    self.tick(Duration::from_millis(note.milliseconds)).await?;
                }
                Ok(())
            }
            &Command::Wait { milliseconds } => self.tick(Duration::from_millis(milliseconds)).await,
            Command::WaitUntil { condition } => {
                while !self.check(condition).await? {
                    self.tick(POLL_INTERVAL).await?;
                }
                Ok(())
            }
            Command::Repeat { count, body } => {
                for _ in 0..*count {
                    self.steps(body).await?;
                    self.tick(LOOP_TICK).await?;
                }
                Ok(())
            }
            Command::Forever { body } => loop {
                self.steps(body).await?;
                self.tick(LOOP_TICK).await?;
            },
            Command::If { condition, then, otherwise } => {
                if self.check(condition).await? {
                    self.steps(then).await
                } else {
                    self.steps(otherwise).await
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::CardGroup;
    use crate::hardware::mock::MockDevice;
    use crate::program::compiler::compile;
    use crate::visual::{DriveMode, Workspace};

    fn setup() -> (MockDevice, Executor) {
        let mock = MockDevice::new();
        let exec = Executor::new(Arc::new(mock.clone()));
        (mock, exec)
    }

    #[tokio::test(start_paused = true)]
    async fn starter_program_runs_then_stops_motor() {
        let (mock, exec) = setup();
        mock.connect().await.unwrap();
        let program = compile(&Workspace::example()).unwrap().program;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel = CancellationToken::new();

        let run = tokio::spawn(async move { exec.run(program, cancel, tx).await });
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(mock.with_state(|s| s.single.speed), 50, "motor running during wait");

        assert_eq!(run.await.unwrap(), RunOutcome::Completed);
        assert_eq!(mock.with_state(|s| s.single.speed), 0, "motor stopped at end");

        let mut started = Vec::new();
        while let Ok(e) = rx.try_recv() {
            if let ExecEvent::BlockStarted(id) = e {
                started.push(id);
            }
        }
        assert_eq!(started.len(), 4);
    }

    #[tokio::test(start_paused = true)]
    async fn power_for_time_runs_then_stops() {
        use crate::hardware::{Direction, Motor};
        use crate::visual::BlockKind;
        let (mock, exec) = setup();
        mock.connect().await.unwrap();
        let mut ws = Workspace::new();
        let blocks = vec![
            ws.make_block(BlockKind::WhenRun),
            ws.make_block(BlockKind::MotorForTime {
                motor: Motor::Single,
                group: CardGroup::NoCard,
                direction: Direction::CounterClockwise,
                mode: DriveMode::Power,
                speed: 80,
                seconds: 1.0,
            }),
        ];
        ws.add_stack([0.0, 0.0], blocks);
        let program = compile(&ws).unwrap().program;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        let run = tokio::spawn(async move { exec.run(program, CancellationToken::new(), tx).await });
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(mock.with_state(|s| s.single.speed), -80, "running anticlockwise at 80% power");

        assert_eq!(run.await.unwrap(), RunOutcome::Completed);
        assert_eq!(mock.with_state(|s| s.single.speed), 0, "stopped after the time");
    }

    #[tokio::test(start_paused = true)]
    async fn play_tune_beeps_each_note_in_time() {
        use crate::program::tune::Tune;
        use crate::visual::BlockKind;
        let (mock, exec) = setup();
        mock.connect().await.unwrap();
        let mut ws = Workspace::new();
        let blocks = vec![
            ws.make_block(BlockKind::WhenRun),
            ws.make_block(BlockKind::PlayTune {
                group: CardGroup::NoCard,
                tune: Tune::TaDa,
                output: crate::audio::SoundOutput::Devices,
                volume: 100,
            }),
        ];
        ws.add_stack([0.0, 0.0], blocks);
        let program = compile(&ws).unwrap().program;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        let start = tokio::time::Instant::now();
        let outcome = exec.run(program, CancellationToken::new(), tx).await;
        assert_eq!(outcome, RunOutcome::Completed);
        let total: u64 = Tune::TaDa.notes().iter().map(|n| n.milliseconds).sum();
        assert!(start.elapsed() >= Duration::from_millis(total));
        let beeps = mock.with_state(|s| s.log.iter().filter(|e| e.text.starts_with("beep ")).count());
        assert_eq!(beeps, Tune::TaDa.notes().len());
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_stops_all_motors() {
        let (mock, exec) = setup();
        mock.connect().await.unwrap();
        let program = compile(&Workspace::example()).unwrap().program;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        let run = tokio::spawn(async move { exec.run(program, c2, tx).await });
        tokio::time::sleep(Duration::from_millis(500)).await;
        cancel.cancel();
        assert_eq!(run.await.unwrap(), RunOutcome::Stopped);
        assert_eq!(mock.with_state(|s| s.single.speed), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn colour_program_reacts_to_simulated_sensor() {
        let (mock, exec) = setup();
        mock.connect().await.unwrap();
        let program = compile(&crate::visual::tests::colour_program()).unwrap().program;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        let run = tokio::spawn(async move { exec.run(program, c2, tx).await });

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(mock.with_state(|s| s.single.speed), 0, "no colour: stopped");
        mock.with_state(|s| s.color = crate::hardware::LegoColor::Red);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(mock.with_state(|s| s.single.speed), 100, "red: full speed");
        mock.with_state(|s| s.color = crate::hardware::LegoColor::Blue);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(mock.with_state(|s| s.single.speed), 0, "blue: stopped again");

        cancel.cancel();
        assert_eq!(run.await.unwrap(), RunOutcome::Stopped);
    }

    #[tokio::test(start_paused = true)]
    async fn positions_count_from_zero_at_run() {
        use crate::hardware::{Direction, Motor};
        use crate::program::condition::{Comparison, Condition};
        use crate::visual::BlockKind;

        let (mock, exec) = setup();
        mock.connect().await.unwrap();
        // Lots of turning before RUN (like a motor that's been used a while).
        mock.with_state(|s| s.single.position = 10_005.0);

        // RUN: motor at 10% until position > 90°, then stop.
        let mut ws = Workspace::new();
        let motor = Motor::Single;
        let blocks = vec![
            ws.make_block(BlockKind::WhenRun),
            ws.make_block(BlockKind::RunMotor { motor, group: CardGroup::NoCard, direction: Direction::Clockwise, mode: DriveMode::Speed, speed: 10 }),
            ws.make_block(BlockKind::WaitUntil {
                condition: Condition::MotorPosition { motor, cmp: Comparison::Greater, degrees: 90, group: CardGroup::NoCard }
            }),
            ws.make_block(BlockKind::StopMotor { motor, group: CardGroup::NoCard }),
            ws.make_block(BlockKind::ResetPosition { motor, group: CardGroup::NoCard }),
        ];
        ws.add_stack([0.0, 0.0], blocks);
        let program = compile(&ws).unwrap().program;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let run = tokio::spawn(async move { exec.run(program, CancellationToken::new(), tx).await });

        // 10% = 100°/s in the simulator: the wait must take ~0.9 s, not 0 s.
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(mock.with_state(|s| s.single.speed), 10, "still turning: counted from 0 at RUN");
        assert_eq!(run.await.unwrap(), RunOutcome::Completed);
        assert_eq!(mock.with_state(|s| s.single.position), 0.0, "Reset Position block zeroed it");
    }

    #[derive(Default)]
    struct FakeSpeaker {
        played: std::sync::Mutex<Vec<(Vec<(u16, u32)>, u32)>>,
    }

    impl Speaker for FakeSpeaker {
        fn play(&self, notes: &[(u16, u32)], volume: u32) -> Result<(), String> {
            self.played.lock().unwrap().push((notes.to_vec(), volume));
            Ok(())
        }

        fn stop(&self) {}
    }

    #[tokio::test(start_paused = true)]
    async fn sounds_play_on_the_computer() {
        use crate::audio::SoundOutput;
        use crate::hardware::BeepPattern;
        use crate::program::tune::Tune;
        use crate::visual::BlockKind;

        // No LEGO device connected at all: computer-only sound still plays.
        let mock = MockDevice::new();
        let speaker = Arc::new(FakeSpeaker::default());
        let exec = Executor::new(Arc::new(mock.clone())).with_speaker(speaker.clone());
        let mut ws = Workspace::new();
        let blocks = vec![
            ws.make_block(BlockKind::WhenRun),
            ws.make_block(BlockKind::Beep {
                group: CardGroup::NoCard,
                pattern: BeepPattern::Double,
                note: 96,
                count: 1,
                output: SoundOutput::Computer,
                volume: 80,
            }),
            ws.make_block(BlockKind::PlayTune { group: CardGroup::NoCard, tune: Tune::TaDa, output: SoundOutput::Computer, volume: 100 }),
        ];
        ws.add_stack([0.0, 0.0], blocks);
        let program = compile(&ws).unwrap().program;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let start = tokio::time::Instant::now();
        assert_eq!(exec.run(program, CancellationToken::new(), tx).await, RunOutcome::Completed);

        let played = speaker.played.lock().unwrap();
        assert_eq!(played.len(), 1 + Tune::TaDa.notes().len());
        assert_eq!(played[0].1, 80);
        assert_eq!(played[0].0.iter().filter(|n| n.0 > 0).count(), 2, "double beep");
        let beep = crate::audio::length(&played[0].0);
        let tune: u64 = Tune::TaDa.notes().iter().map(|n| n.milliseconds).sum();
        assert!(start.elapsed() >= beep + Duration::from_millis(tune), "waits for the sounds to finish");
        assert!(mock.with_state(|s| s.log.iter().all(|e| !e.text.starts_with("beep "))), "devices stay quiet");
    }

    struct FakeCamera(std::sync::Mutex<CameraReading>);

    impl PoseSource for FakeCamera {
        fn reading(&self) -> CameraReading {
            self.0.lock().unwrap().clone()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn camera_pose_starts_the_motor() {
        use crate::camera::{CameraPose, Detection};
        use crate::hardware::{Direction, Motor};
        use crate::visual::BlockKind;

        let mock = MockDevice::new();
        mock.connect().await.unwrap();
        let camera = Arc::new(FakeCamera(std::sync::Mutex::new(CameraReading::Starting)));
        let exec = Executor::new(Arc::new(mock.clone())).with_camera(camera.clone());
        let mut ws = Workspace::new();
        let motor = Motor::Single;
        let blocks = vec![
            ws.make_block(BlockKind::WhenRun),
            ws.make_block(BlockKind::WaitUntil { condition: Condition::Camera { pose: CameraPose::BothArmsUp } }),
            ws.make_block(BlockKind::RunMotor { motor, group: CardGroup::NoCard, direction: Direction::Clockwise, mode: DriveMode::Speed, speed: 30 }),
        ];
        ws.add_stack([0.0, 0.0], blocks);
        let program = compile(&ws).unwrap().program;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let run = tokio::spawn(async move { exec.run(program, CancellationToken::new(), tx).await });

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(mock.with_state(|s| s.single.speed), 0, "camera still starting: keep waiting");
        let arms_down = Detection { person: true, builtin: Some(CameraPose::ArmsDown), custom: None, pose: None };
        *camera.0.lock().unwrap() = CameraReading::Live(arms_down.clone());
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(mock.with_state(|s| s.single.speed), 0, "arms down: keep waiting");
        let arms_up = Detection { builtin: Some(CameraPose::BothArmsUp), ..arms_down.clone() };
        *camera.0.lock().unwrap() = CameraReading::Live(arms_up);
        assert_eq!(run.await.unwrap(), RunOutcome::Completed);
        assert_eq!(mock.with_state(|s| s.single.speed), 30);
    }

    #[tokio::test(start_paused = true)]
    async fn camera_off_fails_the_run() {
        let (mock, exec) = setup();
        mock.connect().await.unwrap();
        let exec = exec.with_camera(Arc::new(FakeCamera(std::sync::Mutex::new(CameraReading::Off))));
        let mut ws = Workspace::new();
        let blocks = vec![
            ws.make_block(crate::visual::BlockKind::WhenRun),
            ws.make_block(crate::visual::BlockKind::WaitUntil {
                condition: Condition::Camera { pose: crate::camera::CameraPose::Person },
            }),
        ];
        ws.add_stack([0.0, 0.0], blocks);
        let program = compile(&ws).unwrap().program;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let outcome = exec.run(program, CancellationToken::new(), tx).await;
        assert_eq!(outcome, RunOutcome::Failed("the camera is off".into()));
    }

    #[tokio::test(start_paused = true)]
    async fn disconnect_fails_the_run_and_stops_motors() {
        let (mock, exec) = setup();
        mock.connect().await.unwrap();
        let program = compile(&Workspace::example()).unwrap().program;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let run = tokio::spawn(async move { exec.run(program, CancellationToken::new(), tx).await });
        tokio::time::sleep(Duration::from_millis(500)).await;
        mock.with_state(|s| s.connection = crate::hardware::ConnectionState::Disconnected);
        assert!(matches!(run.await.unwrap(), RunOutcome::Failed(_)));
        assert_eq!(mock.with_state(|s| s.single.speed), 0);
    }
}
