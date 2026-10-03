//! Drawing and editing a single block.

use egui::{
    Align2, Color32, CornerRadius, DragValue, FontId, Painter, Rect, Sense, Stroke, StrokeKind, Ui, vec2,
    pos2,
};

use crate::hardware::{CardGroup, DeviceKind, Direction, LegoColor, Lever, Motor};
use crate::program::condition::{Comparison, Condition, ConditionKind, MAX_DEGREES};
use crate::visual::{Block, BlockKind, Category, MAX_REPEAT, MAX_ROTATIONS, MAX_SECONDS};

use super::theme;

pub const BLOCK_W: f32 = 270.0;
const HEADER_H: f32 = 36.0;
const HAT_H: f32 = 50.0;
const ROW_H: f32 = 30.0;
const BOTTOM_PAD: f32 = 8.0;
const FIELD_H: f32 = 24.0;
const PAD_X: f32 = 12.0;
const GAP: f32 = 6.0;
pub const TAB_W: f32 = 34.0;
pub const TAB_H: f32 = 7.0;
pub const TAB_X: f32 = 22.0;
/// Width of the left arm of a C-block; nested blocks are indented by this.
pub const ARM_W: f32 = 18.0;
/// Height of an empty mouth.
pub const EMPTY_SLOT_H: f32 = 28.0;
/// Height of the "else" bar of If/Else.
pub const MID_H: f32 = 30.0;
/// Height of the bottom bar of a C-block.
pub const FOOT_H: f32 = 22.0;

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Motor,
    Direction,
    Speed,
    Seconds,
    Rotations,
    Count,
    CondKind,
    CondColor,
    CondCmp,
    CondPercent,
    CondDegrees,
    CondDevice,
    CondMotor,
    CondLever,
    Text(&'static str),
}

impl Field {
    fn is_condition(self) -> bool {
        matches!(
            self,
            Field::CondKind
                | Field::CondColor
                | Field::CondCmp
                | Field::CondPercent
                | Field::CondDegrees
                | Field::CondDevice
                | Field::CondMotor
                | Field::CondLever
        )
    }
}

type Row = Vec<(Field, f32)>;

fn condition_rows(c: &Condition) -> Vec<Row> {
    use Field::*;
    let picker = vec![(CondKind, 170.0)];
    match c {
        Condition::ColorIs { .. } => vec![picker, vec![(CondColor, 140.0)]],
        Condition::Reflection { .. } => vec![picker, vec![(CondCmp, 54.0), (CondPercent, 70.0)]],
        Condition::ButtonPressed { .. } => vec![picker, vec![(Text("on"), 22.0), (CondDevice, 160.0)]],
        Condition::MotorPosition { .. } => {
            vec![picker, vec![(CondMotor, 180.0)], vec![(CondCmp, 54.0), (CondDegrees, 84.0)]]
        }
        Condition::LeverPosition { .. } => vec![picker, vec![(CondLever, 80.0), (CondCmp, 54.0), (CondPercent, 70.0)]],
    }
}

/// Field rows below the header, with widths.
fn rows(kind: &BlockKind) -> Vec<Row> {
    use Field::*;
    let motor = vec![(Motor, 200.0)];
    let dir_speed = vec![(Direction, 136.0), (Text("speed"), 44.0), (Speed, 58.0)];
    match kind {
        BlockKind::WhenRun | BlockKind::Forever => vec![],
        BlockKind::RunMotor { .. } => vec![motor, dir_speed],
        BlockKind::StopMotor { .. } | BlockKind::ResetPosition { .. } => vec![motor],
        BlockKind::MotorForTime { .. } => {
            vec![motor, dir_speed, vec![(Text("for"), 24.0), (Seconds, 70.0), (Text("seconds"), 60.0)]]
        }
        BlockKind::MotorRotations { .. } => {
            vec![motor, dir_speed, vec![(Text("for"), 24.0), (Rotations, 70.0), (Text("rotations"), 70.0)]]
        }
        BlockKind::Wait { .. } => vec![vec![(Seconds, 70.0), (Text("seconds"), 60.0)]],
        BlockKind::Repeat { .. } => vec![vec![(Count, 70.0), (Text("times"), 50.0)]],
        BlockKind::WaitUntil { condition }
        | BlockKind::If { condition }
        | BlockKind::IfElse { condition } => condition_rows(condition),
    }
}

