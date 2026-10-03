//! Executable program representation, independent of the UI.

pub mod compiler;
pub mod condition;

use serde::{Deserialize, Serialize};

use crate::hardware::{CardGroup, Direction, Motor, Speed};
use crate::visual::BlockId;

pub use condition::Condition;

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
    MotorResetPosition {
        motor: Motor,
        group: CardGroup,
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
