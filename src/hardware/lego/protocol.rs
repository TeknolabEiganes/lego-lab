//! Wire protocol of the LEGO Education Computer Science & AI devices.
//!
//! Pure encoding/decoding, no Bluetooth. Layouts follow LEGO's official
//! `legoeducation` Python package (RPC 2.0); see docs/lego-hardware.md.
//! Messages are `[id: u8][payload…]`, little-endian, with no extra framing.

use super::super::{DeviceKind, Direction, LegoColor, Motor};

/// Protocol versions we speak. 1.0 (firmware 1.0.73, legoeducation 1.0.x)
/// and 2.0 (legoeducation 1.1.x) have byte-identical message definitions.
pub const SUPPORTED_RPC: [(u8, u8); 2] = [(1, 0), (2, 0)];

pub const INFO_REQUEST: u8 = 0;
pub const INFO_RESPONSE: u8 = 1;
pub const DEVICE_NOTIFICATION_REQUEST: u8 = 40;
pub const DEVICE_NOTIFICATION_RESPONSE: u8 = 41;
pub const DEVICE_NOTIFICATION: u8 = 60;
pub const MOTOR_RESET_RELATIVE_POSITION: u8 = 120;
pub const MOTOR_RESET_RELATIVE_POSITION_RESULT: u8 = 121;
pub const MOTOR_RUN: u8 = 122;
pub const MOTOR_RUN_RESULT: u8 = 123;
pub const MOTOR_RUN_FOR_DEGREES: u8 = 124;
pub const MOTOR_RUN_FOR_DEGREES_RESULT: u8 = 125;
pub const MOTOR_RUN_FOR_TIME: u8 = 126;
pub const MOTOR_RUN_FOR_TIME_RESULT: u8 = 127;
pub const MOTOR_STOP: u8 = 138;
pub const MOTOR_STOP_RESULT: u8 = 139;
pub const MOTOR_SET_SPEED: u8 = 140;

/// Motor results (121–145, odd) start with the motor bit mask, so
/// responses are matched per motor.
fn is_motor_result(id: u8) -> bool {
    (121..=145).contains(&id) && id % 2 == 1
}

/// Key used to match a response to the request waiting for it.
pub type ResponseKey = (u8, Option<u8>);

pub fn response_key(message: &[u8]) -> Option<ResponseKey> {
    let id = *message.first()?;
    if is_motor_result(id) {
        Some((id, Some(*message.get(1)?)))
    } else {
        Some((id, None))
    }
}

// ---- commands -------------------------------------------------------------

pub fn motor_mask(motor: Motor) -> u8 {
    match motor {
        Motor::Single | Motor::DoubleLeft => 1,
        Motor::DoubleRight => 2,
        Motor::DoubleBoth => 3,
    }
}

fn direction_byte(direction: Direction) -> u8 {
    match direction {
        Direction::Clockwise => 0,
        Direction::CounterClockwise => 1,
    }
}

pub fn info_request() -> Vec<u8> {
    vec![INFO_REQUEST]
}

/// Ask for sensor notifications every `delay_ms` (0 = off, else 15–1000).
pub fn device_notification_request(delay_ms: u16) -> Vec<u8> {
    let delay = if delay_ms == 0 { 0 } else { delay_ms.clamp(15, 1000) };
    let mut m = vec![DEVICE_NOTIFICATION_REQUEST];
    m.extend_from_slice(&delay.to_le_bytes());
    m
}

pub fn motor_set_speed(mask: u8, speed: i8) -> Vec<u8> {
    vec![MOTOR_SET_SPEED, mask, speed.clamp(-100, 100) as u8]
}

pub fn motor_run(mask: u8, direction: Direction) -> Vec<u8> {
    vec![MOTOR_RUN, mask, direction_byte(direction)]
}

pub fn motor_run_for_time(mask: u8, milliseconds: u32, direction: Direction) -> Vec<u8> {
    let mut m = vec![MOTOR_RUN_FOR_TIME, mask];
    m.extend_from_slice(&milliseconds.to_le_bytes());
    m.push(direction_byte(direction));
    m
}

