//! Sensor conditions used by If, If/Else and Wait Until.
//!
//! Only conditions the Computer Science & AI hardware can actually measure
//! (see docs/lego-hardware.md), plus the computer's camera.

use serde::{Deserialize, Serialize};

use crate::camera::body::{MAX_DISTANCE, MAX_POSITION};
use crate::camera::{Axis, BodyPoint, CameraPose, Detection};
use crate::hardware::{CardGroup, DeviceKind, DeviceTarget, LegoColor, Lever, Motor, SensorQuery, SensorValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Comparison {
    Less,
    Greater,
}

impl Comparison {
    pub fn symbol(self) -> &'static str {
        match self {
            Comparison::Less => "<",
            Comparison::Greater => ">",
        }
    }

    pub fn test(self, value: i64, threshold: i64) -> bool {
        match self {
            Comparison::Less => value < threshold,
            Comparison::Greater => value > threshold,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "sensor", rename_all = "snake_case")]
pub enum Condition {
    /// Colour Sensor sees a colour.
    ColorIs {
        color: LegoColor,
        #[serde(default)]
        group: CardGroup,
    },
    /// Colour Sensor reflection in percent.
    Reflection {
        cmp: Comparison,
        percent: i32,
        #[serde(default)]
        group: CardGroup,
    },
    /// The button on a device is pressed.
    ButtonPressed {
        device: DeviceKind,
        #[serde(default)]
        group: CardGroup,
    },
    /// Relative motor position in degrees.
    MotorPosition {
        motor: Motor,
        cmp: Comparison,
        degrees: i32,
        #[serde(default)]
        group: CardGroup,
    },
    /// Controller lever position in percent (−100…100).
    LeverPosition {
        lever: Lever,
        cmp: Comparison,
        percent: i32,
        #[serde(default)]
        group: CardGroup,
    },
    /// The computer's camera sees a person or a pose.
    Camera {
        pose: CameraPose,
    },
    /// Two body points touch (camera).
    BodyTouching {
        one: BodyPoint,
        two: BodyPoint,
    },
    /// Distance between two body points (camera), in Coding Canvas units.
    BodyDistance {
        one: BodyPoint,
        two: BodyPoint,
        cmp: Comparison,
        distance: i32,
    },
    /// Direction from one body point to another (camera), 0–359°.
    BodyAngle {
        one: BodyPoint,
        two: BodyPoint,
        cmp: Comparison,
        degrees: i32,
    },
    /// Where a body point is in the picture (camera).
    BodyPosition {
        axis: Axis,
        point: BodyPoint,
        cmp: Comparison,
        value: i32,
    },
}

/// The kinds of condition, for the sensor picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionKind {
    Color,
    Reflection,
    Button,
    MotorPosition,
    Lever,
    Camera,
    BodyTouching,
    BodyDistance,
    BodyAngle,
    BodyPosition,
}

impl ConditionKind {
    pub const ALL: [ConditionKind; 10] = [
        ConditionKind::Color,
        ConditionKind::Reflection,
        ConditionKind::Button,
        ConditionKind::MotorPosition,
        ConditionKind::Lever,
        ConditionKind::Camera,
        ConditionKind::BodyTouching,
        ConditionKind::BodyDistance,
        ConditionKind::BodyAngle,
        ConditionKind::BodyPosition,
    ];

    /// Read from the computer's camera rather than a LEGO device.
    pub fn is_camera(self) -> bool {
        matches!(
            self,
            ConditionKind::Camera
                | ConditionKind::BodyTouching
                | ConditionKind::BodyDistance
                | ConditionKind::BodyAngle
                | ConditionKind::BodyPosition
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            ConditionKind::Color => "Colour is",
            ConditionKind::Reflection => "Reflection",
            ConditionKind::Button => "Button pressed",
            ConditionKind::MotorPosition => "Motor position",
            ConditionKind::Lever => "Controller lever",
            ConditionKind::Camera => "Camera sees",
            ConditionKind::BodyTouching => "Body points touch",
            ConditionKind::BodyDistance => "Body distance",
            ConditionKind::BodyAngle => "Body angle",
            ConditionKind::BodyPosition => "Body position",
        }
    }

