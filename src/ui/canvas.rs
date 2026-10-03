//! The coding canvas: pan, zoom, drag, snap, select, delete.

use std::collections::HashSet;

use egui::emath::TSTransform;
use egui::{
    Align2, Area, FontId, Id, Key, LayerId, Order, PointerButton, Pos2, Rect, Sense, Ui, UiBuilder,
    Vec2, pos2, vec2,
};

use crate::program::condition::Condition;
use crate::visual::{Block, BlockId, BlockKind, BlockPath, SeqPath, Workspace};

use super::block::{
    self, ARM_W, BLOCK_W, EMPTY_SLOT_H, FOOT_H, Look, MID_H, header_height, seq_height,
};
use super::theme;

const SNAP_DISTANCE: f32 = 40.0;
const MIN_ZOOM: f32 = 0.35;
const MAX_ZOOM: f32 = 2.0;
const CONDITION_GHOST: Vec2 = vec2(230.0, 32.0);

pub struct View {
    /// Screen offset of the canvas origin from the canvas rect's top-left.
    pub offset: Vec2,
    pub zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        Self { offset: vec2(0.0, 0.0), zoom: 1.0 }
    }
}

/// What is being dragged.
pub enum Payload {
    /// Blocks already removed from the workspace.
    Blocks(Vec<Block>),
    /// A sensor condition from the palette.
    Condition(Condition),
}

pub struct Drag {
    pub payload: Payload,
    /// Pointer position relative to the payload's top-left, canvas units.
    pub grab: Vec2,
    pub from_palette: bool,
}

pub struct Editor {
    pub ws: Workspace,
    pub view: View,
    pub drag: Option<Drag>,
    pub selected: Option<BlockId>,
    clipboard: Option<(Block, [f32; 2])>,
    paste_count: u32,
    /// Screen rect of the palette this frame (drop there to delete).
    pub palette_rect: Rect,
}

impl Editor {
    pub fn new(ws: Workspace) -> Self {
        Self {
            ws,
            view: View::default(),
            drag: None,
            selected: None,
            clipboard: None,
            paste_count: 0,
            palette_rect: Rect::NOTHING,
        }
    }

    pub fn delete_selected(&mut self) -> bool {
        match self.selected.take() {
            Some(id) => self.ws.delete_block(id),
            None => false,
        }
    }
}

/// What the canvas needs to know about execution.
pub struct CanvasInfo<'a> {
    pub active: &'a HashSet<BlockId>,
    pub error_block: Option<BlockId>,
}

// ---- layout -------------------------------------------------------------

struct Laid {
    path: BlockPath,
    rect: Rect,
    /// Y ranges of the mouths of a C-block.
    slots: Vec<(f32, f32)>,
}

/// A place a chain can be inserted: before `index` in `seq`.
struct Snap {
    seq: SeqPath,
    index: usize,
    pos: Pos2,
}

#[derive(Default)]
struct Layout {
    blocks: Vec<Laid>,
    snaps: Vec<Snap>,
    conditions: Vec<(BlockPath, Rect)>,
}

/// Space opened up for a dragged chain: (sequence, index, height).
type Gap<'a> = Option<(&'a SeqPath, usize, f32)>;

fn layout(ws: &Workspace, gap: Gap) -> Layout {
    let mut out = Layout::default();
    for (si, stack) in ws.stacks.iter().enumerate() {
        layout_seq(&stack.blocks, SeqPath::top(si), pos2(stack.pos[0], stack.pos[1]), gap, &mut out);
    }
    out
}

fn layout_seq(seq: &[Block], path: SeqPath, origin: Pos2, gap: Gap, out: &mut Layout) -> f32 {
    let mut y = origin.y;
    let gap_here = |i: usize| match gap {
        Some((p, gi, h)) if *p == path && gi == i && !(path.is_top_level() && i == 0) => h,
        _ => 0.0,
    };
    for (i, b) in seq.iter().enumerate() {
        out.snaps.push(Snap { seq: path.clone(), index: i, pos: pos2(origin.x, y) });
        y += gap_here(i);
        y += layout_block(b, BlockPath { seq: path.clone(), index: i }, pos2(origin.x, y), gap, out);
    }
    out.snaps.push(Snap { seq: path.clone(), index: seq.len(), pos: pos2(origin.x, y) });
    y += gap_here(seq.len());
    y - origin.y
}

