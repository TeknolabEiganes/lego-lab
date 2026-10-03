//! The visual program: what the child sees and edits on the canvas.
//!
//! This is the UI representation. It knows about positions and stacks but
//! nothing about hardware or execution; `program::compiler` turns it into an
//! executable [`crate::program::Program`].

use serde::{Deserialize, Serialize};

use crate::hardware::{CardGroup, Direction, Motor};
use crate::program::condition::Condition;

pub type BlockId = u64;

pub const MAX_SECONDS: f32 = 3600.0;
pub const MAX_ROTATIONS: f32 = 1000.0;
pub const MAX_REPEAT: u32 = 10_000;
/// Deepest nesting a loaded file may contain.
pub const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Events,
    Motors,
    Sounds,
    Control,
    Sensors,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockKind {
    WhenRun,
    RunMotor {
        motor: Motor,
        #[serde(default)]
        group: CardGroup,
        direction: Direction,
        speed: i32,
    },
    StopMotor {
        motor: Motor,
        #[serde(default)]
        group: CardGroup,
    },
    ResetPosition {
        motor: Motor,
        #[serde(default)]
        group: CardGroup,
    },
    MotorForTime {
        motor: Motor,
        #[serde(default)]
        group: CardGroup,
        direction: Direction,
        speed: i32,
        seconds: f32,
    },
    MotorRotations {
        motor: Motor,
        #[serde(default)]
        group: CardGroup,
        direction: Direction,
        speed: i32,
        rotations: f32,
    },
    Beep {
        #[serde(default)]
        group: CardGroup,
    },
    Wait {
        seconds: f32,
    },
    WaitUntil {
        condition: Condition,
    },
    Repeat {
        count: u32,
    },
    Forever,
    If {
        condition: Condition,
    },
    IfElse {
        condition: Condition,
    },
}

impl BlockKind {
    /// Blocks offered in the palette, in display order.
    pub fn palette() -> Vec<BlockKind> {
        let motor = Motor::Single;
        let direction = Direction::Clockwise;
        let condition = Condition::default();
        vec![
            BlockKind::WhenRun,
            BlockKind::RunMotor { motor, group: CardGroup::NoCard, direction, speed: 50 },
            BlockKind::StopMotor { motor, group: CardGroup::NoCard },
            BlockKind::MotorForTime { motor, group: CardGroup::NoCard, direction, speed: 50, seconds: 1.0 },
            BlockKind::MotorRotations { motor, group: CardGroup::NoCard, direction, speed: 50, rotations: 1.0 },
            BlockKind::ResetPosition { motor, group: CardGroup::NoCard },
            BlockKind::Beep { group: CardGroup::NoCard },
            BlockKind::Wait { seconds: 1.0 },
            BlockKind::Repeat { count: 4 },
            BlockKind::Forever,
            BlockKind::If { condition },
            BlockKind::IfElse { condition },
            BlockKind::WaitUntil { condition },
        ]
    }

    pub fn category(&self) -> Category {
        match self {
            BlockKind::WhenRun => Category::Events,
            BlockKind::RunMotor { .. }
            | BlockKind::StopMotor { .. }
            | BlockKind::ResetPosition { .. }
            | BlockKind::MotorForTime { .. }
            | BlockKind::MotorRotations { .. } => Category::Motors,
            BlockKind::Beep { .. } => Category::Sounds,
            BlockKind::Wait { .. }
            | BlockKind::WaitUntil { .. }
            | BlockKind::Repeat { .. }
            | BlockKind::Forever
            | BlockKind::If { .. }
            | BlockKind::IfElse { .. } => Category::Control,
        }
    }

    /// Hat blocks start a script and can only sit at the top of a stack.
    pub fn is_hat(&self) -> bool {
        matches!(self, BlockKind::WhenRun)
    }

    /// Nothing can be attached below a cap block (it never finishes).
    pub fn is_cap(&self) -> bool {
        matches!(self, BlockKind::Forever)
    }

    /// Number of nested block sequences ("mouths") this block has.
    pub fn slot_count(&self) -> usize {
        match self {
            BlockKind::Repeat { .. } | BlockKind::Forever | BlockKind::If { .. } => 1,
            BlockKind::IfElse { .. } => 2,
            _ => 0,
        }
    }