    pub fn default_condition(self) -> Condition {
        match self {
            ConditionKind::Color => Condition::ColorIs {
                color: LegoColor::Red,
                group: CardGroup::NoCard,
            },
            ConditionKind::Reflection => Condition::Reflection {
                cmp: Comparison::Greater,
                percent: 50,
                group: CardGroup::NoCard,
            },
            ConditionKind::Button => Condition::ButtonPressed {
                device: DeviceKind::Controller,
                group: CardGroup::NoCard,
            },
            ConditionKind::MotorPosition => Condition::MotorPosition {
                motor: Motor::Single,
                cmp: Comparison::Greater,
                degrees: 90,
                group: CardGroup::NoCard,
            },
            ConditionKind::Lever => Condition::LeverPosition {
                lever: Lever::Left,
                cmp: Comparison::Greater,
                percent: 50,
                group: CardGroup::NoCard,
            },
            ConditionKind::Camera => Condition::Camera { pose: CameraPose::Person },
            ConditionKind::BodyTouching => Condition::BodyTouching { one: BodyPoint::LeftWrist, two: BodyPoint::RightWrist },
            ConditionKind::BodyDistance => Condition::BodyDistance {
                one: BodyPoint::LeftWrist,
                two: BodyPoint::RightWrist,
                cmp: Comparison::Greater,
                distance: 50,
            },
            ConditionKind::BodyAngle => Condition::BodyAngle {
                one: BodyPoint::LeftShoulder,
                two: BodyPoint::RightShoulder,
                cmp: Comparison::Greater,
                degrees: 20,
            },
            ConditionKind::BodyPosition => Condition::BodyPosition {
                axis: Axis::X,
                point: BodyPoint::Nose,
                cmp: Comparison::Greater,
                value: 0,
            },
        }
    }
}

pub const MAX_DEGREES: i32 = 1_000_000;

impl Default for Condition {
    fn default() -> Self {
        ConditionKind::Color.default_condition()
    }
}

impl Condition {
    pub fn kind(&self) -> ConditionKind {
        match self {
            Condition::ColorIs { .. } => ConditionKind::Color,
            Condition::Reflection { .. } => ConditionKind::Reflection,
            Condition::ButtonPressed { .. } => ConditionKind::Button,
            Condition::MotorPosition { .. } => ConditionKind::MotorPosition,
            Condition::LeverPosition { .. } => ConditionKind::Lever,
            Condition::Camera { .. } => ConditionKind::Camera,
            Condition::BodyTouching { .. } => ConditionKind::BodyTouching,
            Condition::BodyDistance { .. } => ConditionKind::BodyDistance,
            Condition::BodyAngle { .. } => ConditionKind::BodyAngle,
            Condition::BodyPosition { .. } => ConditionKind::BodyPosition,
        }
    }

    /// Read from the computer's camera rather than a LEGO device.
    pub fn is_camera(&self) -> bool {
        self.kind().is_camera()
    }

    /// Evaluate a camera condition against what the camera sees.
    pub fn evaluate_camera(&self, seen: &Detection) -> bool {
        let pose = seen.pose.as_ref();
        let test = |value: Option<f32>, cmp: Comparison, threshold: i32| {
            value.is_some_and(|v| cmp.test(v.floor() as i64, threshold as i64))
        };
        match *self {
            Condition::Camera { pose } => seen.matches(pose),
            Condition::BodyTouching { one, two } => pose.is_some_and(|p| p.touching(one, two)),
            Condition::BodyDistance { one, two, cmp, distance } => test(pose.and_then(|p| p.distance(one, two)), cmp, distance),
            Condition::BodyAngle { one, two, cmp, degrees } => test(pose.and_then(|p| p.angle(one, two)), cmp, degrees),
            Condition::BodyPosition { axis, point, cmp, value } => test(pose.and_then(|p| p.coordinate(axis, point)), cmp, value),
            _ => false,
        }
    }

    /// The LEGO device that has to be connected to evaluate this condition
    /// (none for the camera).
    pub fn device(&self) -> Option<DeviceKind> {
        match self {
            Condition::ColorIs { .. } | Condition::Reflection { .. } => Some(DeviceKind::ColorSensor),
            Condition::ButtonPressed { device, .. } => Some(*device),
            Condition::MotorPosition { motor, .. } => Some(motor.device()),
            Condition::LeverPosition { .. } => Some(DeviceKind::Controller),
            _ => None,
        }
    }

    /// The card group whose devices this condition reads.
    pub fn group(&self) -> Option<CardGroup> {
        match *self {
            Condition::ColorIs { group, .. }
            | Condition::Reflection { group, .. }
            | Condition::ButtonPressed { group, .. }
            | Condition::MotorPosition { group, .. }
            | Condition::LeverPosition { group, .. } => Some(group),
            _ => None,
        }
    }