pub fn motor_run_for_degrees(mask: u8, degrees: i32, direction: Direction) -> Vec<u8> {
    let mut m = vec![MOTOR_RUN_FOR_DEGREES, mask];
    m.extend_from_slice(&degrees.to_le_bytes());
    m.push(direction_byte(direction));
    m
}

/// Set the motor's relative position counter (degrees) to `position`.
pub fn motor_reset_relative_position(mask: u8, position: i32) -> Vec<u8> {
    let mut m = vec![MOTOR_RESET_RELATIVE_POSITION, mask];
    m.extend_from_slice(&position.to_le_bytes());
    m
}

pub fn motor_stop(mask: u8) -> Vec<u8> {
    vec![MOTOR_STOP, mask]
}

// ---- responses ------------------------------------------------------------

/// Little-endian reader that fails instead of panicking on short input.
struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn u8(&mut self) -> Option<u8> {
        let (&b, rest) = self.0.split_first()?;
        self.0 = rest;
        Some(b)
    }
    fn i8(&mut self) -> Option<i8> {
        self.u8().map(|b| b as i8)
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes([self.u8()?, self.u8()?]))
    }
    fn i16(&mut self) -> Option<i16> {
        self.u16().map(|v| v as i16)
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes([self.u8()?, self.u8()?, self.u8()?, self.u8()?]))
    }
    fn skip(&mut self, n: usize) -> Option<()> {
        self.0 = self.0.get(n..)?;
        Some(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Info {
    pub rpc: (u8, u8, u16),
    pub firmware: (u8, u8, u16),
    pub max_packet_size: u16,
    pub product: u16,
}

impl Info {
    pub fn rpc_compatible(&self) -> bool {
        SUPPORTED_RPC.contains(&(self.rpc.0, self.rpc.1))
    }
}

/// Parse a full `InfoResponse` message (including the id byte).
pub fn parse_info(message: &[u8]) -> Option<Info> {
    let mut r = Reader(message);
    if r.u8()? != INFO_RESPONSE {
        return None;
    }
    let rpc = (r.u8()?, r.u8()?, r.u16()?);
    let firmware = (r.u8()?, r.u8()?, r.u16()?);
    r.skip(4)?; // bootloader version
    Some(Info { rpc, firmware, max_packet_size: r.u16()?, product: r.u16()? })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    Completed,
    Interrupted,
    Refused,
}

/// Status byte of a result message. Motor results carry the mask first.
pub fn parse_status(message: &[u8]) -> Option<CommandStatus> {
    let id = *message.first()?;
    let status = if is_motor_result(id) { *message.get(2)? } else { *message.get(1)? };
    Some(match status {
        0 => CommandStatus::Completed,
        1 => CommandStatus::Interrupted,
        _ => CommandStatus::Refused,
    })
}

// ---- notifications ----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notification {
    Info { battery: u8, usb_power: bool },
    Imu { yaw: i16, pitch: i16, roll: i16 },
    Card { color: i8, serial: u16 },
    Button { pressed: bool },
    /// `position`: relative degrees (counts whole turns); `angle`: 0–359°.
    Motor { mask: u8, state: u8, angle: u16, position: i32, speed: i8 },
    Color { color: i8, reflection: u8 },
    Controller { left: i8, right: i8 },
    Gesture(i8),
}

/// Parse a full `DeviceNotification` message (including the id byte) into
/// its items. Stops at the first unknown or truncated item.
pub fn parse_notification(message: &[u8]) -> Vec<Notification> {
    let mut out = Vec::new();
    let mut r = Reader(message);
    if r.u8() != Some(DEVICE_NOTIFICATION) {
        return out;
    }
    let Some(len) = r.u16() else { return out };
    let data = &r.0[..(len as usize).min(r.0.len())];
    let mut r = Reader(data);
    while let Some(kind) = r.u8() {
        let item = (|| {
            Some(match kind {
                0 => Notification::Info { battery: r.u8()?, usb_power: r.u8()? == 1 },
                1 => {
                    r.skip(2)?; // orientation, yaw face
                    let (yaw, pitch, roll) = (r.i16()?, r.i16()?, r.i16()?);
                    r.skip(12)?; // accelerometer, gyroscope
                    Notification::Imu { yaw, pitch, roll }
                }
                3 => Notification::Card { color: r.i8()?, serial: r.u16()? },
                4 => Notification::Button { pressed: r.u8()? == 1 },
                10 => {
                    let (mask, state, angle) = (r.u8()?, r.u8()?, r.u16()?);
                    r.skip(2)?; // power
                    let speed = r.i8()?;
                    let position = r.i32()?;
                    r.skip(1)?; // gesture
                    Notification::Motor { mask, state, angle, position, speed }
                }
                12 => {
                    let (color, reflection) = (r.i8()?, r.u8()?);
                    r.skip(10)?; // raw RGB, HSV
                    Notification::Color { color, reflection }
                }
                15 => {
                    let (left, right) = (r.i8()?, r.i8()?);
                    r.skip(4)?; // lever angles
                    Notification::Controller { left, right }
                }
                16 => Notification::Gesture(r.i8()?),
                _ => return None,
            })
        })();
        match item {
            Some(n) => out.push(n),
            None => break,
        }
    }
    out
}

/// Firmware colour number → colours the Colour Sensor can detect.
pub fn sensor_color(fw: i8) -> LegoColor {
    match fw {
        2 => LegoColor::Purple,
        3 => LegoColor::Blue,
        5 => LegoColor::Teal,
        6 => LegoColor::Green,
        7 => LegoColor::Yellow,
        9 => LegoColor::Red,
        10 => LegoColor::White,
        _ => LegoColor::None,
    }
}

/// Name of a connection card colour (firmware numbering). LEGO calls the
/// pink card "magenta".
pub fn card_color_name(fw: i8) -> &'static str {
    match fw {
        1 => "pink",
        2 => "purple",
        3 => "blue",
        4 => "azure",
        5 => "turquoise",
        6 => "green",
        7 => "yellow",
        8 => "orange",
        9 => "red",
        10 => "white",
        _ => "no card",
    }
}