    pub fn condition_mut(&mut self) -> Option<&mut Condition> {
        match self {
            BlockKind::WaitUntil { condition }
            | BlockKind::If { condition }
            | BlockKind::IfElse { condition } => Some(condition),
            _ => None,
        }
    }

    pub fn condition(&self) -> Option<&Condition> {
        match self {
            BlockKind::WaitUntil { condition }
            | BlockKind::If { condition }
            | BlockKind::IfElse { condition } => Some(condition),
            _ => None,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            BlockKind::WhenRun => "When Run",
            BlockKind::RunMotor { .. } => "Run Motor",
            BlockKind::StopMotor { .. } => "Stop Motor",
            BlockKind::ResetPosition { .. } => "Reset Position",
            BlockKind::MotorForTime { .. } => "Motor for Time",
            BlockKind::MotorRotations { .. } => "Motor Rotations",
            BlockKind::Beep { .. } => "Beep",
            BlockKind::Wait { .. } => "Wait",
            BlockKind::WaitUntil { .. } => "Wait Until",
            BlockKind::Repeat { .. } => "Repeat",
            BlockKind::Forever => "Forever",
            BlockKind::If { .. } => "If",
            BlockKind::IfElse { .. } => "If / Else",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            BlockKind::WhenRun => "▶",
            BlockKind::RunMotor { .. } | BlockKind::MotorForTime { .. } => "⚙",
            BlockKind::StopMotor { .. } => "■",
            BlockKind::ResetPosition { .. } => "⟲",
            BlockKind::MotorRotations { .. } => "⟳",
            BlockKind::Beep { .. } => "♫",
            BlockKind::Wait { .. } | BlockKind::WaitUntil { .. } => "⏱",
            BlockKind::Repeat { .. } | BlockKind::Forever => "🔁",
            BlockKind::If { .. } | BlockKind::IfElse { .. } => "❓",
        }
    }

    /// Force every parameter into its valid range. Applied to loaded files
    /// and after every edit.
    pub fn sanitize(&mut self) {
        fn secs(v: &mut f32) {
            *v = if v.is_finite() { v.clamp(0.0, MAX_SECONDS) } else { 1.0 };
        }
        match self {
            BlockKind::WhenRun
            | BlockKind::StopMotor { .. }
            | BlockKind::ResetPosition { .. }
            | BlockKind::Beep { .. }
            | BlockKind::Forever => {}
            BlockKind::RunMotor { speed, .. } => *speed = (*speed).clamp(0, 100),
            BlockKind::MotorForTime { speed, seconds, .. } => {
                *speed = (*speed).clamp(0, 100);
                secs(seconds);
            }
            BlockKind::MotorRotations { speed, rotations, .. } => {
                *speed = (*speed).clamp(0, 100);
                *rotations = if rotations.is_finite() {
                    rotations.clamp(0.0, MAX_ROTATIONS)
                } else {
                    1.0
                };
            }
            BlockKind::Wait { seconds } => secs(seconds),
            BlockKind::Repeat { count } => *count = (*count).min(MAX_REPEAT),
            BlockKind::WaitUntil { condition }
            | BlockKind::If { condition }
            | BlockKind::IfElse { condition } => condition.sanitize(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub id: BlockId,
    #[serde(flatten)]
    pub kind: BlockKind,
    /// Blocks inside a C-block (Repeat, Forever, If, the "then" of If/Else).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body: Vec<Block>,
    /// The "else" part of If/Else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub else_body: Vec<Block>,
}

impl Block {
    pub fn slot(&self, slot: usize) -> &Vec<Block> {
        if slot == 0 { &self.body } else { &self.else_body }
    }

    pub fn slot_mut(&mut self, slot: usize) -> &mut Vec<Block> {
        if slot == 0 { &mut self.body } else { &mut self.else_body }
    }

    /// Visit this block and everything nested inside it.
    #[cfg(test)]
    pub fn walk(&self, f: &mut impl FnMut(&Block)) {
        f(self);
        for b in self.body.iter().chain(&self.else_body) {
            b.walk(f);
        }
    }
}

/// A vertical chain of connected blocks at a canvas position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stack {
    /// Top-left corner in canvas coordinates.
    pub pos: [f32; 2],
    pub blocks: Vec<Block>,
}

/// Address of a block sequence: a top-level stack, or a slot inside a
/// C-block reached by `(block index, slot)` steps from that stack.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SeqPath {
    pub stack: usize,
    pub nest: Vec<(usize, usize)>,
}

impl SeqPath {
    pub fn top(stack: usize) -> Self {
        Self { stack, nest: Vec::new() }
    }