    pub fn group_mut(&mut self) -> Option<&mut CardGroup> {
        match self {
            Condition::ColorIs { group, .. }
            | Condition::Reflection { group, .. }
            | Condition::ButtonPressed { group, .. }
            | Condition::MotorPosition { group, .. }
            | Condition::LeverPosition { group, .. } => Some(group),
            _ => None,
        }
    }

    pub fn target(&self) -> Option<DeviceTarget> {
        Some(DeviceTarget { kind: self.device()?, group: self.group()? })
    }

    /// What to ask the LEGO devices (none for the camera).
    pub fn query(&self) -> Option<SensorQuery> {
        Some(match *self {
            Condition::ColorIs { .. } => SensorQuery::Color,
            Condition::Reflection { .. } => SensorQuery::Reflection,
            Condition::ButtonPressed { device, .. } => SensorQuery::Button(device),
            Condition::MotorPosition { motor, .. } => SensorQuery::MotorPosition(motor),
            Condition::LeverPosition { lever, .. } => SensorQuery::ControllerLever(lever),
            _ => return None,
        })
    }

    /// Evaluate against a sensor reading. A reading of the wrong type is false.
    pub fn evaluate(&self, value: SensorValue) -> bool {
        match (*self, value) {
            (Condition::ColorIs { color, .. }, SensorValue::Color(seen)) => color == seen,
            (Condition::Reflection { cmp, percent, .. }, SensorValue::Percent(v))
            | (Condition::LeverPosition { cmp, percent, .. }, SensorValue::Percent(v)) => {
                cmp.test(v as i64, percent as i64)
            }
            (Condition::ButtonPressed { .. }, SensorValue::Pressed(p)) => p,
            (Condition::MotorPosition { cmp, degrees, .. }, SensorValue::Degrees(d)) => {
                cmp.test(d, degrees as i64)
            }
            _ => false,
        }
    }

    /// A card-colour group may contain several matching devices. Conditions
    /// succeed when any device's latest reading satisfies them.
    pub fn evaluate_any(&self, values: impl IntoIterator<Item = SensorValue>) -> bool {
        values.into_iter().any(|value| self.evaluate(value))
    }

    pub fn sanitize(&mut self) {
        match self {
            Condition::Reflection { percent, .. } => *percent = (*percent).clamp(0, 100),
            Condition::LeverPosition { percent, .. } => *percent = (*percent).clamp(-100, 100),
            Condition::MotorPosition { motor, degrees, .. } => {
                // "Both" has no single position; read the left side.
                if *motor == Motor::DoubleBoth {
                    *motor = Motor::DoubleLeft;
                }
                *degrees = (*degrees).clamp(-MAX_DEGREES, MAX_DEGREES);
            }
            Condition::Camera { pose } => pose.sanitize(),
            Condition::BodyDistance { distance, .. } => *distance = (*distance).clamp(0, MAX_DISTANCE),
            Condition::BodyAngle { degrees, .. } => *degrees = (*degrees).clamp(0, 359),
            Condition::BodyPosition { value, .. } => *value = (*value).clamp(-MAX_POSITION, MAX_POSITION),
            Condition::ColorIs { .. } | Condition::ButtonPressed { .. } | Condition::BodyTouching { .. } => {}
        }
    }