/// Height of the header part (everything above the first mouth).
pub fn header_height(kind: &BlockKind) -> f32 {
    if kind.is_hat() {
        return HAT_H;
    }
    let n = rows(kind).len() as f32;
    HEADER_H + n * ROW_H + if n > 0.0 { BOTTOM_PAD } else { 0.0 }
}

pub fn seq_height(blocks: &[Block]) -> f32 {
    blocks.iter().map(block_height).sum()
}

/// Full height of a block including anything nested inside it.
pub fn block_height(block: &Block) -> f32 {
    let mut h = header_height(&block.kind);
    for slot in 0..block.kind.slot_count() {
        if slot == 1 {
            h += MID_H;
        }
        h += seq_height(block.slot(slot)).max(EMPTY_SLOT_H);
    }
    if block.kind.slot_count() > 0 {
        h += FOOT_H;
    }
    h
}

fn field_rects(kind: &BlockKind, header: Rect) -> Vec<(Field, Rect)> {
    let mut out = Vec::new();
    for (ri, row) in rows(kind).into_iter().enumerate() {
        let cy = header.top() + HEADER_H + ri as f32 * ROW_H + ROW_H / 2.0 - 2.0;
        let indent = if kind.condition().is_some() { 8.0 } else { 0.0 };
        let mut x = header.left() + PAD_X + indent;
        for (field, w) in row {
            out.push((field, Rect::from_min_size(pos2(x, cy - FIELD_H / 2.0), vec2(w, FIELD_H))));
            x += w + GAP;
        }
    }
    out
}

/// The teal sensor area of a block with a condition (also the drop zone
/// for sensor chips).
pub fn condition_rect(kind: &BlockKind, header: Rect) -> Option<Rect> {
    let condition = kind.condition()?;
    let n = condition_rows(condition).len() as f32;
    Some(Rect::from_min_max(
        pos2(header.left() + 6.0, header.top() + HEADER_H - 5.0),
        pos2(header.right() - 6.0, header.top() + HEADER_H + n * ROW_H + 1.0),
    ))
}

fn motor_of(kind: &mut BlockKind) -> Option<&mut Motor> {
    match kind {
        BlockKind::RunMotor { motor, .. }
        | BlockKind::StopMotor { motor, .. }
        | BlockKind::ResetPosition { motor, .. }
        | BlockKind::MotorForTime { motor, .. }
        | BlockKind::MotorRotations { motor, .. } => Some(motor),
        _ => None,
    }
}

fn condition_group_mut(c: &mut Condition) -> &mut CardGroup {
    match c {
        Condition::ColorIs { group, .. }
        | Condition::Reflection { group, .. }
        | Condition::ButtonPressed { group, .. }
        | Condition::MotorPosition { group, .. }
        | Condition::LeverPosition { group, .. } => group,
    }
}

fn card_group_mut(kind: &mut BlockKind) -> Option<&mut CardGroup> {
    match kind {
        BlockKind::RunMotor { group, .. }
        | BlockKind::StopMotor { group, .. }
        | BlockKind::ResetPosition { group, .. }
        | BlockKind::MotorForTime { group, .. }
        | BlockKind::MotorRotations { group, .. } => Some(group),
        BlockKind::WaitUntil { condition } | BlockKind::If { condition } | BlockKind::IfElse { condition } => {
            Some(condition_group_mut(condition))
        }
        _ => None,
    }
}

fn card_group(kind: &BlockKind) -> Option<CardGroup> {
    if let Some(condition) = kind.condition() {
        Some(condition.target().group)
    } else {
        match kind {
            BlockKind::RunMotor { group, .. }
            | BlockKind::StopMotor { group, .. }
            | BlockKind::ResetPosition { group, .. }
            | BlockKind::MotorForTime { group, .. }
            | BlockKind::MotorRotations { group, .. } => Some(*group),
            _ => None,
        }
    }
}