fn layout_block(b: &Block, path: BlockPath, pos: Pos2, gap: Gap, out: &mut Layout) -> f32 {
    let idx = out.blocks.len();
    out.blocks.push(Laid { path: path.clone(), rect: Rect::NOTHING, slots: Vec::new() });
    let header = Rect::from_min_size(pos, vec2(BLOCK_W, header_height(&b.kind)));
    if let Some(cond) = block::condition_rect(&b.kind, header) {
        out.conditions.push((path.clone(), cond));
    }
    let mut y = header.bottom();
    let mut slots = Vec::new();
    for slot in 0..b.kind.slot_count() {
        if slot == 1 {
            y += MID_H;
        }
        let child = path.seq.child(path.index, slot);
        let h = layout_seq(b.slot(slot), child, pos2(pos.x + ARM_W, y), gap, out).max(EMPTY_SLOT_H);
        slots.push((y, y + h));
        y += h;
    }
    if !slots.is_empty() {
        y += FOOT_H;
    }
    out.blocks[idx].rect = Rect::from_min_max(pos, pos2(pos.x + BLOCK_W, y));
    out.blocks[idx].slots = slots;
    y - pos.y
}

fn chain_height(blocks: &[Block]) -> f32 {
    seq_height(blocks)
}

/// Closest compatible connection point for a dragged chain.
fn find_target(ws: &Workspace, snaps: &[Snap], chain: &[Block], ghost: Pos2) -> Option<usize> {
    let h = chain_height(chain);
    let mut best: Option<(f32, usize)> = None;
    for (i, s) in snaps.iter().enumerate() {
        let point = if s.seq.is_top_level() && s.index == 0 { s.pos - vec2(0.0, h) } else { s.pos };
        let d = ghost.distance(point);
        if d < SNAP_DISTANCE && best.is_none_or(|(bd, _)| d < bd) && ws.can_insert(&s.seq, s.index, chain) {
            best = Some((d, i));
        }
    }
    best.map(|(_, i)| i)
}

/// Paint a free-floating chain (drag ghost).
fn paint_chain(painter: &egui::Painter, top_left: Pos2, blocks: &[Block], fade: f32) {
    let mut ws = Workspace::new();
    ws.add_stack([top_left.x, top_left.y], blocks.to_vec());
    let lay = layout(&ws, None);
    let look = Look { static_fields: true, fade, ..Look::normal() };
    for l in &lay.blocks {
        if let Some(b) = ws.block(&l.path) {
            block::paint_block(painter, l.rect, &l.slots, &b.kind, look);
        }
    }
    for l in &lay.blocks {
        if let Some(b) = ws.block(&l.path) {
            block::paint_tabs(painter, l.rect, &l.slots, &b.kind, fade);
        }
    }
}

fn paint_grid(painter: &egui::Painter, rect: Rect, transform: TSTransform) {
    let spacing = 26.0 * transform.scaling;
    if spacing < 9.0 {
        return;
    }
    let origin = transform * Pos2::ZERO;
    let start_x = rect.left() + (origin.x - rect.left()).rem_euclid(spacing);
    let start_y = rect.top() + (origin.y - rect.top()).rem_euclid(spacing);
    let r = (1.6 * transform.scaling).clamp(1.0, 2.2);
    let mut y = start_y;
    while y < rect.bottom() {
        let mut x = start_x;
        while x < rect.right() {
            painter.circle_filled(pos2(x, y), r, theme::GRID_DOT);
            x += spacing;
        }
        y += spacing;
    }
}

