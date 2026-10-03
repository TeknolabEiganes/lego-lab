//! Hardware abstraction.
//!
//! Everything above this module (executor, UI) talks to LEGO hardware only
//! through the [`LegoDevice`] trait. Protocol details live in the concrete
//! backends: [`mock`] (simulator) and [`lego`] (real BLE hardware).
//!
//! The LEGO Education Computer Science & AI kits have no hub with ports.
//! Every element (Single Motor, Double Motor, Colour Sensor, Controller) is a
//! separate Bluetooth LE device. A [`Motor`] names the motor layout; commands
//! combine it with a [`CardGroup`] to target one device group rather than a hub port.

pub mod lego;
pub mod mock;

use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Which motor a command addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Motor {
    /// The Single Motor element.
    Single,
    /// Left side of the Double Motor element.
    DoubleLeft,
    /// Right side of the Double Motor element.
    DoubleRight,
    /// Both sides of the Double Motor element.
    DoubleBoth,
}

impl Motor {
    pub const ALL: [Motor; 4] = [
        Motor::Single,
        Motor::DoubleLeft,
        Motor::DoubleRight,
        Motor::DoubleBoth,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Motor::Single => "Single motor",
            Motor::DoubleLeft => "Double motor · left",
            Motor::DoubleRight => "Double motor · right",
            Motor::DoubleBoth => "Double motor · both",
        }
    }

    /// The physical device this motor belongs to.
    pub fn device(self) -> DeviceKind {
        match self {
            Motor::Single => DeviceKind::SingleMotor,
            _ => DeviceKind::DoubleMotor,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    Clockwise,
    CounterClockwise,
}

impl Direction {
    pub fn label(self) -> &'static str {
        match self {
            Direction::Clockwise => "↻ clockwise",
            Direction::CounterClockwise => "↺ anticlockwise",
        }
    }

    pub fn sign(self) -> i8 {
        match self {
            Direction::Clockwise => 1,
            Direction::CounterClockwise => -1,
        }
    }
}

/// Motor speed in percent. Always within the hardware range `0..=100`.
///
/// Direction is carried separately (the real protocol takes a direction
/// field plus a speed), so the value itself is never negative.
/// Deserialising clamps, so a project file can never carry an out-of-range speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "i64", into = "u8")]
pub struct Speed(u8);

impl From<i64> for Speed {
    fn from(v: i64) -> Self {
        Speed::new(v)
    }
}

impl From<Speed> for u8 {
    fn from(s: Speed) -> u8 {
        s.0
    }
}

impl Speed {
    pub const MAX: u8 = 100;

    /// Construct a speed, clamping into the valid hardware range.
    pub fn new(percent: i64) -> Self {
        Speed(percent.clamp(0, Self::MAX as i64) as u8)
    }

    pub fn percent(self) -> u8 {
        self.0
    }

    pub fn signed(self, direction: Direction) -> i8 {
        self.0 as i8 * direction.sign()
    }
}

/// Every device type in the LEGO Education Computer Science & AI kits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DeviceKind {
    SingleMotor,
    DoubleMotor,
    ColorSensor,
    Controller,
}

/// Connection-card colour used to group physical devices. This is deliberately
/// separate from [`LegoColor`], which represents colours seen by a Colour Sensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
pub enum CardGroup {
    #[default]
    NoCard,
    Pink,
    Purple,
    Blue,
    Azure,
    Turquoise,
    Green,
    Yellow,
    Orange,
    Red,
    White,
}

impl CardGroup {
    pub const ALL: [CardGroup; 11] = [
        CardGroup::NoCard,
        CardGroup::Pink,
        CardGroup::Purple,
        CardGroup::Blue,
        CardGroup::Azure,
        CardGroup::Turquoise,
        CardGroup::Green,
        CardGroup::Yellow,
        CardGroup::Orange,
        CardGroup::Red,
        CardGroup::White,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CardGroup::NoCard => "No card",
            CardGroup::Pink => "Pink card",
            CardGroup::Purple => "Purple card",
            CardGroup::Blue => "Blue card",
            CardGroup::Azure => "Azure card",
            CardGroup::Turquoise => "Turquoise card",
            CardGroup::Green => "Green card",
            CardGroup::Yellow => "Yellow card",
            CardGroup::Orange => "Orange card",
            CardGroup::Red => "Red card",
            CardGroup::White => "White card",
        }
    }

    /// LEGO firmware connection-card colour number. Unknown and zero values
    /// are treated as unassigned cards.
    pub fn from_firmware(color: i8) -> Self {
        match color {
            1 => CardGroup::Pink,
            2 => CardGroup::Purple,
            3 => CardGroup::Blue,
            4 => CardGroup::Azure,
            5 => CardGroup::Turquoise,
            6 => CardGroup::Green,
            7 => CardGroup::Yellow,
            8 => CardGroup::Orange,
            9 => CardGroup::Red,
            10 => CardGroup::White,
            _ => CardGroup::NoCard,
        }
    }
}

/// A device kind within a connection-card colour group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DeviceTarget {
    pub kind: DeviceKind,
    #[serde(default)]
    pub group: CardGroup,
}

impl DeviceKind {
    pub const ALL: [DeviceKind; 4] = [
        DeviceKind::SingleMotor,
        DeviceKind::DoubleMotor,
        DeviceKind::ColorSensor,
        DeviceKind::Controller,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DeviceKind::SingleMotor => "Single Motor",
            DeviceKind::DoubleMotor => "Double Motor",
            DeviceKind::ColorSensor => "Colour Sensor",
            DeviceKind::Controller => "Controller",
        }
    }
}

/// A device found during discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachedDevice {
    pub kind: DeviceKind,
    pub group: CardGroup,
    /// Human readable identification, e.g. a connection card or "simulated".
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

