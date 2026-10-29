//! Executable program representation, independent of the UI.

pub mod compiler;
pub mod condition;
pub mod tune;

use serde::{Deserialize, Serialize};

use crate::audio::SoundOutput;
use crate::hardware::{CardGroup, Direction, Motor, Speed, Tone};
use crate::visual::BlockId;

pub use condition::Condition;
use tune::TuneNote;

/// A single instruction. Control-flow commands contain nested steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    MotorRun {
        motor: Motor,
        group: CardGroup,
        direction: Direction,
        speed: Speed,
    },
    MotorRunForTime {
        motor: Motor,
        group: CardGroup,
        direction: Direction,
        speed: Speed,
        milliseconds: u64,
    },
    MotorRunForDegrees {
        motor: Motor,
        group: CardGroup,
        direction: Direction,
        speed: Speed,
        degrees: u32,
    },
    MotorStop {
        motor: Motor,
        group: CardGroup,
    },
    /// Run continuously at a fixed power (duty cycle), 0–100%.
    MotorRunPower {
        motor: Motor,
        group: CardGroup,
        direction: Direction,
        power: u8,
    },
    /// Run at a fixed power (duty cycle) for a duration, then stop.
    MotorRunPowerForTime {
        motor: Motor,
        group: CardGroup,
        direction: Direction,
        power: u8,
        milliseconds: u64,
    },
    /// Acceleration and deceleration, 0–100% each.
    MotorSetAcceleration {
        motor: Motor,
        group: CardGroup,
        acceleration: u8,
        deceleration: u8,
    },
    MotorResetPosition {
        motor: Motor,
        group: CardGroup,
    },
    Beep {
        group: CardGroup,
        tone: Tone,
        output: SoundOutput,
        /// Volume on the computer, 0–100%.
        volume: u32,
    },
    /// Single beeps, each followed by a pause until the next note.
    PlayTune {
        group: CardGroup,
        notes: Vec<TuneNote>,
        output: SoundOutput,
        volume: u32,
    },
    Wait {
        milliseconds: u64,
    },
    WaitUntil {
        condition: Condition,
    },
    Repeat {
        count: u32,
        body: Vec<Step>,
    },
    Forever {
        body: Vec<Step>,
    },
    If {
        condition: Condition,
        then: Vec<Step>,
        otherwise: Vec<Step>,
    },
}

/// A command plus the block it came from (for highlighting).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub source: BlockId,
    pub command: Command,
}

/// One "When Run" script.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Script {
    pub hat: BlockId,
    pub steps: Vec<Step>,
}

/// Everything that runs when RUN is pressed. Scripts run concurrently.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Program {
    pub scripts: Vec<Script>,
}
