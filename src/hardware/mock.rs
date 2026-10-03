//! Simulated LEGO Education hardware.
//!
//! Simulates the four Computer Science & AI kit devices so the whole
//! application can be used and tested without physical hardware.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;

use super::{
    AttachedDevice, CardGroup, ConnectionState, DeviceKind, Direction, HardwareError, HwResult, LegoColor,
    LegoDevice, Lever, Motor, SensorQuery, SensorValue, Speed,
};

/// Degrees per second per 1% of speed (100% ≈ 1000°/s).
const DEG_PER_SEC_PER_PERCENT: f64 = 10.0;
const LOG_CAPACITY: usize = 200;
const CONNECT_DELAY: Duration = Duration::from_millis(400);

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub time: String,
    pub text: String,
}

/// One simulated motor.
#[derive(Debug, Clone, Default)]
pub struct MockMotor {
    /// Signed speed in percent (negative = anticlockwise).
    pub speed: i8,
    /// Relative position in degrees.
    pub position: f64,
    /// Bumped on every new command so a finished timed command does not stop
    /// a motor that has since been given a different command.
    generation: u64,
}

/// Complete simulator state. The UI reads and edits this directly.
#[derive(Debug)]
pub struct MockState {
    pub connection: ConnectionState,
    /// Which simulated devices are switched on, indexed like `DeviceKind::ALL`.
    pub attached: [bool; 4],
    pub groups: [CardGroup; 4],
    pub single: MockMotor,
    pub left: MockMotor,
    pub right: MockMotor,
    pub color: LegoColor,
    pub reflection: u8,
    pub lever_left: i8,
    pub lever_right: i8,
    /// Button pressed state per device, indexed like `DeviceKind::ALL`.
    pub buttons: [bool; 4],
    pub log: VecDeque<LogEntry>,
    last_tick: Instant,
}

impl Default for MockState {
    fn default() -> Self {
        Self {
            connection: ConnectionState::Disconnected,
            attached: [true; 4],
            groups: [CardGroup::NoCard; 4],
            single: MockMotor::default(),
            left: MockMotor::default(),
            right: MockMotor::default(),
            color: LegoColor::None,
            reflection: 0,
            lever_left: 0,
            lever_right: 0,
            buttons: [false; 4],
            log: VecDeque::new(),
            last_tick: Instant::now(),
        }
    }
}

fn kind_index(kind: DeviceKind) -> usize {
    DeviceKind::ALL.iter().position(|k| *k == kind).unwrap_or(0)
}

impl MockState {
    pub fn is_attached(&self, kind: DeviceKind) -> bool {
        self.attached[kind_index(kind)]
    }

    pub fn group(&self, kind: DeviceKind) -> CardGroup {
        self.groups[kind_index(kind)]
    }

    pub fn set_group(&mut self, kind: DeviceKind, group: CardGroup) {
        self.groups[kind_index(kind)] = group;
    }

    pub fn set_attached(&mut self, kind: DeviceKind, on: bool) {
        let i = kind_index(kind);
        if self.attached[i] != on {
            self.attached[i] = on;
            if !on {
                // A device that switches off stops its motors.
                match kind {
                    DeviceKind::SingleMotor => self.single.speed = 0,
                    DeviceKind::DoubleMotor => {
                        self.left.speed = 0;
                        self.right.speed = 0;
                    }
                    _ => {}
                }
            }
            self.log(format!(
                "{} {}",
                kind.label(),
                if on { "switched on" } else { "switched off" }
            ));
        }
    }

    pub fn button(&self, kind: DeviceKind) -> bool {
        self.buttons[kind_index(kind)]
    }

    pub fn set_button(&mut self, kind: DeviceKind, pressed: bool) {
        self.buttons[kind_index(kind)] = pressed;
    }

    pub fn log(&mut self, text: impl Into<String>) {
        if self.log.len() >= LOG_CAPACITY {
            self.log.pop_front();
        }
        self.log.push_back(LogEntry {
            time: chrono::Local::now().format("%H:%M:%S").to_string(),
            text: text.into(),
        });
    }

    /// Integrate motor positions up to now.
    pub fn advance(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f64();
        self.last_tick = now;
        for m in [&mut self.single, &mut self.left, &mut self.right] {
            m.position += m.speed as f64 * DEG_PER_SEC_PER_PERCENT * dt;
        }
    }