    /// Short text, e.g. for palette chips and ghosts. `poses` names the
    /// project's trained camera poses.
    pub fn describe(&self, poses: &[String]) -> String {
        match self {
            Condition::ColorIs { color, .. } => format!("colour is {}", color.label()),
            Condition::Reflection { cmp, percent, .. } => format!("reflection {} {percent}%", cmp.symbol()),
            Condition::ButtonPressed { device, .. } => format!("{} button pressed", device.label()),
            Condition::MotorPosition { motor, cmp, degrees, .. } => {
                format!("{} position {} {degrees}°", motor.label(), cmp.symbol())
            }
            Condition::LeverPosition { lever, cmp, percent, .. } => {
                format!("{} lever {} {percent}%", lever.label(), cmp.symbol())
            }
            Condition::Camera { pose } => format!("camera sees {}", pose.label(poses)),
            Condition::BodyTouching { one, two } => format!("{} touches {}", one.label(), two.label()),
            Condition::BodyDistance { one, two, cmp, distance } => {
                format!("distance {}–{} {} {distance}", one.label(), two.label(), cmp.symbol())
            }
            Condition::BodyAngle { one, two, cmp, degrees } => {
                format!("angle {}→{} {} {degrees}°", one.label(), two.label(), cmp.symbol())
            }
            Condition::BodyPosition { axis, point, cmp, value } => {
                format!("{} {} {} {value}", point.label(), axis.label(), cmp.symbol())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_readings() {
        let red = Condition::ColorIs { color: LegoColor::Red, group: CardGroup::NoCard };
        assert!(red.evaluate(SensorValue::Color(LegoColor::Red)));
        assert!(!red.evaluate(SensorValue::Color(LegoColor::Blue)));
        assert!(!red.evaluate(SensorValue::Percent(5)), "wrong reading type is false");

        let pos = Condition::MotorPosition {
            motor: Motor::Single,
            cmp: Comparison::Greater,
            degrees: 90,
            group: CardGroup::NoCard,
        };
        assert!(pos.evaluate(SensorValue::Degrees(91)));
        assert!(!pos.evaluate(SensorValue::Degrees(90)));
    }

    #[test]
    fn group_condition_matches_any_device_reading() {
        let red = Condition::ColorIs { color: LegoColor::Red, group: CardGroup::Blue };
        assert!(red.evaluate_any([
            SensorValue::Color(LegoColor::Blue),
            SensorValue::Color(LegoColor::Red)
        ]));
        assert!(!red.evaluate_any([
            SensorValue::Color(LegoColor::Blue),
            SensorValue::Color(LegoColor::Yellow)
        ]));
    }

    #[test]
    fn camera_conditions_load_and_need_no_device() {
        let custom: Condition = serde_json::from_str(r#"{"sensor":"camera","pose":{"Custom":99}}"#).unwrap();
        let mut custom = custom;
        custom.sanitize();
        assert_eq!(custom, Condition::Camera { pose: CameraPose::Custom(7) });
        let arms: Condition = serde_json::from_str(r#"{"sensor":"camera","pose":"LeftArmUp"}"#).unwrap();
        assert_eq!(arms.target(), None);
        assert_eq!(arms.query(), None);
        assert_eq!(arms.describe(&[]), "camera sees left arm up");
        assert_eq!(Condition::Camera { pose: CameraPose::Custom(0) }.describe(&["wave".into()]), "camera sees “wave”");
    }

    #[test]
    fn body_sensing_conditions() {
        let mut pose = crate::camera::pose::tests::standing(false, false);
        let seen = |pose: &crate::camera::Pose| Detection::from_pose(Some(pose), &[]);
        let touch = ConditionKind::BodyTouching.default_condition();
        assert!(touch.is_camera() && touch.target().is_none());
        assert!(!touch.evaluate_camera(&seen(&pose)));
        pose.points[BodyPoint::LeftWrist.index()].x = 0.51;
        pose.points[BodyPoint::RightWrist.index()].x = 0.49;
        assert!(touch.evaluate_camera(&seen(&pose)), "hands together");
        assert!(!touch.evaluate_camera(&Detection::default()), "nobody there");

        let apart = Condition::BodyDistance { one: BodyPoint::LeftWrist, two: BodyPoint::RightWrist, cmp: Comparison::Less, distance: 5 };
        assert!(apart.evaluate_camera(&seen(&pose)));
        let nose_up = Condition::BodyPosition { axis: Axis::Y, point: BodyPoint::Nose, cmp: Comparison::Greater, value: 20 };
        assert!(nose_up.evaluate_camera(&seen(&pose)));
        let level = Condition::BodyAngle { one: BodyPoint::LeftShoulder, two: BodyPoint::RightShoulder, cmp: Comparison::Less, degrees: 5 };
        assert!(level.evaluate_camera(&seen(&pose)));

        let json = r#"{"sensor":"body_angle","one":"left_shoulder","two":"right_wrist","cmp":"Greater","degrees":999}"#;
        let mut c: Condition = serde_json::from_str(json).unwrap();
        c.sanitize();
        assert_eq!(c, Condition::BodyAngle { one: BodyPoint::LeftShoulder, two: BodyPoint::RightWrist, cmp: Comparison::Greater, degrees: 359 });
    }

    #[test]
    fn sanitize_clamps() {
        let mut c = Condition::LeverPosition {
            lever: Lever::Left,
            cmp: Comparison::Less,
            percent: 999,
            group: CardGroup::NoCard,
        };
        c.sanitize();
        assert_eq!(
            c,
            Condition::LeverPosition {
                lever: Lever::Left,
                cmp: Comparison::Less,
                percent: 100,
                group: CardGroup::NoCard,
            }
        );
    }
}