/// Colours the Colour Sensor reports (Coding Canvas numbering).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LegoColor {
    None,
    Red,
    Yellow,
    Blue,
    Teal,
    Green,
    Purple,
    White,
}

impl LegoColor {
    pub const ALL: [LegoColor; 8] = [
        LegoColor::None,
        LegoColor::Red,
        LegoColor::Yellow,
        LegoColor::Blue,
        LegoColor::Teal,
        LegoColor::Green,
        LegoColor::Purple,
        LegoColor::White,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LegoColor::None => "no colour",
            LegoColor::Red => "red",
            LegoColor::Yellow => "yellow",
            LegoColor::Blue => "blue",
            LegoColor::Teal => "teal",
            LegoColor::Green => "green",
            LegoColor::Purple => "purple",
            LegoColor::White => "white",
        }
    }

    pub fn rgb(self) -> [u8; 3] {
        match self {
            LegoColor::None => [40, 40, 40],
            LegoColor::Red => [0xde, 0x1a, 0x21],
            LegoColor::Yellow => [0xff, 0xd4, 0x00],
            LegoColor::Blue => [0x00, 0x6c, 0xb8],
            LegoColor::Teal => [0x00, 0xa3, 0xa3],
            LegoColor::Green => [0x61, 0xa8, 0x36],
            LegoColor::Purple => [0x4b, 0x2f, 0x91],
            LegoColor::White => [0xf4, 0xf4, 0xf4],
        }
    }
}

/// A sensor reading the executor can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorQuery {
    Color,
    Reflection,
    MotorPosition(Motor),
    ControllerLever(Lever),
    Button(DeviceKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Lever {
    Left,
    Right,
}

impl Lever {
    pub fn label(self) -> &'static str {
        match self {
            Lever::Left => "Left",
            Lever::Right => "Right",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SensorValue {
    Color(LegoColor),
    Percent(i32),
    Degrees(i64),
    Pressed(bool),
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum HardwareError {
    #[error("not connected to LEGO hardware")]
    NotConnected,
    #[error("the {} is not connected", .0.label())]
    DeviceMissing(DeviceKind),
    #[error("communication failed: {0}")]
    Communication(String),
}

pub type HwResult<T> = Result<T, HardwareError>;

/// Asynchronous interface to a set of LEGO Education devices.
///
/// Methods take `&self` so that a long-running command (e.g. "motor for
/// 10 seconds") never blocks `stop_all`: STOP must always be responsive.
/// Implementations use interior mutability.
#[async_trait]
pub trait LegoDevice: Send + Sync {
    fn backend_name(&self) -> &'static str;

    fn connection(&self) -> ConnectionState;

    async fn connect(&self) -> HwResult<()>;

    async fn disconnect(&self) -> HwResult<()>;

    /// Which devices are currently available.
    async fn discover(&self) -> HwResult<Vec<AttachedDevice>>;

    /// Start a motor running continuously.
    async fn motor_run(&self, motor: Motor, group: CardGroup, direction: Direction, speed: Speed) -> HwResult<()>;

    /// Run a motor for a duration, resolving when it has finished.
    async fn motor_run_for_time(
        &self,
        motor: Motor,
        group: CardGroup,
        direction: Direction,
        speed: Speed,
        duration: Duration,
    ) -> HwResult<()>;

    /// Run a motor for a number of degrees, resolving when it has finished.
    async fn motor_run_for_degrees(
        &self,
        motor: Motor,
        group: CardGroup,
        direction: Direction,
        speed: Speed,
        degrees: u32,
    ) -> HwResult<()>;

    async fn motor_stop(&self, motor: Motor, group: CardGroup) -> HwResult<()>;

    /// Set a motor's position count back to 0°.
    async fn motor_reset_position(&self, motor: Motor, group: CardGroup) -> HwResult<()>;

    /// Set every connected motor's position count back to 0° (done when a
    /// program starts, so "motor position" means "turned since RUN").
    async fn reset_motor_positions(&self) -> HwResult<()>;

    /// Stop every motor on every device. Must be safe to call at any time.
    async fn stop_all(&self) -> HwResult<()>;

    /// Read the latest value from every matching device in this card group.
    async fn read_sensors(&self, query: SensorQuery, group: CardGroup) -> HwResult<Vec<SensorValue>>;

    /// Live per-device readings for the device panel (real hardware).
    fn snapshot(&self) -> Vec<DeviceSnapshot> {
        Vec::new()
    }
}

/// What the device panel shows about one connected device.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceSnapshot {
    pub kind: DeviceKind,
    pub group: CardGroup,
    /// Bluetooth address (matches `FoundDevice::address`).
    pub address: String,
    /// e.g. "blue 3683" (connection card).
    pub card: String,
    pub battery: Option<u8>,
    /// (label, value) pairs such as ("Position", "120°").
    pub readings: Vec<(String, String)>,
}

impl fmt::Display for Motor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_is_clamped_to_hardware_range() {
        assert_eq!(Speed::new(-50).percent(), 0);
        assert_eq!(Speed::new(250).percent(), 100);
        assert_eq!(Speed::new(42).percent(), 42);
        assert_eq!(Speed::new(30).signed(Direction::CounterClockwise), -30);
    }

    #[test]
    fn card_groups_use_connection_card_colours_not_sensor_colours() {
        assert_eq!(CardGroup::from_firmware(0), CardGroup::NoCard);
        assert_eq!(CardGroup::from_firmware(1), CardGroup::Pink);
        assert_eq!(CardGroup::from_firmware(3), CardGroup::Blue);
        assert_eq!(CardGroup::from_firmware(9), CardGroup::Red);
        assert_eq!(CardGroup::from_firmware(99), CardGroup::NoCard);
    }
}
