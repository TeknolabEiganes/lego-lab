//! Visual program → executable [`Program`].

use std::collections::BTreeSet;

use crate::hardware::{CardGroup, DeviceTarget, Speed};
use crate::visual::{Block, BlockId, BlockKind, MAX_ROTATIONS, MAX_SECONDS, Workspace};

use super::{Command, Program, Script, Step};

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
}

fn seconds_to_ms(seconds: f32) -> u64 {
    (seconds.clamp(0.0, MAX_SECONDS) as f64 * 1000.0).round() as u64
}

struct Compiler {
    errors: Vec<CompileError>,
    devices: BTreeSet<DeviceTarget>,
    groups: BTreeSet<CardGroup>,
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
            BlockKind::RunMotor { motor, group, direction, speed } => Command::MotorRun {
                motor,
                group,
                direction,
                speed: Speed::new(speed as i64),
            },
            BlockKind::StopMotor { motor, group } => Command::MotorStop { motor, group },
            BlockKind::ResetPosition { motor, group } => Command::MotorResetPosition { motor, group },
            BlockKind::MotorForTime { motor, group, direction, speed, seconds } => {
                if speed <= 0 {
                    self.error(block, "Speed must be more than 0%.");
                    return None;
                }
                Command::MotorRunForTime {
                    motor,
                    group,
                    direction,
                    speed: Speed::new(speed as i64),
                    milliseconds: seconds_to_ms(seconds),
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
            BlockKind::Beep { group } => Command::Beep { group },
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
            self.devices.insert(condition.target());
        }
        if let Some(target) = motor_target(&command) {
            self.devices.insert(target);
        }
        if let Command::Beep { group } = &command {
            self.groups.insert(*group);
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
        | Command::MotorResetPosition { motor, group } => {
            Some(DeviceTarget { kind: motor.device(), group: *group })
        }
        _ => None,
    }
}

pub fn compile(ws: &Workspace) -> Result<Compiled, Vec<CompileError>> {
    let mut c = Compiler { errors: Vec::new(), devices: BTreeSet::new(), groups: BTreeSet::new() };
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
        Ok(Compiled { program, devices: c.devices, groups: c.groups })
    } else {
        Err(c.errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{CardGroup, DeviceKind, Direction, Motor};
    use crate::program::Condition;

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
    fn beep_requires_a_device_in_its_card_group() {
        let mut ws = Workspace::new();
        let hat = ws.make_block(BlockKind::WhenRun);
        let beep = ws.make_block(BlockKind::Beep { group: CardGroup::Blue });
        ws.add_stack([0.0, 0.0], vec![hat, beep]);

        let compiled = compile(&ws).unwrap();
        assert!(compiled.groups.contains(&CardGroup::Blue));
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