fn handle_pan_zoom(ui: &Ui, ed: &mut Editor, rect: Rect, bg: &egui::Response) {
    if bg.dragged() {
        ed.view.offset += bg.drag_delta();
    }
    let ctx = ui.ctx();
    let Some(pointer) = ctx.pointer_hover_pos() else { return };
    if !rect.contains(pointer) || egui::Popup::is_any_open(ctx) {
        return;
    }
    let (zoom_delta, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
    if zoom_delta != 1.0 {
        zoom_about(ed, rect, pointer, ed.view.zoom * zoom_delta);
    } else if scroll != Vec2::ZERO {
        ed.view.offset += scroll;
    }
}

fn zoom_about(ed: &mut Editor, rect: Rect, screen_point: Pos2, new_zoom: f32) {
    let new_zoom = new_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    let canvas_point = (screen_point - rect.min - ed.view.offset) / ed.view.zoom;
    ed.view.zoom = new_zoom;
    ed.view.offset = screen_point - rect.min - canvas_point * new_zoom;
}

fn zoom_controls(ui: &Ui, ed: &mut Editor, rect: Rect) {
    Area::new(Id::new("zoom_controls"))
        .order(Order::Foreground)
        .fixed_pos(rect.right_bottom() - vec2(176.0, 52.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(egui::Stroke::new(1.0, theme::PANEL_BORDER))
                .corner_radius(10)
                .inner_margin(4)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("−").on_hover_text("Zoom out").clicked() {
                            zoom_about(ed, rect, rect.center(), ed.view.zoom / 1.2);
                        }
                        if ui.button(format!("{:.0}%", ed.view.zoom * 100.0)).on_hover_text("Reset view").clicked() {
                            ed.view = View::default();
                        }
                        if ui.button("+").on_hover_text("Zoom in").clicked() {
                            zoom_about(ed, rect, rect.center(), ed.view.zoom * 1.2);
                        }
                    });
                });
        });
}

enum Action {
    StartDrag { path: BlockPath, grab: Vec2 },
    Select(BlockId),
    Delete(BlockId),
    Duplicate(BlockId),
}

/// Where the current drag would drop.
enum Target {
    Insert { seq: SeqPath, index: usize, pos: Pos2 },
    Condition(BlockPath),
    Free,
}

fn paint_drag_payload(painter: &egui::Painter, payload: &Payload, pos: Pos2, fade: f32) {
    match payload {
        Payload::Blocks(chain) => paint_chain(painter, pos, chain, fade),
        Payload::Condition(condition) => {
            let rect = Rect::from_min_size(pos, CONDITION_GHOST);
            block::paint_condition_chip(painter, rect, &condition.describe(), false, fade);
        }
    }
}