    pub fn is_top_level(&self) -> bool {
        self.nest.is_empty()
    }

    pub fn child(&self, index: usize, slot: usize) -> Self {
        let mut nest = self.nest.clone();
        nest.push((index, slot));
        Self { stack: self.stack, nest }
    }
}

/// Address of one block.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockPath {
    pub seq: SeqPath,
    pub index: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub stacks: Vec<Stack>,
    #[serde(skip)]
    next_id: BlockId,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    /// The starter program from the plan: run, wait, stop.
    #[cfg(test)]
    pub fn example() -> Self {
        let mut ws = Self::new();
        let motor = Motor::Single;
        let blocks = vec![
            ws.make_block(BlockKind::WhenRun),
            ws.make_block(BlockKind::RunMotor { motor, group: CardGroup::NoCard, direction: Direction::Clockwise, speed: 50 }),
            ws.make_block(BlockKind::Wait { seconds: 2.0 }),
            ws.make_block(BlockKind::StopMotor { motor, group: CardGroup::NoCard }),
        ];
        ws.stacks.push(Stack { pos: [80.0, 60.0], blocks });
        ws
    }

    pub fn make_block(&mut self, kind: BlockKind) -> Block {
        self.next_id += 1;
        Block { id: self.next_id, kind, body: Vec::new(), else_body: Vec::new() }
    }

    pub fn seq(&self, path: &SeqPath) -> Option<&Vec<Block>> {
        let mut seq = &self.stacks.get(path.stack)?.blocks;
        for &(index, slot) in &path.nest {
            seq = seq.get(index)?.slot(slot);
        }
        Some(seq)
    }

    pub fn seq_mut(&mut self, path: &SeqPath) -> Option<&mut Vec<Block>> {
        let mut seq = &mut self.stacks.get_mut(path.stack)?.blocks;
        for &(index, slot) in &path.nest {
            seq = seq.get_mut(index)?.slot_mut(slot);
        }
        Some(seq)
    }

    pub fn block(&self, path: &BlockPath) -> Option<&Block> {
        self.seq(&path.seq)?.get(path.index)
    }

    pub fn block_mut(&mut self, path: &BlockPath) -> Option<&mut Block> {
        self.seq_mut(&path.seq)?.get_mut(path.index)
    }

    pub fn find(&self, id: BlockId) -> Option<BlockPath> {
        fn search(seq: &[Block], path: SeqPath, id: BlockId) -> Option<BlockPath> {
            for (i, b) in seq.iter().enumerate() {
                if b.id == id {
                    return Some(BlockPath { seq: path, index: i });
                }
                for slot in 0..2 {
                    if let Some(found) = search(b.slot(slot), path.child(i, slot), id) {
                        return Some(found);
                    }
                }
            }
            None
        }
        self.stacks
            .iter()
            .enumerate()
            .find_map(|(si, s)| search(&s.blocks, SeqPath::top(si), id))
    }

    /// Detach the block at `path` and everything below it in its sequence.
    pub fn take_chain(&mut self, path: &BlockPath) -> Vec<Block> {
        let Some(seq) = self.seq_mut(&path.seq) else { return Vec::new() };
        let chain = seq.split_off(path.index.min(seq.len()));
        self.remove_empty_stack(&path.seq);
        chain
    }

    fn remove_empty_stack(&mut self, seq: &SeqPath) {
        if seq.is_top_level() && self.stacks.get(seq.stack).is_some_and(|s| s.blocks.is_empty()) {
            self.stacks.remove(seq.stack);
        }
    }

