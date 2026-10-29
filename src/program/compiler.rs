//! Visual program → executable [`Program`].

use std::collections::{BTreeMap, BTreeSet};

use crate::camera::CameraPose;
use crate::program::Condition;

use crate::hardware::{CardGroup, DeviceTarget, Speed, Tone};
use crate::visual::{Block, BlockId, BlockKind, DriveMode, MAX_BEEP_COUNT, MAX_ROTATIONS, MAX_SECONDS, Workspace};

use super::{Command, Program, Script, Step, tune};

#[derive(Debug, Clone, PartialEq)]
pub struct CompileError {
    /// Block the problem belongs to, if any (shown highlighted in the UI).
    pub block: Option<BlockId>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Compiled {
    pub program: Program,
    /// Devices the program needs, so the app can check they are connected.
    pub devices: BTreeSet<DeviceTarget>,
    /// Card groups needed by commands that operate on any device kind.
    pub groups: BTreeSet<CardGroup>,
    /// The program reads the computer's camera.
    pub uses_camera: bool,
    /// Trained camera poses the program looks for, with a block using each.
    pub custom_poses: BTreeMap<u8, BlockId>,
    /// The program plays sound on the computer.
    pub uses_speaker: bool,
}

fn seconds_to_ms(seconds: f32) -> u64 {
    (seconds.clamp(0.0, MAX_SECONDS) as f64 * 1000.0).round() as u64
}

fn percent(value: i32) -> u8 {
    value.clamp(0, 100) as u8
}

fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

struct Compiler {
    errors: Vec<CompileError>,
    devices: BTreeSet<DeviceTarget>,
    groups: BTreeSet<CardGroup>,
    uses_camera: bool,
    custom_poses: BTreeMap<u8, BlockId>,
    uses_speaker: bool,
}

impl Compiler {
    fn error(&mut self, block: &Block, message: &str) {
        self.errors.push(CompileError { block: Some(block.id), message: message.into() });
    }

    fn seq(&mut self, blocks: &[Block]) -> Vec<Step> {
        blocks
            .iter()
            .filter_map(|b| self.block(b).map(|command| Step { source: b.id, command }))
            .collect()
    }

