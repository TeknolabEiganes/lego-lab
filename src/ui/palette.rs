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
        BlockKind::RunMotor { .. } => {
            "Start a motor and keep it running. Tap the direction or speed/power to switch them."
        }
        BlockKind::StopMotor { .. } => "Stop a motor.",
        BlockKind::ResetPosition { .. } => "Count the motor's position from 0° again (it also starts at 0° on RUN).",
        BlockKind::MotorForTime { .. } => {
            "Run a motor for some seconds, then stop. Tap the direction or speed/power to switch them."
        }
        BlockKind::MotorRotations { .. } => "Turn a motor a number of rotations, then stop.",
        BlockKind::SetAcceleration { .. } => {
            "How quickly a motor speeds up and slows down (100% = quickest). \
             Applies to the motor blocks that come after it."
        }
        BlockKind::Beep { .. } => {
            "Play a beep pattern on devices in the selected card group, on this computer, or both \
             (tap the dot at the top right). Pick the note and how many times."
        }
        BlockKind::PlayTune { .. } => {
            "Play a short tune on devices in the selected card group, on this computer, or both (tap the dot at the top right)."
        }
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
        ConditionKind::Camera => {
            "The computer's camera sees a person, raised arms, or a pose you taught it in the Camera pane."
        }
        ConditionKind::BodyTouching => "The camera sees two body points touch, e.g. your hands (wrists) together.",
        ConditionKind::BodyDistance => {
            "How far apart two body points are on the camera (the picture is 90 tall). The Camera pane shows live values."
        }
        ConditionKind::BodyAngle => {
            "Which way one body point is from another on the camera: 0° right, 90° up, 180° left, 270° down."
        }
        ConditionKind::BodyPosition => {
            "Where a body point is on the camera: 0 is the middle, y is 45 at the top and −45 at the bottom."
        }
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
        for cat in [Category::Events, Category::Motors, Category::Sounds, Category::Control] {
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