    /// Delete a block (with anything inside it); blocks below move up.
    pub fn delete_block(&mut self, id: BlockId) -> bool {
        let Some(path) = self.find(id) else { return false };
        if let Some(seq) = self.seq_mut(&path.seq) {
            seq.remove(path.index);
        }
        self.remove_empty_stack(&path.seq);
        true
    }

    fn fresh_copy(&mut self, block: &Block) -> Block {
        let mut copy = self.make_block(block.kind.clone());
        copy.body = block.body.iter().map(|b| self.fresh_copy(b)).collect();
        copy.else_body = block.else_body.iter().map(|b| self.fresh_copy(b)).collect();
        copy
    }

    /// Copy a block template and the position of its containing stack.
    pub fn copy_block_template(&self, id: BlockId) -> Option<(Block, [f32; 2])> {
        let path = self.find(id)?;
        let block = self.block(&path)?.clone();
        let pos = self.stacks.get(path.seq.stack)?.pos;
        Some((block, pos))
    }

    /// Paste a fresh copy of a block template as a new stack at `pos`.
    pub fn paste_block_copy(&mut self, template: &Block, pos: [f32; 2]) -> BlockId {
        let copy = self.fresh_copy(template);
        let id = copy.id;
        self.add_stack(pos, vec![copy]);
        id
    }

    /// Duplicate a block (and its contents) into a new stack beside the
    /// original. Returns the new id.
    pub fn duplicate_block(&mut self, id: BlockId) -> Option<BlockId> {
        let path = self.find(id)?;
        let original = self.block(&path)?.clone();
        let copy = self.fresh_copy(&original);
        let new_id = copy.id;
        let [x, y] = self.stacks[path.seq.stack].pos;
        self.stacks.push(Stack { pos: [x + 300.0, y], blocks: vec![copy] });
        Some(new_id)
    }

    /// Whether `chain` may be inserted into `seq` at `index`.
    pub fn can_insert(&self, seq: &SeqPath, index: usize, chain: &[Block]) -> bool {
        let Some(target) = self.seq(seq) else { return false };
        let (Some(_), Some(last)) = (chain.first(), chain.last()) else { return false };
        if index > target.len() {
            return false;
        }
        // Nothing may follow a cap block (Forever).
        if last.kind.is_cap() && index < target.len() {
            return false;
        }
        if index > 0 && target[index - 1].kind.is_cap() {
            return false;
        }
        let has_hat = chain.iter().any(|b| b.kind.is_hat());
        if seq.is_top_level() && index == 0 {
            // Prepending to a stack: its current top must not be a hat, and
            // a hat may only lead the dragged chain.
            !target.first().is_some_and(|b| b.kind.is_hat())
                && !chain[1..].iter().any(|b| b.kind.is_hat())
        } else {
            !has_hat
        }
    }

    pub fn insert_chain(&mut self, seq: &SeqPath, index: usize, chain: Vec<Block>) {
        if let Some(target) = self.seq_mut(seq) {
            let index = index.min(target.len());
            target.splice(index..index, chain);
        }
    }

    pub fn add_stack(&mut self, pos: [f32; 2], blocks: Vec<Block>) {
        if !blocks.is_empty() {
            self.stacks.push(Stack { pos, blocks });
        }
    }