    fn block(&mut self, block: &Block) -> Option<Command> {
        let command = match block.kind.clone() {
            BlockKind::WhenRun => {
                self.error(block, "“When Run” can only be at the top of a stack.");
                return None;
            }
            BlockKind::RunMotor { motor, group, direction, mode, speed } => match mode {
                DriveMode::Speed => Command::MotorRun { motor, group, direction, speed: Speed::new(speed as i64) },
                DriveMode::Power => Command::MotorRunPower { motor, group, direction, power: percent(speed) },
            },
            BlockKind::StopMotor { motor, group } => Command::MotorStop { motor, group },
            BlockKind::SetAcceleration { motor, group, acceleration, deceleration } => Command::MotorSetAcceleration {
                motor,
                group,
                acceleration: percent(acceleration),
                deceleration: percent(deceleration),
            },
            BlockKind::ResetPosition { motor, group } => Command::MotorResetPosition { motor, group },
            BlockKind::MotorForTime { motor, group, direction, mode, speed, seconds } => {
                if speed <= 0 {
                    self.error(block, &format!("{} must be more than 0%.", capitalised(mode.label())));
                    return None;
                }
                let milliseconds = seconds_to_ms(seconds);
                match mode {
                    DriveMode::Speed => Command::MotorRunForTime {
                        motor,
                        group,
                        direction,
                        speed: Speed::new(speed as i64),
                        milliseconds,
                    },
                    DriveMode::Power => {
                        Command::MotorRunPowerForTime { motor, group, direction, power: percent(speed), milliseconds }
                    }
                }
            }
            BlockKind::MotorRotations { motor, group, direction, speed, rotations } => {
                if speed <= 0 {
                    self.error(block, "Speed must be more than 0%.");
                    return None;
                }
                let degrees = (rotations.clamp(0.0, MAX_ROTATIONS) * 360.0).round() as u32;
                Command::MotorRunForDegrees {
                    motor,
                    group,
                    direction,
                    speed: Speed::new(speed as i64),
                    degrees,
                }
            }
            BlockKind::Beep { group, pattern, note, count, output, volume } => Command::Beep {
                group,
                tone: Tone {
                    pattern,
                    frequency: tune::note_hz(tune::nearest_note(note)),
                    count: count.clamp(1, MAX_BEEP_COUNT) as u8,
                },
                output,
                volume: volume.min(100),
            },
            BlockKind::PlayTune { group, tune, output, volume } => {
                Command::PlayTune { group, notes: tune.notes(), output, volume: volume.min(100) }
            }
            BlockKind::Wait { seconds } => Command::Wait { milliseconds: seconds_to_ms(seconds) },
            BlockKind::WaitUntil { condition } => Command::WaitUntil { condition },
            BlockKind::Repeat { count } => Command::Repeat { count, body: self.seq(&block.body) },
            BlockKind::Forever => Command::Forever { body: self.seq(&block.body) },
            BlockKind::If { condition } => Command::If {
                condition,
                then: self.seq(&block.body),
                otherwise: Vec::new(),
            },
            BlockKind::IfElse { condition } => Command::If {
                condition,
                then: self.seq(&block.body),
                otherwise: self.seq(&block.else_body),
            },
        };
        if let Some(condition) = block.kind.condition() {
            match condition.target() {
                Some(target) => {
                    self.devices.insert(target);
                }
                None => self.uses_camera = true,
            }
            if let Condition::Camera { pose: CameraPose::Custom(i) } = condition {
                self.custom_poses.entry(*i).or_insert(block.id);
            }
        }
        if let Some(target) = motor_target(&command) {
            self.devices.insert(target);
        }
        if let Command::Beep { group, output, .. } | Command::PlayTune { group, output, .. } = &command {
            if output.devices() {
                self.groups.insert(*group);
            }
            if output.computer() {
                self.uses_speaker = true;
            }
        }
        Some(command)
    }
}

fn motor_target(command: &Command) -> Option<DeviceTarget> {
    match command {
        Command::MotorRun { motor, group, .. }
        | Command::MotorRunForTime { motor, group, .. }
        | Command::MotorRunForDegrees { motor, group, .. }
        | Command::MotorStop { motor, group }
        | Command::MotorRunPower { motor, group, .. }
        | Command::MotorRunPowerForTime { motor, group, .. }
        | Command::MotorSetAcceleration { motor, group, .. }
        | Command::MotorResetPosition { motor, group } => {
            Some(DeviceTarget { kind: motor.device(), group: *group })
        }
        _ => None,
    }
}

pub fn compile(ws: &Workspace) -> Result<Compiled, Vec<CompileError>> {
    let mut c = Compiler { errors: Vec::new(), devices: BTreeSet::new(), groups: BTreeSet::new(), uses_camera: false, custom_poses: BTreeMap::new(), uses_speaker: false };
    let mut program = Program::default();

    for stack in &ws.stacks {
        let Some(first) = stack.blocks.first() else { continue };
        if !first.kind.is_hat() {
            // Loose blocks without "When Run" simply don't run (like Scratch).
            continue;
        }
        let steps = c.seq(&stack.blocks[1..]);
        program.scripts.push(Script { hat: first.id, steps });
    }

    if program.scripts.is_empty() && c.errors.is_empty() {
        c.errors.push(CompileError {
            block: None,
            message: "Add a “When Run” block and attach blocks below it.".into(),
        });
    }

    if c.errors.is_empty() {
        Ok(Compiled { program, devices: c.devices, groups: c.groups, uses_camera: c.uses_camera,
            custom_poses: c.custom_poses,
            uses_speaker: c.uses_speaker,
        })
    } else {
        Err(c.errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::SoundOutput;
    use crate::hardware::{CardGroup, DeviceKind, Direction, Motor};

    #[test]
    fn compiles_the_starter_program() {
        let ws = Workspace::example();
        let c = compile(&ws).unwrap();
        assert_eq!(c.program.scripts.len(), 1);
        let cmds: Vec<_> = c.program.scripts[0].steps.iter().map(|s| s.command.clone()).collect();
        assert_eq!(
            cmds,
            vec![
                Command::MotorRun {
                    motor: Motor::Single,
                    group: CardGroup::NoCard,
                    direction: Direction::Clockwise,
                    speed: Speed::new(50),
                },
                Command::Wait { milliseconds: 2000 },
                Command::MotorStop { motor: Motor::Single, group: CardGroup::NoCard },
            ]
        );
        assert!(c.devices.contains(&DeviceTarget { kind: DeviceKind::SingleMotor, group: CardGroup::NoCard }));
    }

    #[test]
    fn compiles_nested_control_flow() {
        let ws = crate::visual::tests::colour_program();
        let c = compile(&ws).unwrap();
        let Command::Forever { body } = &c.program.scripts[0].steps[0].command else { panic!() };
        let Command::If { condition, then, otherwise } = &body[0].command else { panic!() };
        assert_eq!(*condition, Condition::ColorIs { color: crate::hardware::LegoColor::Red, group: CardGroup::NoCard });
        assert_eq!(then.len(), 1);
        assert_eq!(otherwise.len(), 1);
        assert!(c.devices.contains(&DeviceTarget { kind: DeviceKind::ColorSensor, group: CardGroup::NoCard }));
        assert!(c.devices.contains(&DeviceTarget { kind: DeviceKind::SingleMotor, group: CardGroup::NoCard }));
    }

    #[test]
    fn compiles_selected_card_group_for_motor_targets() {
        let mut ws = Workspace::new();
        let hat = ws.make_block(BlockKind::WhenRun);
        let run = ws.make_block(BlockKind::RunMotor {
            motor: Motor::Single,
            group: CardGroup::Blue,
            direction: Direction::Clockwise,
            mode: DriveMode::Speed,
            speed: 50,
        });
        ws.add_stack([0.0, 0.0], vec![hat, run]);

        let compiled = compile(&ws).unwrap();
        assert!(compiled.devices.contains(&DeviceTarget { kind: DeviceKind::SingleMotor, group: CardGroup::Blue }));
    }

    #[test]
    fn sensor_condition_requires_its_selected_card_group() {
        let mut ws = Workspace::new();
        let hat = ws.make_block(BlockKind::WhenRun);
        let wait = ws.make_block(BlockKind::WaitUntil {
            condition: Condition::ColorIs { color: crate::hardware::LegoColor::Red, group: CardGroup::Blue },
        });
        ws.add_stack([0.0, 0.0], vec![hat, wait]);

        let compiled = compile(&ws).unwrap();
        assert!(compiled.devices.contains(&DeviceTarget { kind: DeviceKind::ColorSensor, group: CardGroup::Blue }));
    }

    #[test]
    fn camera_conditions_need_the_camera_not_a_device() {
        let mut ws = Workspace::new();
        let hat = ws.make_block(BlockKind::WhenRun);
        let wait = ws.make_block(BlockKind::WaitUntil {
            condition: Condition::Camera { pose: crate::camera::CameraPose::BothArmsUp },
        });
        ws.add_stack([0.0, 0.0], vec![hat, wait]);

        let compiled = compile(&ws).unwrap();
        assert!(compiled.uses_camera);
        assert!(compiled.devices.is_empty());
        assert!(!compile(&Workspace::example()).unwrap().uses_camera);
    }

    #[test]
    fn beep_requires_a_device_in_its_card_group() {
        let mut ws = Workspace::new();
        let hat = ws.make_block(BlockKind::WhenRun);
        let beep = ws.make_block(BlockKind::Beep {
            group: CardGroup::Blue,
            pattern: crate::hardware::BeepPattern::Double,
            note: 72,
            count: 2,
            output: SoundOutput::Devices,
            volume: 100,
        });
        let tune = ws.make_block(BlockKind::PlayTune {
            group: CardGroup::Red,
            tune: tune::Tune::TaDa,
            output: SoundOutput::Both,
            volume: 300,
        });
        let computer_only = ws.make_block(BlockKind::PlayTune {
            group: CardGroup::Green,
            tune: tune::Tune::TaDa,
            output: SoundOutput::Computer,
            volume: 40,
        });
        ws.add_stack([0.0, 0.0], vec![hat, beep, tune, computer_only]);

        let compiled = compile(&ws).unwrap();
        assert!(compiled.groups.contains(&CardGroup::Blue));
        assert!(compiled.groups.contains(&CardGroup::Red));
        assert!(!compiled.groups.contains(&CardGroup::Green), "computer-only sound needs no device");
        assert!(compiled.uses_speaker);
        assert_eq!(
            compiled.program.scripts[0].steps[0].command,
            Command::Beep {
                group: CardGroup::Blue,
                tone: Tone { pattern: crate::hardware::BeepPattern::Double, frequency: 523, count: 2 },
                output: SoundOutput::Devices,
                volume: 100,
            }
        );
        assert!(matches!(compiled.program.scripts[0].steps[1].command, Command::PlayTune { volume: 100, .. }));
    }

    #[test]
    fn old_beep_blocks_load_as_a_single_a4() {
        let json = r#"{"type":"beep","group":"NoCard"}"#;
        let kind: BlockKind = serde_json::from_str(json).unwrap();
        assert_eq!(
            kind,
            BlockKind::Beep {
                group: CardGroup::NoCard,
                pattern: crate::hardware::BeepPattern::Single,
                note: tune::DEFAULT_NOTE,
                count: 1,
                output: SoundOutput::Devices,
                volume: 100,
            }
        );
    }

    #[test]
    fn compiles_power_and_acceleration_blocks() {
        let mut ws = Workspace::new();
        let hat = ws.make_block(BlockKind::WhenRun);
        let accel = ws.make_block(BlockKind::SetAcceleration {
            motor: Motor::DoubleBoth,
            group: CardGroup::NoCard,
            acceleration: 80,
            deceleration: 250,
        });
        let power = ws.make_block(BlockKind::RunMotor {
            motor: Motor::DoubleBoth,
            group: CardGroup::NoCard,
            direction: Direction::CounterClockwise,
            mode: DriveMode::Power,
            speed: 100,
        });
        let timed = ws.make_block(BlockKind::MotorForTime {
            motor: Motor::Single,
            group: CardGroup::NoCard,
            direction: Direction::Clockwise,
            mode: DriveMode::Power,
            speed: 60,
            seconds: 1.5,
        });
        ws.add_stack([0.0, 0.0], vec![hat, accel, power, timed]);

        let compiled = compile(&ws).unwrap();
        let cmds: Vec<_> = compiled.program.scripts[0].steps.iter().map(|s| s.command.clone()).collect();
        assert_eq!(
            cmds,
            vec![
                Command::MotorSetAcceleration {
                    motor: Motor::DoubleBoth,
                    group: CardGroup::NoCard,
                    acceleration: 80,
                    deceleration: 100,
                },
                Command::MotorRunPower {
                    motor: Motor::DoubleBoth,
                    group: CardGroup::NoCard,
                    direction: Direction::CounterClockwise,
                    power: 100,
                },
                Command::MotorRunPowerForTime {
                    motor: Motor::Single,
                    group: CardGroup::NoCard,
                    direction: Direction::Clockwise,
                    power: 60,
                    milliseconds: 1500,
                },
            ]
        );
        assert!(compiled.devices.contains(&DeviceTarget { kind: DeviceKind::DoubleMotor, group: CardGroup::NoCard }));
        assert!(compiled.devices.contains(&DeviceTarget { kind: DeviceKind::SingleMotor, group: CardGroup::NoCard }));
    }

    #[test]
    fn old_files_without_a_mode_load_as_speed() {
        let json = r#"{"type":"run_motor","motor":"Single","direction":"Clockwise","speed":40}"#;
        let kind: BlockKind = serde_json::from_str(json).unwrap();
        assert!(matches!(kind, BlockKind::RunMotor { mode: DriveMode::Speed, speed: 40, .. }));
    }

    #[test]
    fn empty_workspace_is_an_error() {
        assert!(compile(&Workspace::new()).is_err());
    }

    #[test]
    fn loose_stacks_are_ignored() {
        let mut ws = Workspace::example();
        let b = ws.make_block(BlockKind::Wait { seconds: 1.0 });
        ws.add_stack([500.0, 0.0], vec![b]);
        assert_eq!(compile(&ws).unwrap().program.scripts.len(), 1);
    }
}