/// Draw and interact with the canvas. Returns true if the program changed.
pub fn show(ui: &mut Ui, ed: &mut Editor, info: &CanvasInfo) -> bool {
    let mut changed = false;
    let (rect, bg) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let ctx = ui.ctx().clone();

    handle_pan_zoom(ui, ed, rect, &bg);
    if bg.clicked() {
        ed.selected = None;
    }

    let transform = TSTransform::new(rect.min.to_vec2() + ed.view.offset, ed.view.zoom);
    let to_canvas = |p: Pos2| transform.inverse() * p;

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::BG);
    paint_grid(&painter, rect, transform);
    if ed.ws.stacks.is_empty() && ed.drag.is_none() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Drag a “When Run” block here to start your program",
            FontId::proportional(18.0),
            theme::TEXT_MUTED,
        );
    }

    // Work out the drop target from the un-gapped layout.
    let pointer = ctx.pointer_latest_pos();
    let base = layout(&ed.ws, None);
    let mut target = Target::Free;
    let mut ghost = Pos2::ZERO;
    let mut drag_h = 0.0;
    if let (Some(drag), Some(p)) = (&ed.drag, pointer) {
        ghost = to_canvas(p) - drag.grab;
        if rect.contains(p) {
            match &drag.payload {
                Payload::Blocks(chain) => {
                    drag_h = chain_height(chain);
                    if let Some(i) = find_target(&ed.ws, &base.snaps, chain, ghost) {
                        let s = &base.snaps[i];
                        let top_prepend = s.seq.is_top_level() && s.index == 0;
                        let pos = if top_prepend { s.pos - vec2(0.0, drag_h) } else { s.pos };
                        target = Target::Insert { seq: s.seq.clone(), index: s.index, pos };
                    }
                }
                Payload::Condition(_) => {
                    let cp = to_canvas(p);
                    // Innermost (last laid out) condition under the pointer wins.
                    if let Some((path, _)) = base.conditions.iter().rev().find(|(_, r)| r.expand(6.0).contains(cp)) {
                        target = Target::Condition(path.clone());
                    }
                }
            }
        }
    }
    let gap = match &target {
        Target::Insert { seq, index, .. } => Some((seq, *index, drag_h)),
        _ => None,
    };
    let lay = layout(&ed.ws, gap);
    let drop_condition = match &target {
        Target::Condition(p) => Some(p.clone()),
        _ => None,
    };

    // Blocks live on their own transformed layer so widgets scale with zoom.
    let layer = LayerId::new(ui.layer_id().order, ui.id().with("canvas_layer"));
    ctx.set_sublayer(ui.layer_id(), layer);
    ctx.set_transform_layer(layer, transform);
    let canvas_clip = transform.inverse() * rect;
    let mut cui = ui.new_child(UiBuilder::new().layer_id(layer).max_rect(canvas_clip));
    cui.set_clip_rect(canvas_clip);

    let mut actions = Vec::new();
    for laid in &lay.blocks {
        let Some(block) = ed.ws.block_mut(&laid.path) else { continue };
        let id = block.id;
        let resp = cui.interact(laid.rect, Id::new(("block", id)), Sense::click_and_drag());
        let look = Look {
            selected: ed.selected == Some(id),
            active: info.active.contains(&id),
            error: info.error_block == Some(id),
            drop_condition: drop_condition.as_ref() == Some(&laid.path),
            ..Look::normal()
        };
        block::paint_block(cui.painter(), laid.rect, &laid.slots, &block.kind, look);
        let header = Rect::from_min_size(laid.rect.min, vec2(BLOCK_W, header_height(&block.kind)));
        if block::field_widgets(&mut cui, header, id, &mut block.kind) {
            changed = true;
        }
        if resp.drag_started_by(PointerButton::Primary) {
            let press = ctx.input(|i| i.pointer.press_origin()).unwrap_or(laid.rect.min);
            actions.push(Action::StartDrag { path: laid.path.clone(), grab: to_canvas(press) - laid.rect.min });
        } else if resp.clicked() {
            actions.push(Action::Select(id));
        }
        resp.context_menu(|ui| {
            if ui.button("Duplicate").clicked() {
                actions.push(Action::Duplicate(id));
                ui.close();
            }
            if ui.button("Delete block").clicked() {
                actions.push(Action::Delete(id));
                ui.close();
            }
        });
    }
    for laid in &lay.blocks {
        if let Some(b) = ed.ws.block(&laid.path) {
            block::paint_tabs(cui.painter(), laid.rect, &laid.slots, &b.kind, 1.0);
        }
    }

    // The dragged payload follows the pointer on top of everything.
    if let Some(drag) = &ed.drag
        && pointer.is_some()
    {
        let p = pointer.unwrap();
        let pos = match &target {
            Target::Insert { pos, .. } => *pos,
            _ => ghost,
        };
        if rect.contains(p) {
            // Draw in the canvas's already-transformed layer. This keeps the
            // drag preview aligned with the blocks at every zoom level.
            paint_drag_payload(&cui.painter(), &drag.payload, pos, 1.0);
        } else {
            let ghost_layer = LayerId::new(Order::Tooltip, Id::new("drag_ghost"));
            ctx.set_transform_layer(ghost_layer, transform);
            let gp = ctx.layer_painter(ghost_layer);
            let fade = if ed.palette_rect.contains(p) { 0.45 } else { 0.95 };
            paint_drag_payload(&gp, &drag.payload, pos, fade);
        }
    }

    // Drop.
    let released = ed.drag.is_some() && !ctx.input(|i| i.pointer.primary_down());
    if released && let Some(drag) = ed.drag.take() {
        let p = pointer.unwrap_or_default();
        let over_palette = ed.palette_rect.contains(p);
        let on_canvas = rect.contains(p) && !over_palette;
        match drag.payload {
            Payload::Blocks(chain) => {
                if on_canvas {
                    match target {
                        Target::Insert { seq, index, .. } => {
                            if seq.is_top_level() && index == 0 {
                                ed.ws.stacks[seq.stack].pos[1] -= drag_h;
                            }
                            ed.ws.insert_chain(&seq, index, chain);
                        }
                        _ => ed.ws.add_stack([ghost.x, ghost.y], chain),
                    }
                } else if !drag.from_palette && !over_palette {
                    // Released outside the canvas (e.g. over the toolbar): keep it.
                    ed.ws.add_stack([ghost.x, ghost.y], chain);
                }
            }
            Payload::Condition(condition) => match target {
                Target::Condition(path) => {
                    if let Some(c) = ed.ws.block_mut(&path).and_then(|b| b.kind.condition_mut()) {
                        *c = condition;
                    }
                }
                _ if on_canvas => {
                    let b = ed.ws.make_block(BlockKind::WaitUntil { condition });
                    ed.ws.add_stack([ghost.x, ghost.y], vec![b]);
                }
                _ => {}
            },
        }
        changed = true;
    }

    for action in actions {
        match action {
            Action::StartDrag { path, grab } => {
                let blocks = ed.ws.take_chain(&path);
                ed.selected = blocks.first().map(|b| b.id);
                ed.drag = Some(Drag { payload: Payload::Blocks(blocks), grab, from_palette: false });
                changed = true;
            }
            Action::Select(id) => ed.selected = Some(id),
            Action::Delete(id) => {
                changed |= ed.ws.delete_block(id);
                if ed.selected == Some(id) {
                    ed.selected = None;
                }
            }
            Action::Duplicate(id) => {
                if let Some(new_id) = ed.ws.duplicate_block(id) {
                    ed.selected = Some(new_id);
                    changed = true;
                }
            }
        }
    }

    let typing = ctx.memory(|m| m.focused().is_some());
    if !typing && !egui::Popup::is_any_open(&ctx) && ed.drag.is_none() {
        let (copy, paste, delete) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(Key::C),
                i.modifiers.command && i.key_pressed(Key::V),
                i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
            )
        });
        if copy && let Some(id) = ed.selected && let Some(template) = ed.ws.copy_block_template(id) {
            ed.clipboard = Some(template);
            ed.paste_count = 0;
        }
        if paste && let Some((template, origin)) = ed.clipboard.clone() {
            let offset = ed.paste_count.min(10) as f32 * 24.0;
            let pos = if let Some(pointer) = pointer.filter(|p| rect.contains(*p)) {
                let canvas_pos = to_canvas(pointer) - vec2(40.0, 18.0) + vec2(offset, offset);
                [canvas_pos.x, canvas_pos.y]
            } else {
                [origin[0] + 300.0 + offset, origin[1] + offset]
            };
            let id = ed.ws.paste_block_copy(&template, pos);
            ed.selected = Some(id);
            ed.paste_count = ed.paste_count.saturating_add(1);
            changed = true;
        }
        if delete {
            changed |= ed.delete_selected();
        }
    }

    if ed.drag.is_some() {
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    zoom_controls(ui, ed, rect);
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::block::block_height;

    #[test]
    fn layout_nests_blocks_inside_mouths() {
        let ws = crate::visual::tests::colour_program();
        let lay = layout(&ws, None);
        // hat, forever, if/else, run motor, stop motor
        assert_eq!(lay.blocks.len(), 5);
        assert_eq!(lay.conditions.len(), 1);
        let forever = &lay.blocks[1];
        let if_else = &lay.blocks[2];
        assert_eq!(if_else.rect.left(), forever.rect.left() + ARM_W);
        assert!(forever.rect.bottom() > if_else.rect.bottom());
        assert_eq!(if_else.slots.len(), 2);
        assert_eq!(forever.rect.height(), block_height(&ws.stacks[0].blocks[1]));
    }

    #[test]
    fn snapping_finds_mouths_and_ignores_far_points() {
        let mut ws = crate::visual::tests::colour_program();
        let chain = vec![ws.make_block(BlockKind::Wait { seconds: 1.0 })];
        let lay = layout(&ws, None);
        let inner = SeqPath::top(0).child(1, 0);
        let mouth = lay.snaps.iter().find(|s| s.seq == inner && s.index == 0).unwrap().pos;
        let i = find_target(&ws, &lay.snaps, &chain, mouth + vec2(5.0, 5.0)).unwrap();
        assert_eq!(lay.snaps[i].seq, inner);
        assert!(find_target(&ws, &lay.snaps, &chain, pos2(5000.0, 5000.0)).is_none());
    }

    #[test]
    fn gap_pushes_following_blocks_down() {
        let ws = Workspace::example();
        let seq = SeqPath::top(0);
        let before = layout(&ws, None).blocks[2].rect.top();
        let after = layout(&ws, Some((&seq, 2, 50.0))).blocks[2].rect.top();
        assert_eq!(after - before, 50.0);
    }
}