fn card_group_rgb(group: CardGroup) -> Option<[u8; 3]> {
    match group {
        CardGroup::NoCard => None,
        CardGroup::Pink => Some([0xff, 0x69, 0xb4]),
        CardGroup::Purple => Some([0x74, 0x46, 0x99]),
        CardGroup::Blue => Some([0x00, 0x6c, 0xb8]),
        CardGroup::Azure => Some([0x32, 0xb4, 0xf5]),
        CardGroup::Turquoise => Some([0x00, 0xa3, 0xa3]),
        CardGroup::Green => Some([0x61, 0xa8, 0x36]),
        CardGroup::Yellow => Some([0xff, 0xd4, 0x00]),
        CardGroup::Orange => Some([0xff, 0x88, 0x00]),
        CardGroup::Red => Some([0xde, 0x1a, 0x21]),
        CardGroup::White => Some([0xf4, 0xf4, 0xf4]),
    }
}

fn paint_card_group_dot(painter: &Painter, rect: Rect, group: CardGroup, hovered: bool) {
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.38;
    let stroke = if hovered { theme::HIGHLIGHT } else { Color32::WHITE };
    if let Some([r, g, b]) = card_group_rgb(group) {
        painter.circle_filled(center, radius, Color32::from_rgb(r, g, b));
        let border = if group == CardGroup::White { theme::TEXT } else { stroke };
        painter.circle_stroke(center, radius, Stroke::new(1.5, border));
    } else {
        // An unassigned device is an empty white circle, not a wildcard.
        painter.circle_stroke(center, radius, Stroke::new(if hovered { 2.5 } else { 2.0 }, stroke));
    }
}

fn card_group_widget(ui: &mut Ui, header: Rect, id: u64, kind: &mut BlockKind) -> bool {
    let Some(group) = card_group_mut(kind) else { return false };
    let before = *group;
    let rect = card_group_dot_rect(header);
    let response = ui
        .interact(rect, egui::Id::new(("card_group", id)), Sense::click())
        .on_hover_text(format!("Card group: {}", group.label()));
    if response.hovered() {
        paint_card_group_dot(ui.painter(), rect, *group, true);
    }
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(130.0);
        for option in CardGroup::ALL {
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                paint_card_group_dot(ui.painter(), dot, option, false);
                ui.selectable_value(group, option, option.label());
            });
        }
    });
    *group != before
}

fn card_group_dot_rect(header: Rect) -> Rect {
    Rect::from_min_size(pos2(header.right() - 26.0, header.top() + 8.0), vec2(18.0, 18.0))
}

fn direction_of(kind: &mut BlockKind) -> Option<&mut Direction> {
    match kind {
        BlockKind::RunMotor { direction, .. }
        | BlockKind::MotorForTime { direction, .. }
        | BlockKind::MotorRotations { direction, .. } => Some(direction),
        _ => None,
    }
}

fn speed_of(kind: &mut BlockKind) -> Option<&mut i32> {
    match kind {
        BlockKind::RunMotor { speed, .. }
        | BlockKind::MotorForTime { speed, .. }
        | BlockKind::MotorRotations { speed, .. } => Some(speed),
        _ => None,
    }
}

fn seconds_of(kind: &mut BlockKind) -> Option<&mut f32> {
    match kind {
        BlockKind::MotorForTime { seconds, .. } | BlockKind::Wait { seconds } => Some(seconds),
        _ => None,
    }
}

fn cmp_of(c: &mut Condition) -> Option<&mut Comparison> {
    match c {
        Condition::Reflection { cmp, .. }
        | Condition::MotorPosition { cmp, .. }
        | Condition::LeverPosition { cmp, .. } => Some(cmp),
        _ => None,
    }
}

fn percent_of(c: &mut Condition) -> Option<(&mut i32, i32)> {
    match c {
        Condition::Reflection { percent, .. } => Some((percent, 0)),
        Condition::LeverPosition { percent, .. } => Some((percent, -100)),
        _ => None,
    }
}