// ---- advertising --------------------------------------------------------------

/// What a device tells us in its advertisement (LEGO manufacturer data).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Advert {
    pub kind: DeviceKind,
    pub card_color: i8,
    pub card_serial: u16,
}

/// e.g. "blue 3683", used to tell devices in a classroom apart.
pub fn card_label(color: i8, serial: u16) -> String {
    if serial == 0 && color <= 0 {
        return "no card".into();
    }
    format!("{} {:04}", card_color_name(color), serial)
}

impl Advert {
    pub fn card(&self) -> String {
        card_label(self.card_color, self.card_serial)
    }
}

pub fn product_kind(product: u16) -> Option<DeviceKind> {
    match product {
        512 => Some(DeviceKind::SingleMotor),
        513 => Some(DeviceKind::DoubleMotor),
        514 => Some(DeviceKind::ColorSensor),
        515 => Some(DeviceKind::Controller),
        _ => None,
    }
}

/// Parse manufacturer data (company id already stripped by the BLE stack).
pub fn parse_advert(data: &[u8]) -> Option<Advert> {
    let mut r = Reader(data);
    let product = (r.u8()? as u16) << 8 | r.u8()? as u16;
    Some(Advert { kind: product_kind(product)?, card_color: r.i8()?, card_serial: r.u16()? })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_commands_like_the_official_package() {
        // struct.pack("<BBB", 140, 1, 50)
        assert_eq!(motor_set_speed(1, 50), [140, 1, 50]);
        assert_eq!(motor_set_speed(3, -100), [140, 3, 0x9c]);
        assert_eq!(motor_run(2, Direction::CounterClockwise), [122, 2, 1]);
        // "<BBLB": 2000 ms = d0 07 00 00
        assert_eq!(motor_run_for_time(1, 2000, Direction::Clockwise), [126, 1, 0xd0, 0x07, 0, 0, 0]);
        assert_eq!(motor_run_for_degrees(3, -360, Direction::Clockwise), [124, 3, 0x98, 0xfe, 0xff, 0xff, 0]);
        assert_eq!(motor_stop(3), [138, 3]);
        assert_eq!(motor_reset_relative_position(3, 0), [120, 3, 0, 0, 0, 0]);
        assert_eq!(device_notification_request(50), [40, 50, 0]);
        assert_eq!(device_notification_request(5), [40, 15, 0], "clamped to the minimum");
        assert_eq!(motor_mask(Motor::DoubleRight), 2);
    }

    #[test]
    fn parses_info_response() {
        let msg = [1, 2, 0, 5, 0, 1, 3, 7, 0, 1, 0, 2, 0, 0xf4, 0, 0x00, 0x02];
        let info = parse_info(&msg).unwrap();
        assert_eq!(info.rpc, (2, 0, 5));
        assert_eq!(info.firmware, (1, 3, 7));
        assert_eq!(info.max_packet_size, 244);
        assert_eq!(product_kind(info.product), Some(DeviceKind::SingleMotor));
        assert!(info.rpc_compatible());
        let mut old = info;
        old.rpc = (1, 0, 73); // firmware seen on a real Double Motor
        assert!(old.rpc_compatible());
        old.rpc = (3, 0, 0);
        assert!(!old.rpc_compatible());
        assert!(parse_info(&msg[..10]).is_none(), "truncated");
    }

    #[test]
    fn matches_motor_results_per_motor() {
        assert_eq!(response_key(&[127, 2, 0]), Some((127, Some(2))));
        assert_eq!(response_key(&[41, 0]), Some((41, None)));
        assert_eq!(parse_status(&[127, 2, 1]), Some(CommandStatus::Interrupted));
        assert_eq!(parse_status(&[41, 0]), Some(CommandStatus::Completed));
    }

    #[test]
    fn parses_device_notifications() {
        let mut items = vec![0u8, 87, 0]; // battery 87%
        items.extend([4, 1]); // button pressed
        items.extend([10, 1, 1, 0x10, 0, 0x20, 0, 50, 0x68, 0x01, 0, 0, 0xff]); // motor pos 360
        items.extend([12, 9, 42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // red, reflection 42
        items.extend([15, 0x9c, 60, 0, 0, 0, 0]); // levers -100 / 60
        let mut msg = vec![DEVICE_NOTIFICATION];
        msg.extend((items.len() as u16).to_le_bytes());
        msg.extend(&items);
        assert_eq!(
            parse_notification(&msg),
            vec![
                Notification::Info { battery: 87, usb_power: false },
                Notification::Button { pressed: true },
                Notification::Motor { mask: 1, state: 1, angle: 16, position: 360, speed: 50 },
                Notification::Color { color: 9, reflection: 42 },
                Notification::Controller { left: -100, right: 60 },
            ]
        );
        assert_eq!(sensor_color(9), LegoColor::Red);
        // Truncated item: parse what's complete, then stop.
        let cut = &msg[..msg.len() - 3];
        assert_eq!(parse_notification(cut).len(), 4);
    }

    #[test]
    fn parses_advertisements() {
        let ad = parse_advert(&[2, 1, 3, 0x63, 0x0e, 0]).unwrap();
        assert_eq!(ad.kind, DeviceKind::DoubleMotor);
        assert_eq!(ad.card(), "blue 3683");
        assert!(parse_advert(&[1, 0, 3, 0, 0]).is_none(), "not a CS & AI device");
        // Real Double Motor with no card scanned yet.
        assert_eq!(parse_advert(&[2, 1, 0, 0, 0]).unwrap().card(), "no card");
    }

    #[test]
    fn card_scanned_while_connected() {
        // Pink (firmware "magenta" = 1) card number 0042.
        let msg = [DEVICE_NOTIFICATION, 4, 0, 3, 1, 42, 0];
        assert_eq!(parse_notification(&msg), vec![Notification::Card { color: 1, serial: 42 }]);
        assert_eq!(card_label(1, 42), "pink 0042");
    }
}