    /// Repair anything a hand-edited or corrupted file could contain:
    /// out-of-range parameters, duplicate ids, misplaced hats, blocks after
    /// a Forever, bodies on blocks that can't have them, excessive nesting,
    /// empty stacks and non-finite positions.
    pub fn sanitize(&mut self) {
        fn clean_seq(seq: &mut Vec<Block>, depth: usize) {
            seq.retain(|b| !b.kind.is_hat());
            if let Some(cap) = seq.iter().position(|b| b.kind.is_cap()) {
                seq.truncate(cap + 1);
            }
            for b in seq {
                clean_block(b, depth);
            }
        }
        fn clean_block(b: &mut Block, depth: usize) {
            b.kind.sanitize();
            let slots = if depth >= MAX_DEPTH { 0 } else { b.kind.slot_count() };
            if slots < 1 {
                b.body.clear();
            }
            if slots < 2 {
                b.else_body.clear();
            }
            clean_seq(&mut b.body, depth + 1);
            clean_seq(&mut b.else_body, depth + 1);
        }

        let mut fixed: Vec<Stack> = Vec::new();
        for stack in std::mem::take(&mut self.stacks) {
            let [mut x, mut y] = stack.pos;
            if !x.is_finite() {
                x = 0.0;
            }
            if !y.is_finite() {
                y = 0.0;
            }
            let mut current = Stack { pos: [x, y], blocks: Vec::new() };
            for mut block in stack.blocks {
                clean_block(&mut block, 1);
                let after_cap = current.blocks.last().is_some_and(|b| b.kind.is_cap());
                if !current.blocks.is_empty() && (block.kind.is_hat() || after_cap) {
                    // Split: a hat always starts a new stack; nothing follows a cap.
                    let pos = [current.pos[0] + 320.0, current.pos[1]];
                    fixed.push(std::mem::replace(&mut current, Stack { pos, blocks: Vec::new() }));
                }
                current.blocks.push(block);
            }
            if !current.blocks.is_empty() {
                fixed.push(current);
            }
        }
        self.stacks = fixed;

        // Make ids unique.
        fn visit(seq: &mut [Block], f: &mut impl FnMut(&mut Block)) {
            for b in seq {
                f(b);
                visit(&mut b.body, f);
                visit(&mut b.else_body, f);
            }
        }
        let mut max_id = 0;
        for s in &mut self.stacks {
            visit(&mut s.blocks, &mut |b| max_id = max_id.max(b.id));
        }
        let mut seen = std::collections::HashSet::new();
        let mut next = max_id;
        for s in &mut self.stacks {
            visit(&mut s.blocks, &mut |b| {
                if b.id == 0 || !seen.insert(b.id) {
                    next += 1;
                    b.id = next;
                    seen.insert(b.id);
                }
            });
        }
        self.next_id = next;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::program::condition::{Comparison, Condition};

    /// FOREVER { IF colour is red { motor 100% } ELSE { stop } }
    pub fn colour_program() -> Workspace {
        let mut ws = Workspace::new();
        let motor = Motor::Single;
        let hat = ws.make_block(BlockKind::WhenRun);
        let mut forever = ws.make_block(BlockKind::Forever);
        let mut if_else = ws.make_block(BlockKind::IfElse {
            condition: Condition::ColorIs { color: crate::hardware::LegoColor::Red, group: CardGroup::NoCard },
        });
        if_else.body.push(ws.make_block(BlockKind::RunMotor { motor, group: CardGroup::NoCard, direction: Direction::Clockwise, speed: 100 }));
        if_else.else_body.push(ws.make_block(BlockKind::StopMotor { motor, group: CardGroup::NoCard }));
        forever.body.push(if_else);
        ws.add_stack([0.0, 0.0], vec![hat, forever]);
        ws
    }

    #[test]
    fn take_and_insert_chain() {
        let mut ws = Workspace::example();
        let chain = ws.take_chain(&BlockPath { seq: SeqPath::top(0), index: 2 });
        assert_eq!(chain.len(), 2);
        assert_eq!(ws.stacks[0].blocks.len(), 2);
        assert!(ws.can_insert(&SeqPath::top(0), 1, &chain));
        ws.insert_chain(&SeqPath::top(0), 1, chain);
        assert_eq!(ws.stacks[0].blocks[1].kind.title(), "Wait");
    }

    #[test]
    fn hats_cannot_go_mid_stack_or_inside() {
        let mut ws = colour_program();
        let hat = vec![ws.make_block(BlockKind::WhenRun)];
        assert!(!ws.can_insert(&SeqPath::top(0), 1, &hat));
        assert!(!ws.can_insert(&SeqPath::top(0), 0, &hat), "stack already starts with a hat");
        assert!(!ws.can_insert(&SeqPath::top(0).child(1, 0), 0, &hat));
    }

    #[test]
    fn nothing_attaches_below_forever() {
        let mut ws = colour_program();
        let wait = vec![ws.make_block(BlockKind::Wait { seconds: 1.0 })];
        assert!(!ws.can_insert(&SeqPath::top(0), 2, &wait));
        assert!(ws.can_insert(&SeqPath::top(0).child(1, 0), 1, &wait), "but inside is fine");
        let forever = vec![ws.make_block(BlockKind::Forever)];
        assert!(!ws.can_insert(&SeqPath::top(0), 1, &forever), "forever can't go above blocks");
    }

    #[test]
    fn find_and_take_nested() {
        let mut ws = colour_program();
        let stop_id = ws.stacks[0].blocks[1].body[0].else_body[0].id;
        let path = ws.find(stop_id).unwrap();
        assert_eq!(path.seq, SeqPath::top(0).child(1, 0).child(0, 1));
        let chain = ws.take_chain(&path);
        assert_eq!(chain[0].id, stop_id);
        assert!(ws.stacks[0].blocks[1].body[0].else_body.is_empty());
    }

    #[test]
    fn duplicate_gives_fresh_ids() {
        let mut ws = colour_program();
        let forever_id = ws.stacks[0].blocks[1].id;
        let copy = ws.duplicate_block(forever_id).unwrap();
        let mut ids = std::collections::HashSet::new();
        for s in &ws.stacks {
            for b in &s.blocks {
                b.walk(&mut |b| assert!(ids.insert(b.id), "duplicate id {}", b.id));
            }
        }
        assert_ne!(copy, forever_id);
    }

    #[test]
    fn copy_paste_makes_a_fresh_deep_copy() {
        let mut ws = colour_program();
        let source_id = ws.stacks[0].blocks[1].id;
        let (template, _) = ws.copy_block_template(source_id).unwrap();
        let pasted_id = ws.paste_block_copy(&template, [500.0, 20.0]);
        let pasted = ws.block(&ws.find(pasted_id).unwrap()).unwrap();

        assert_eq!(pasted.kind, template.kind);
        assert_eq!(pasted.body.len(), template.body.len());
        let mut source_ids = std::collections::HashSet::new();
        let mut pasted_ids = std::collections::HashSet::new();
        ws.block(&ws.find(source_id).unwrap()).unwrap().walk(&mut |b| {
            source_ids.insert(b.id);
        });
        pasted.walk(&mut |b| {
            assert!(pasted_ids.insert(b.id), "duplicate id {} within pasted block", b.id);
            assert!(!source_ids.contains(&b.id), "pasted block reused source id {}", b.id);
        });
    }

    #[test]
    fn sanitize_repairs_bad_input() {
        let json = r#"{"stacks":[{"pos":[0,0],"blocks":[
            {"id":1,"type":"run_motor","motor":"Single","direction":"Clockwise","speed":9000},
            {"id":1,"type":"when_run"},
            {"id":2,"type":"wait","seconds":-5,"body":[{"id":7,"type":"forever"}]},
            {"id":3,"type":"forever","body":[{"id":4,"type":"when_run"}]},
            {"id":5,"type":"stop_motor","motor":"Single"},
            {"id":6,"type":"if","condition":{"sensor":"reflection","cmp":"Less","percent":500}}
        ]}]}"#;
        let mut ws: Workspace = serde_json::from_str(json).unwrap();
        ws.sanitize();
        assert_eq!(ws.stacks.len(), 3, "hat and forever split the stack");
        assert_eq!(
            ws.stacks[0].blocks[0].kind,
            BlockKind::RunMotor { motor: Motor::Single, group: CardGroup::NoCard, direction: Direction::Clockwise, speed: 100 }
        );
        let wait = &ws.stacks[1].blocks[1];
        assert_eq!(wait.kind, BlockKind::Wait { seconds: 0.0 });
        assert!(wait.body.is_empty(), "wait can't have a body");
        assert!(ws.stacks[1].blocks[2].body.is_empty(), "hat removed from forever body");
        assert_eq!(
            ws.stacks[2].blocks[1].kind,
            BlockKind::If { condition: Condition::Reflection { cmp: Comparison::Less, percent: 100, group: CardGroup::NoCard } }
        );
        let mut ids = std::collections::HashSet::new();
        for s in &ws.stacks {
            for b in &s.blocks {
                b.walk(&mut |b| assert!(ids.insert(b.id)));
            }
        }
    }
}
