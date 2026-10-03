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

use crate::hardware::{HardwareError, LegoDevice};
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
}

pub struct Executor {
    device: Arc<dyn LegoDevice>,
}

impl Executor {
    pub fn new(device: Arc<dyn LegoDevice>) -> Self {
        Self { device }
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
            let cancel = cancel.clone();
            let events = events.clone();
            tasks.spawn(async move { run_script(&*device, script, &cancel, &events).await });
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

async fn run_script(
    device: &dyn LegoDevice,
    script: Script,
    cancel: &CancellationToken,
    events: &UnboundedSender<ExecEvent>
) -> Result<(), ScriptError> {
    let _ = events.send(ExecEvent::BlockStarted(script.hat));
    let _ = events.send(ExecEvent::BlockFinished(script.hat));
    let ctx = Ctx { device, cancel, events };
    ctx.steps(&script.steps).await
}

struct Ctx<'a> {
    device: &'a dyn LegoDevice,
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

    async fn check(&self, condition: &Condition) -> Result<bool, ScriptError> {
        let values = self.device.read_sensors(condition.query(), condition.target().group).await.map_err(ScriptError::Hardware)?;
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
            &Command::MotorResetPosition { motor, group } => hw(device.motor_reset_position(motor, group).await),
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
    use crate::visual::Workspace;

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
            ws.make_block(BlockKind::RunMotor { motor, group: CardGroup::NoCard, direction: Direction::Clockwise, speed: 10 }),
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