fn field_text(kind: &BlockKind, field: Field) -> String {
    let mut k = kind.clone();
    let text = match field {
        Field::Motor => motor_of(&mut k).map(|m| m.label().to_string()),
        Field::Direction => direction_of(&mut k).map(|d| d.label().to_string()),
        Field::Speed => speed_of(&mut k).map(|s| format!("{s} %")),
        Field::Seconds => seconds_of(&mut k).map(|s| format!("{s:.1} s")),
        Field::Rotations => match k {
            BlockKind::MotorRotations { rotations, .. } => Some(format!("{rotations:.1}")),
            _ => None,
        },
        Field::Count => match k {
            BlockKind::Repeat { count } => Some(count.to_string()),
            _ => None,
        },
        Field::Text(t) => Some(t.to_string()),
        _ => k.condition_mut().map(|c| condition_field_text(c, field)),
    };
    text.unwrap_or_default()
}

fn condition_field_text(c: &mut Condition, field: Field) -> String {
    match (field, &*c) {
        (Field::CondKind, _) => c.kind().label().to_string(),
        (Field::CondColor, Condition::ColorIs { color, .. }) => color.label().to_string(),
        (Field::CondDevice, Condition::ButtonPressed { device, .. }) => device.label().to_string(),
        (Field::CondMotor, Condition::MotorPosition { motor, .. }) => motor.label().to_string(),
        (Field::CondLever, Condition::LeverPosition { lever, .. }) => lever.label().to_string(),
        (Field::CondDegrees, Condition::MotorPosition { degrees, .. }) => format!("{degrees}°"),
        (Field::CondCmp, _) => cmp_of(c).map(|x| x.symbol().to_string()).unwrap_or_default(),
        (Field::CondPercent, _) => percent_of(c).map(|(p, _)| format!("{p} %")).unwrap_or_default(),
        _ => String::new(),
    }
}

/// How a block should look this frame.
#[derive(Clone, Copy, Default)]
pub struct Look {
    pub selected: bool,
    pub active: bool,
    pub error: bool,
    /// Highlight the condition area as a drop target.
    pub drop_condition: bool,
    /// Draw field values as text (ghosts) instead of leaving room for widgets.
    pub static_fields: bool,
    /// 0..1 opacity multiplier.
    pub fade: f32,
}

impl Look {
    pub fn normal() -> Self {
        Self { fade: 1.0, ..Default::default() }
    }
}

fn cr(nw: u8, ne: u8, sw: u8, se: u8) -> CornerRadius {
    CornerRadius { nw, ne, sw, se }
}

/// The filled pieces of a block: one rect for a plain block, or
/// header/arm/bars for a C-block whose mouths span `slots` (y ranges).
fn pieces(rect: Rect, slots: &[(f32, f32)], kind: &BlockKind) -> Vec<(Rect, CornerRadius)> {
    let top = if kind.is_hat() { 22 } else { 8 };
    if slots.is_empty() {
        return vec![(rect, cr(top, top, 8, 8))];
    }
    let (l, r) = (rect.left(), rect.right());
    let mut out = vec![(Rect::from_min_max(rect.min, pos2(r, slots[0].0)), cr(top, top, 0, 6))];
    out.push((Rect::from_min_max(pos2(l, slots[0].0 - 1.0), pos2(l + ARM_W, slots[slots.len() - 1].1 + 1.0)), cr(0, 0, 0, 0)));
    for w in slots.windows(2) {
        out.push((Rect::from_min_max(pos2(l, w[0].1), pos2(r, w[1].0)), cr(0, 6, 0, 6)));
    }
    out.push((Rect::from_min_max(pos2(l, slots[slots.len() - 1].1), rect.max), cr(0, 6, 8, 8)));
    out
}

