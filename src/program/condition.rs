//! Sensor conditions used by If, If/Else and Wait Until.
//!
//! Only conditions the Computer Science & AI hardware can actually measure
//! (see docs/lego-hardware.md).

use serde::{Deserialize, Serialize};

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
}

/// The kinds of condition, for the sensor picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionKind {
    Color,
    Reflection,
    Button,
    MotorPosition,
    Lever,
}

impl ConditionKind {
    pub const ALL: [ConditionKind; 5] = [
        ConditionKind::Color,
        ConditionKind::Reflection,
        ConditionKind::Button,
        ConditionKind::MotorPosition,
        ConditionKind::Lever,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ConditionKind::Color => "Colour is",
            ConditionKind::Reflection => "Reflection",
            ConditionKind::Button => "Button pressed",
            ConditionKind::MotorPosition => "Motor position",
            ConditionKind::Lever => "Controller lever",
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
        }
    }

    /// The device that has to be connected to evaluate this condition.
    pub fn device(&self) -> DeviceKind {
        match self {
            Condition::ColorIs { .. } | Condition::Reflection { .. } => DeviceKind::ColorSensor,
            Condition::ButtonPressed { device, .. } => *device,
            Condition::MotorPosition { motor, .. } => motor.device(),
            Condition::LeverPosition { .. } => DeviceKind::Controller,
        }
    }

    pub fn target(&self) -> DeviceTarget {
        let group = match *self {
            Condition::ColorIs { group, .. }
            | Condition::Reflection { group, .. }
            | Condition::ButtonPressed { group, .. }
            | Condition::MotorPosition { group, .. }
            | Condition::LeverPosition { group, .. } => group,
        };
        DeviceTarget { kind: self.device(), group }
    }

    pub fn query(&self) -> SensorQuery {
        match *self {
            Condition::ColorIs { .. } => SensorQuery::Color,
            Condition::Reflection { .. } => SensorQuery::Reflection,
            Condition::ButtonPressed { device, .. } => SensorQuery::Button(device),
            Condition::MotorPosition { motor, .. } => SensorQuery::MotorPosition(motor),
            Condition::LeverPosition { lever, .. } => SensorQuery::ControllerLever(lever),
        }
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
            Condition::ColorIs { .. } | Condition::ButtonPressed { .. } => {}
        }
    }

    /// Short text, e.g. for palette chips and ghosts.
    pub fn describe(&self) -> String {
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