    pub fn any_motor_running(&self) -> bool {
        self.single.speed != 0 || self.left.speed != 0 || self.right.speed != 0
    }

    fn motors_mut(&mut self, motor: Motor) -> Vec<&mut MockMotor> {
        match motor {
            Motor::Single => vec![&mut self.single],
            Motor::DoubleLeft => vec![&mut self.left],
            Motor::DoubleRight => vec![&mut self.right],
            Motor::DoubleBoth => vec![&mut self.left, &mut self.right],
        }
    }

    fn motor_speeds(&mut self, motor: Motor) -> Vec<i8> {
        self.motors_mut(motor).into_iter().map(|m| m.speed).collect()
    }

    fn check(&self, kind: DeviceKind, group: CardGroup) -> HwResult<()> {
        if self.connection != ConnectionState::Connected {
            return Err(HardwareError::NotConnected);
        }
        if !self.is_attached(kind) || !group.matches_device_group(self.group(kind)) {
            return Err(HardwareError::DeviceMissing(kind));
        }
        Ok(())
    }

    fn check_group(&self, group: CardGroup) -> HwResult<()> {
        if self.connection != ConnectionState::Connected {
            return Err(HardwareError::NotConnected);
        }
        if !DeviceKind::ALL
            .into_iter()
            .any(|kind| self.is_attached(kind) && group.matches_device_group(self.group(kind)))
        {
            return Err(HardwareError::DeviceGroupMissing(group));
        }
        Ok(())
    }

    /// Set a motor's speed, returning the new command generations.
    fn set_motor(&mut self, motor: Motor, speed: i8) -> Vec<u64> {
        self.advance();
        self.motors_mut(motor)
            .into_iter()
            .map(|m| {
                m.speed = speed;
                m.generation += 1;
                m.generation
            })
            .collect()
    }

    /// Stop a motor only if no newer command has been given to it.
    fn finish_motor(&mut self, motor: Motor, generations: &[u64]) -> bool {
        self.advance();
        let mut stopped = false;
        for (m, g) in self.motors_mut(motor).into_iter().zip(generations) {
            if m.generation == *g {
                m.speed = 0;
                stopped = true;
            }
        }
        stopped
    }

    fn stop_everything(&mut self) {
        self.advance();
        for m in [&mut self.single, &mut self.left, &mut self.right] {
            m.speed = 0;
            m.generation += 1;
        }
    }
}

/// Simulated LEGO devices. Cheap to clone; clones share state.
#[derive(Clone, Default)]
pub struct MockDevice {
    state: Arc<Mutex<MockState>>,
}

impl MockDevice {
    pub fn new() -> Self {
        Self::default()
    }

    /// Access the simulator state (positions are brought up to date first).
    pub fn with_state<R>(&self, f: impl FnOnce(&mut MockState) -> R) -> R {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.advance();
        f(&mut state)
    }

    async fn timed(
        &self,
        motor: Motor,
        group: CardGroup,
        direction: Direction,
        speed: Speed,
        duration: Duration,
        description: String,
    ) -> HwResult<()> {
        let generations = self.with_state(|s| {
            s.check(motor.device(), group)?;
            s.log(description);
            Ok::<_, HardwareError>(s.set_motor(motor, speed.signed(direction)))
        })?;
        tokio::time::sleep(duration).await;
        self.with_state(|s| {
            s.check(motor.device(), group)?;
            if s.finish_motor(motor, &generations) {
                s.log(format!("{} → stop", motor.label()));
            }
            Ok(())
        })
    }
}