/// Paint the body, header, condition area and (optionally) static field values.
pub fn paint_block(painter: &Painter, rect: Rect, slots: &[(f32, f32)], kind: &BlockKind, look: Look) {
    let style = theme::category(kind.category());
    let a = look.fade;
    let parts = pieces(rect, slots, kind);

    // Containers (loops, ifs) are "active" while anything inside runs; give
    // them a quiet header marker so the inner running block stands out.
    let container = !slots.is_empty();
    if look.active && !container {
        let t = painter.ctx().input(|i| i.time) as f32;
        let pulse = 0.55 + 0.45 * (t * 6.0).sin().abs();
        for (p, r) in &parts {
            painter.rect_filled(p.expand(6.0), *r, theme::HIGHLIGHT.gamma_multiply(0.30 * pulse));
        }
        painter.ctx().request_repaint();
    }

    // Drop shadow gives a "brick" depth.
    for (p, r) in &parts {
        painter.rect_filled(p.translate(vec2(0.0, 3.0)), *r, style.dark.gamma_multiply(a));
    }
    for (p, r) in &parts {
        painter.rect_filled(*p, *r, style.fill.gamma_multiply(a));
    }

    if !kind.is_hat() {
        // Socket at the top that the block above plugs into.
        let notch = Rect::from_min_size(rect.left_top() + vec2(TAB_X, 0.0), vec2(TAB_W, TAB_H - 1.0));
        painter.rect_filled(notch, cr(0, 0, 4, 4), style.dark.gamma_multiply(0.6 * a));
    }

    let header = Rect::from_min_size(rect.min, vec2(rect.width(), header_height(kind)));
    let header_y = if kind.is_hat() { header.center().y + 3.0 } else { header.top() + HEADER_H / 2.0 + 2.0 };
    let text_col = style.text.gamma_multiply(a);
    painter.text(pos2(rect.left() + PAD_X + 4.0, header_y), Align2::LEFT_CENTER, kind.icon(), FontId::proportional(17.0), text_col);
    painter.text(
        pos2(rect.left() + PAD_X + 30.0, header_y),
        Align2::LEFT_CENTER,
        kind.title().to_uppercase(),
        FontId::proportional(15.5),
        text_col,
    );
    if slots.len() == 2 {
        painter.text(
            pos2(rect.left() + ARM_W + TAB_X + TAB_W + 10.0, (slots[0].1 + slots[1].0) / 2.0 + 2.0),
            Align2::LEFT_CENTER,
            "ELSE",
            FontId::proportional(15.5),
            text_col,
        );
    }
    if matches!(kind, BlockKind::Forever) || matches!(kind, BlockKind::Repeat { .. }) {
        let foot_y = rect.bottom() - FOOT_H / 2.0;
        painter.text(pos2(rect.right() - 14.0, foot_y), Align2::RIGHT_CENTER, "↻", FontId::proportional(15.0), text_col);
    }

    if let Some(cond) = condition_rect(kind, header) {
        let sensors = theme::category(Category::Sensors);
        painter.rect_filled(cond, CornerRadius::same(14), sensors.fill.gamma_multiply(a));
        if look.drop_condition {
            painter.rect_stroke(cond.expand(2.0), CornerRadius::same(15), Stroke::new(3.0, theme::HIGHLIGHT), StrokeKind::Outside);
        }
    }

    for (field, frect) in field_rects(kind, header) {
        let col = if field.is_condition() || kind.condition().is_some() { Color32::WHITE.gamma_multiply(a) } else { text_col };
        match field {
            Field::Text(t) => {
                painter.text(frect.left_center(), Align2::LEFT_CENTER, t, FontId::proportional(14.0), col);
            }
            _ if look.static_fields => {
                painter.rect_filled(frect, CornerRadius::same(12), Color32::WHITE.gamma_multiply(a));
                painter.text(
                    frect.left_center() + vec2(9.0, 0.0),
                    Align2::LEFT_CENTER,
                    field_text(kind, field),
                    FontId::proportional(13.5),
                    theme::TEXT.gamma_multiply(a),
                );
            }
            _ => {}
        }
    }

    if let Some(group) = card_group(kind) {
        paint_card_group_dot(painter, card_group_dot_rect(header), group, false);
    }

    if look.active && container {
        let (header_part, r) = parts[0];
        painter.rect_stroke(header_part.expand(1.0), r, Stroke::new(2.0, theme::HIGHLIGHT), StrokeKind::Outside);
    }
    let outline = if look.error {
        Some(Stroke::new(3.0, theme::ERROR))
    } else if look.active && !container {
        Some(Stroke::new(3.5, theme::HIGHLIGHT))
    } else if look.selected {
        Some(Stroke::new(2.5, theme::SELECT))
    } else {
        None
    };
    if let Some(stroke) = outline {
        for (p, r) in &parts {
            painter.rect_stroke(p.expand(1.0), *r, stroke, StrokeKind::Outside);
        }
    }
}

