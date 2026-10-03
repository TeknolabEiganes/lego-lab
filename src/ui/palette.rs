//! Block palette on the left.

use egui::{Align2, FontId, Sense, Ui, vec2};

use crate::program::condition::ConditionKind;
use crate::visual::{BlockKind, Category};

use super::block;
use super::canvas::{Drag, Editor, Payload};
use super::theme;

fn description(kind: &BlockKind) -> &'static str {
    match kind {
        BlockKind::WhenRun => "Your program starts here when you press RUN.",
        BlockKind::RunMotor { .. } => "Start a motor and keep it running.",
        BlockKind::StopMotor { .. } => "Stop a motor.",
        BlockKind::ResetPosition { .. } => "Count the motor's position from 0° again (it also starts at 0° on RUN).",
        BlockKind::MotorForTime { .. } => "Run a motor for some seconds, then stop.",
        BlockKind::MotorRotations { .. } => "Turn a motor a number of rotations, then stop.",
        BlockKind::Wait { .. } => "Wait before doing the next block.",
        BlockKind::WaitUntil { .. } => "Wait until a sensor condition is true.",
        BlockKind::Repeat { .. } => "Do the blocks inside a number of times.",
        BlockKind::Forever => "Do the blocks inside again and again, until STOP.",
        BlockKind::If { .. } => "Do the blocks inside only if the condition is true.",
        BlockKind::IfElse { .. } => "Do the first blocks if the condition is true, otherwise the second.",
    }
}

fn sensor_description(kind: ConditionKind) -> &'static str {
    match kind {
        ConditionKind::Color => "Colour Sensor sees a colour.",
        ConditionKind::Reflection => "How much light the Colour Sensor sees reflected.",
        ConditionKind::Button => "The button on a device is pressed.",
        ConditionKind::MotorPosition => "How far a motor has turned, in degrees.",
        ConditionKind::Lever => "Where a Controller lever is pushed (−100 to 100%).",
    }
}

fn heading(ui: &mut Ui, cat: Category) {
    let style = theme::category(cat);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 5.0, style.fill);
        ui.label(egui::RichText::new(style.name).size(12.5).strong().color(theme::TEXT_MUTED));
    });
}

pub fn show(ui: &mut Ui, ed: &mut Editor, icons: &super::icons::IconTextures) {
    let palette = BlockKind::palette();
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
            icons.puzzle(ui.painter(), rect.shrink(2.0), theme::TEXT);
            ui.label(egui::RichText::new("BLOCKS").size(16.0).strong());
        });
        ui.add_space(6.0);
        for cat in [Category::Events, Category::Motors, Category::Control] {
            heading(ui, cat);
            for kind in palette.iter().filter(|k| k.category() == cat) {
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width() - 6.0, 34.0), Sense::click_and_drag());
                block::paint_chip(ui.painter(), rect, kind, resp.hovered());
                let resp = resp.on_hover_text(description(kind));
                if resp.hovered() && ed.drag.is_none() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                if resp.drag_started() {
                    let block = ed.ws.make_block(kind.clone());
                    ed.drag = Some(Drag { payload: Payload::Blocks(vec![block]), grab: vec2(40.0, 18.0), from_palette: true });
                }
                ui.add_space(4.0);
            }
            ui.add_space(10.0);
        }

        heading(ui, Category::Sensors);
        for kind in ConditionKind::ALL {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width() - 6.0, 32.0), Sense::click_and_drag());
            block::paint_condition_chip(ui.painter(), rect, kind.label(), resp.hovered(), 1.0);
            let resp = resp.on_hover_text(format!(
                "{}\nDrop it on an If, If / Else or Wait Until block.",
                sensor_description(kind)
            ));
            if resp.hovered() && ed.drag.is_none() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            if resp.drag_started() {
                ed.drag = Some(Drag {
                    payload: Payload::Condition(kind.default_condition()),
                    grab: vec2(40.0, 16.0),
                    from_palette: true,
                });
            }
            ui.add_space(4.0);
        }
    });

    // Dragging over the palette deletes.
    if ed.drag.is_some()
        && let Some(p) = ui.ctx().pointer_latest_pos()
        && ed.palette_rect.contains(p)
    {
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("trash")));
        painter.rect_filled(ed.palette_rect, 0.0, theme::STOP_RED.gamma_multiply(0.12));
        painter.text(ed.palette_rect.center(), Align2::CENTER_CENTER, "🗑  Drop to delete", FontId::proportional(18.0), theme::STOP_RED);
    }
}