#[async_trait]
impl LegoDevice for MockDevice {
    fn backend_name(&self) -> &'static str {
        "Simulator"
    }

    fn connection(&self) -> ConnectionState {
        self.with_state(|s| s.connection)
    }

    async fn connect(&self) -> HwResult<()> {
        self.with_state(|s| {
            s.connection = ConnectionState::Connecting;
            s.log("Connecting to simulated devices…");
        });
        tokio::time::sleep(CONNECT_DELAY).await;
        self.with_state(|s| {
            s.connection = ConnectionState::Connected;
            s.log("Connected");
        });
        Ok(())
    }

    async fn disconnect(&self) -> HwResult<()> {
        self.with_state(|s| {
            s.stop_everything();
            s.connection = ConnectionState::Disconnected;
            s.log("Disconnected");
        });
        Ok(())
    }

    async fn discover(&self) -> HwResult<Vec<AttachedDevice>> {
        self.with_state(|s| {
            if s.connection != ConnectionState::Connected {
                return Err(HardwareError::NotConnected);
            }
            Ok(DeviceKind::ALL
                .into_iter()
                .filter(|k| s.is_attached(*k))
                .map(|kind| AttachedDevice {
                    kind,
                    group: s.group(kind),
                    name: "simulated".into(),
                })
                .collect())
        })
    }

    async fn motor_run(&self, motor: Motor, group: CardGroup, direction: Direction, speed: Speed) -> HwResult<()> {
        self.with_state(|s| {
            s.check(motor.device(), group)?;
            let signed = speed.signed(direction);
            let changed = s.motor_speeds(motor).iter().any(|v| *v != signed);
            s.set_motor(motor, signed);
            if changed {
                s.log(format!("{} → {}%", motor.label(), signed));
            }
            Ok(())
        })
    }

    async fn motor_run_for_time(
        &self,
        motor: Motor,
        group: CardGroup,
        direction: Direction,
        speed: Speed,
        duration: Duration,
    ) -> HwResult<()> {
        let text = format!(
            "{} → {}% for {:.1} s",
            motor.label(),
            speed.signed(direction),
            duration.as_secs_f64()
        );
        self.timed(motor, group, direction, speed, duration, text).await
    }

    async fn motor_run_for_degrees(
        &self,
        motor: Motor,
        group: CardGroup,
        direction: Direction,
        speed: Speed,
        degrees: u32,
    ) -> HwResult<()> {
        if speed.percent() == 0 || degrees == 0 {
            return self.with_state(|s| s.check(motor.device(), group));
        }
        let secs = degrees as f64 / (speed.percent() as f64 * DEG_PER_SEC_PER_PERCENT);
        let text = format!(
            "{} → {}% for {}°",
            motor.label(),
            speed.signed(direction),
            degrees
        );
        self.timed(motor, group, direction, speed, Duration::from_secs_f64(secs), text)
            .await
    }

    async fn motor_stop(&self, motor: Motor, group: CardGroup) -> HwResult<()> {
        self.with_state(|s| {
            s.check(motor.device(), group)?;
            let changed = s.motor_speeds(motor).iter().any(|v| *v != 0);
            s.set_motor(motor, 0);
            if changed {
                s.log(format!("{} → stop", motor.label()));
            }
            Ok(())
        })
    }

    async fn stop_all(&self) -> HwResult<()> {
        self.with_state(|s| {
            let was_running = s.any_motor_running();
            s.stop_everything();
            if was_running {
                s.log("All motors → stop");
            }
            Ok(())
        })
    }

    async fn motor_reset_position(&self, motor: Motor, group: CardGroup) -> HwResult<()> {
        self.with_state(|s| {
            s.check(motor.device(), group)?;
            for m in s.motors_mut(motor) {
                m.position = 0.0;
            }
            s.log(format!("{} → position 0°", motor.label()));
            Ok(())
        })
    }

    async fn beep(&self, group: CardGroup) -> HwResult<()> {
        self.with_state(|s| {
            s.check_group(group)?;
            s.log(format!("Beep on {} group", group.label()));
            Ok(())
        })
    }

    async fn reset_motor_positions(&self) -> HwResult<()> {
        self.with_state(|s| {
            if s.connection != ConnectionState::Connected {
                return Err(HardwareError::NotConnected);
            }
            for m in [&mut s.single, &mut s.left, &mut s.right] {
                m.position = 0.0;
            }
            Ok(())
        })
    }

    async fn read_sensors(&self, query: SensorQuery, group: CardGroup) -> HwResult<Vec<SensorValue>> {
        self.with_state(|s| match query {
            SensorQuery::Color => {
                s.check(DeviceKind::ColorSensor, group)?;
                Ok(vec![SensorValue::Color(s.color)])
            }
            SensorQuery::Reflection => {
                s.check(DeviceKind::ColorSensor, group)?;
                Ok(vec![SensorValue::Percent(s.reflection as i32)])
            }
            SensorQuery::MotorPosition(motor) => {
                s.check(motor.device(), group)?;
                let pos = match motor {
                    Motor::Single => s.single.position,
                    Motor::DoubleLeft | Motor::DoubleBoth => s.left.position,
                    Motor::DoubleRight => s.right.position,
                };
                Ok(vec![SensorValue::Degrees(pos.round() as i64)])
            }
            SensorQuery::ControllerLever(lever) => {
                s.check(DeviceKind::Controller, group)?;
                let v = match lever {
                    Lever::Left => s.lever_left,
                    Lever::Right => s.lever_right,
                };
                Ok(vec![SensorValue::Percent(v as i32)])
            }
            SensorQuery::Button(kind) => {
                s.check(kind, group)?;
                Ok(vec![SensorValue::Pressed(s.button(kind))])
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn timed_run_stops_motor_afterwards() {
        let dev = MockDevice::new();
        dev.connect().await.unwrap();
        dev.motor_run_for_time(
            Motor::Single,
            CardGroup::NoCard,
            Direction::Clockwise,
            Speed::new(50),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert_eq!(dev.with_state(|s| s.single.speed), 0);
    }

    #[tokio::test]
    async fn commands_fail_when_disconnected() {
        let dev = MockDevice::new();
        let err = dev
            .motor_run(Motor::Single, CardGroup::NoCard, Direction::Clockwise, Speed::new(50))
            .await
            .unwrap_err();
        assert_eq!(err, HardwareError::NotConnected);
    }

    #[tokio::test]
    async fn missing_device_is_reported() {
        let dev = MockDevice::new();
        dev.connect().await.unwrap();
        dev.with_state(|s| s.set_attached(DeviceKind::DoubleMotor, false));
        let err = dev.motor_stop(Motor::DoubleLeft, CardGroup::NoCard).await.unwrap_err();
        assert_eq!(err, HardwareError::DeviceMissing(DeviceKind::DoubleMotor));
    }

    #[tokio::test]
    async fn motor_commands_require_the_selected_card_group() {
        let dev = MockDevice::new();
        dev.connect().await.unwrap();
        dev.with_state(|s| s.set_group(DeviceKind::SingleMotor, CardGroup::Blue));

        let missing = dev
            .motor_run(Motor::Single, CardGroup::NoCard, Direction::Clockwise, Speed::new(40))
            .await
            .unwrap_err();
        assert_eq!(missing, HardwareError::DeviceMissing(DeviceKind::SingleMotor));

        dev.motor_run(Motor::Single, CardGroup::Blue, Direction::Clockwise, Speed::new(40))
            .await
            .unwrap();
        assert_eq!(dev.with_state(|s| s.single.speed), 40);
    }

    #[tokio::test]
    async fn sensor_readings_are_scoped_to_the_selected_card_group() {
        let dev = MockDevice::new();
        dev.connect().await.unwrap();
        dev.with_state(|s| {
            s.set_group(DeviceKind::ColorSensor, CardGroup::Blue);
            s.color = LegoColor::Red;
        });

        let values = dev.read_sensors(SensorQuery::Color, CardGroup::Blue).await.unwrap();
        assert_eq!(values, vec![SensorValue::Color(LegoColor::Red)]);
        assert_eq!(dev.read_sensors(SensorQuery::Color, CardGroup::NoCard).await.unwrap_err(), HardwareError::DeviceMissing(DeviceKind::ColorSensor));
    }

    #[tokio::test]
    async fn beep_targets_devices_in_its_card_group() {
        let dev = MockDevice::new();
        dev.connect().await.unwrap();
        dev.with_state(|s| {
            for kind in DeviceKind::ALL {
                s.set_group(kind, CardGroup::Blue);
            }
        });

        assert_eq!(dev.beep(CardGroup::NoCard).await.unwrap_err(), HardwareError::DeviceGroupMissing(CardGroup::NoCard));
        dev.beep(CardGroup::Blue).await.unwrap();
        assert!(dev.with_state(|s| s.log.back().unwrap().text.contains("Blue card")));
    }
}