fn paint_tab_at(painter: &Painter, left_x: f32, top_y: f32, kind: &BlockKind, fade: f32) {
    let style = theme::category(kind.category());
    let tab = Rect::from_min_size(pos2(left_x + TAB_X, top_y - 1.0), vec2(TAB_W, TAB_H));
    painter.rect_filled(tab.translate(vec2(0.0, 2.0)), cr(0, 0, 5, 5), style.dark.gamma_multiply(fade));
    painter.rect_filled(tab, cr(0, 0, 5, 5), style.fill.gamma_multiply(fade));
}

/// Connector tabs: below the block (unless it is a cap) and at the top of
/// each mouth. Painted after the blocks they plug into.
pub fn paint_tabs(painter: &Painter, rect: Rect, slots: &[(f32, f32)], kind: &BlockKind, fade: f32) {
    if !kind.is_cap() {
        paint_tab_at(painter, rect.left(), rect.bottom(), kind, fade);
    }
    for (top, _) in slots {
        paint_tab_at(painter, rect.left() + ARM_W, *top, kind, fade);
    }
}

fn field_visuals(ui: &mut Ui) {
    let v = ui.visuals_mut();
    v.override_text_color = Some(theme::TEXT);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.bg_fill = Color32::WHITE;
        w.weak_bg_fill = Color32::WHITE;
        w.corner_radius = CornerRadius::same(12);
        w.fg_stroke.color = theme::TEXT;
    }
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.bg_stroke = Stroke::new(2.0, Color32::from_black_alpha(60));
}

fn combo<T: PartialEq + Copy>(
    ui: &mut Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    width: f32,
    value: &mut T,
    options: &[T],
    label: impl Fn(T) -> String,
) {
    egui::ComboBox::from_id_salt(salt)
        .width(width - 8.0)
        .selected_text(label(*value))
        .show_ui(ui, |ui| {
            for &o in options {
                ui.selectable_value(value, o, label(o));
            }
        });
}

/// Interactive parameter editors placed on top of a painted block header.
/// Returns true if anything changed.
pub fn field_widgets(ui: &mut Ui, header: Rect, id: u64, kind: &mut BlockKind) -> bool {
    let before = kind.clone();
    for (i, (field, frect)) in field_rects(kind, header).into_iter().enumerate() {
        let salt = ("field", id, i);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(frect).id_salt(salt));
        field_visuals(&mut child);
        let ui = &mut child;
        let w = frect.width();
        match field {
            Field::Text(_) => {}
            Field::Motor => {
                if let Some(m) = motor_of(kind) {
                    combo(ui, salt, w, m, &Motor::ALL, |m| m.label().into());
                }
            }
            Field::Direction => {
                if let Some(d) = direction_of(kind) {
                    combo(ui, salt, w, d, &[Direction::Clockwise, Direction::CounterClockwise], |d| d.label().into());
                }
            }
            Field::Speed => {
                if let Some(speed) = speed_of(kind) {
                    ui.add_sized(frect.size(), DragValue::new(speed).range(0..=100).speed(0.5).suffix(" %"));
                }
            }
            Field::Seconds => {
                if let Some(s) = seconds_of(kind) {
                    ui.add_sized(
                        frect.size(),
                        DragValue::new(s).range(0.0..=MAX_SECONDS).speed(0.05).fixed_decimals(1).suffix(" s"),
                    );
                }
            }
            Field::Rotations => {
                if let BlockKind::MotorRotations { rotations, .. } = kind {
                    ui.add_sized(frect.size(), DragValue::new(rotations).range(0.0..=MAX_ROTATIONS).speed(0.05).fixed_decimals(1));
                }
            }
            Field::Count => {
                if let BlockKind::Repeat { count } = kind {
                    ui.add_sized(frect.size(), DragValue::new(count).range(0..=MAX_REPEAT).speed(0.1));
                }
            }
            _ => {
                if let Some(c) = kind.condition_mut() {
                    condition_widget(ui, salt, frect, field, c);
                }
            }
        }
    }
    let group_changed = card_group_widget(ui, header, id, kind);
    kind.sanitize();
    *kind != before || group_changed
}

fn condition_widget(ui: &mut Ui, salt: (&str, u64, usize), frect: Rect, field: Field, c: &mut Condition) {
    let w = frect.width();
    match field {
        Field::CondKind => {
            let mut k = c.kind();
            combo(ui, salt, w, &mut k, &ConditionKind::ALL, |k| k.label().into());
            if k != c.kind() {
                *c = k.default_condition();
            }
        }
        Field::CondCmp => {
            if let Some(cmp) = cmp_of(c) {
                combo(ui, salt, w, cmp, &[Comparison::Less, Comparison::Greater], |x| x.symbol().into());
            }
        }
        Field::CondPercent => {
            if let Some((p, min)) = percent_of(c) {
                ui.add_sized(frect.size(), DragValue::new(p).range(min..=100).speed(0.5).suffix(" %"));
            }
        }
        _ => match c {
            Condition::ColorIs { color, .. } => {
                let colors: Vec<LegoColor> = LegoColor::ALL.into_iter().filter(|c| *c != LegoColor::None).collect();
                combo(ui, salt, w, color, &colors, |c| c.label().into());
            }
            Condition::ButtonPressed { device, .. } => combo(ui, salt, w, device, &DeviceKind::ALL, |d| d.label().into()),
            Condition::MotorPosition { motor, degrees, .. } => {
                if field == Field::CondMotor {
                    combo(ui, salt, w, motor, &[Motor::Single, Motor::DoubleLeft, Motor::DoubleRight], |m| m.label().into());
                } else {
                    ui.add_sized(frect.size(), DragValue::new(degrees).range(-MAX_DEGREES..=MAX_DEGREES).speed(1.0).suffix("°"));
                }
            }
            Condition::LeverPosition { lever, .. } => {
                combo(ui, salt, w, lever, &[Lever::Left, Lever::Right], |l| l.label().into());
            }
            Condition::Reflection { .. } => {}
        },
    }
}

/// Compact palette entry for a block.
pub fn paint_chip(painter: &Painter, rect: Rect, kind: &BlockKind, hovered: bool) {
    let style = theme::category(kind.category());
    let r = CornerRadius::same(if kind.is_hat() { 14 } else { 8 });
    painter.rect_filled(rect.translate(vec2(0.0, 2.0)), r, style.dark);
    let fill = if hovered { style.fill.lerp_to_gamma(Color32::WHITE, 0.15) } else { style.fill };
    painter.rect_filled(rect, r, fill);
    painter.text(rect.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, kind.icon(), FontId::proportional(15.0), style.text);
    painter.text(rect.left_center() + vec2(34.0, 0.0), Align2::LEFT_CENTER, kind.title(), FontId::proportional(14.5), style.text);
}

/// A sensor condition pill (palette chip and drag ghost).
pub fn paint_condition_chip(painter: &Painter, rect: Rect, text: &str, hovered: bool, fade: f32) {
    let style = theme::category(Category::Sensors);
    let r = CornerRadius::same((rect.height() / 2.0) as u8);
    painter.rect_filled(rect.translate(vec2(0.0, 2.0)), r, style.dark.gamma_multiply(fade));
    let fill = if hovered { style.fill.lerp_to_gamma(Color32::WHITE, 0.15) } else { style.fill };
    painter.rect_filled(rect, r, fill.gamma_multiply(fade));
    painter.text(rect.left_center() + vec2(14.0, 0.0), Align2::LEFT_CENTER, text, FontId::proportional(14.5), style.text.gamma_multiply(fade));
}
