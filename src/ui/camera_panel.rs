//! The Camera pane: live picture with the body points, what the camera
//! sees, and teaching it your own poses.

use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

use crate::camera::pose::{MAX_CLASSES, MAX_EXAMPLES, MIN_SCORE, SKELETON};
use crate::camera::{BodyPoint, Camera, CameraPose, CameraStatus, Detection, Pose, PoseClass};

use super::theme;

pub enum CameraAction {
    TurnOn,
    TurnOff,
    AddPose,
    /// The name of a pose was edited.
    Renamed,
    ClearPose(usize),
    DeletePose(usize),
    /// The Record button of this pose is held down.
    Record(usize),
}

/// Live picture of the camera, uploaded to the GPU when a new one arrives,
/// and the two body points being measured.
pub struct Preview {
    texture: Option<egui::TextureHandle>,
    frame_number: u64,
    measure: (BodyPoint, BodyPoint),
}

impl Default for Preview {
    fn default() -> Self {
        Preview { texture: None, frame_number: 0, measure: (BodyPoint::LeftWrist, BodyPoint::RightWrist) }
    }
}

fn card(ui: &mut Ui, highlight: bool, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(if highlight { theme::HIGHLIGHT.gamma_multiply(0.35) } else { theme::BG })
        .stroke(if highlight { Stroke::new(2.0, theme::HIGHLIGHT) } else { Stroke::NONE })
        .corner_radius(10)
        .inner_margin(8)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    ui.add_space(4.0);
}

/// Draw the stick figure. The picture is shown mirrored, like a mirror, so
/// the body points are too.
fn paint_skeleton(painter: &egui::Painter, to_screen: impl Fn(f32, f32) -> Pos2, pose: &Pose, width: f32, color: Color32) {
    let seen = |i: usize| pose.points.get(i).filter(|k| k.score >= MIN_SCORE);
    for (a, b) in SKELETON {
        if let (Some(a), Some(b)) = (seen(a), seen(b)) {
            painter.line_segment([to_screen(a.x, a.y), to_screen(b.x, b.y)], Stroke::new(width, color));
        }
    }
    for i in 0..pose.points.len() {
        if let Some(k) = seen(i) {
            painter.circle_filled(to_screen(k.x, k.y), width * 1.2, Color32::WHITE);
            painter.circle_stroke(to_screen(k.x, k.y), width * 1.2, Stroke::new(1.0, color));
        }
    }
}

/// A small stick figure of a recorded example, scaled to fit.
fn paint_thumbnail(ui: &mut Ui, pose: Option<&Pose>) {
    let (rect, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 6, Color32::WHITE);
    let Some(pose) = pose else {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, "?", egui::FontId::proportional(16.0), theme::TEXT_MUTED);
        return;
    };
    let seen: Vec<_> = pose.points.iter().filter(|k| k.score >= MIN_SCORE).collect();
    if seen.is_empty() {
        return;
    }
    let (min_x, max_x) = seen.iter().fold((f32::MAX, f32::MIN), |(lo, hi), k| (lo.min(k.x), hi.max(k.x)));
    let (min_y, max_y) = seen.iter().fold((f32::MAX, f32::MIN), |(lo, hi), k| (lo.min(k.y), hi.max(k.y)));
    let inner = rect.shrink(4.0);
    let span = ((max_x - min_x) * pose.aspect).max(max_y - min_y).max(0.01);
    let scale = inner.width() / span;
    let centre = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
    let to_screen = |x: f32, y: f32| {
        inner.center() + vec2(-(x - centre.0) * pose.aspect * scale, (y - centre.1) * scale)
    };
    paint_skeleton(painter, to_screen, pose, 1.5, theme::category(crate::visual::Category::Sensors).fill);
}

fn preview(ui: &mut Ui, camera: &Camera, view: &mut Preview, status: &CameraStatus) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, width * 0.75), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 10, Color32::from_rgb(0x1F, 0x29, 0x37));

    let (frame, pose) = camera.with_view(|v| {
        let fresh = v.frame_number != view.frame_number;
        view.frame_number = v.frame_number;
        (v.frame.clone().filter(|_| fresh), v.pose.clone())
    });
    if let Some(frame) = frame {
        let image = egui::ColorImage::from_rgb([frame.width, frame.height], &frame.rgb);
        match &mut view.texture {
            Some(t) => t.set(image, egui::TextureOptions::LINEAR),
            None => view.texture = Some(ui.ctx().load_texture("camera_preview", image, egui::TextureOptions::LINEAR)),
        }
    }
    let on = matches!(status, CameraStatus::On { .. });
    if !on {
        view.texture = None;
        let text = match status {
            CameraStatus::Starting => "Starting the camera…",
            _ => "📷  The camera is off",
        };
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(16.0), Color32::WHITE);
        return;
    }
    let Some(texture) = &view.texture else { return };
    // Fit the picture into the box, keeping its shape.
    let [tw, th] = texture.size().map(|v| v as f32);
    let scale = (rect.width() / tw).min(rect.height() / th);
    let image_rect = Rect::from_center_size(rect.center(), vec2(tw, th) * scale);
    // Mirrored: flip the texture horizontally.
    let uv = Rect::from_min_max(pos2(1.0, 0.0), pos2(0.0, 1.0));
    painter.image(texture.id(), image_rect, uv, Color32::WHITE);
    if let Some(pose) = pose.filter(Pose::has_person) {
        let to_screen = |x: f32, y: f32| pos2(image_rect.right() - x * image_rect.width(), image_rect.top() + y * image_rect.height());
        paint_skeleton(&painter, to_screen, &pose, 3.0, theme::HIGHLIGHT);
        // The measured points, joined by a line.
        let (one, two) = view.measure;
        let seen = |p: BodyPoint| pose.points.get(p.index()).filter(|k| k.score >= MIN_SCORE).map(|k| to_screen(k.x, k.y));
        let colour = theme::category(crate::visual::Category::Sensors).fill;
        if let (Some(a), Some(b)) = (seen(one), seen(two)) {
            painter.line_segment([a, b], Stroke::new(2.5, colour));
        }
        for p in [seen(one), seen(two)].into_iter().flatten() {
            painter.circle_filled(p, 6.0, colour);
            painter.circle_stroke(p, 6.0, Stroke::new(2.0, Color32::WHITE));
        }
    }
}

/// Live body-sensing values for two chosen points, to help pick the
/// numbers for Body distance / angle / position blocks.
fn measure(ui: &mut Ui, view: &mut Preview, pose: Option<&Pose>) {
    ui.label(RichText::new("MEASURE").size(12.5).strong().color(theme::TEXT_MUTED));
    card(ui, false, |ui| {
        let (one, two) = &mut view.measure;
        for (salt, point) in [("measure_one", one), ("measure_two", two)] {
            egui::ComboBox::from_id_salt(salt).selected_text(point.label()).width(130.0).show_ui(ui, |ui| {
                for p in BodyPoint::ALL {
                    ui.selectable_value(point, p, p.label());
                }
            });
        }
        let (one, two) = view.measure;
        let number = |v: Option<f32>, unit: &str| v.map_or("–".to_string(), |v| format!("{}{unit}", v.floor() as i64));
        egui::Grid::new("measure_grid").num_columns(2).show(ui, |ui| {
            let row = |ui: &mut Ui, name: &str, value: String| {
                ui.label(RichText::new(name).color(theme::TEXT_MUTED));
                ui.label(RichText::new(value).strong());
                ui.end_row();
            };
            for p in [one, two] {
                let x = number(pose.and_then(|q| q.coordinate(crate::camera::Axis::X, p)), "");
                let y = number(pose.and_then(|q| q.coordinate(crate::camera::Axis::Y, p)), "");
                row(ui, p.label(), format!("x {x}   y {y}"));
            }
            row(ui, "distance", number(pose.and_then(|q| q.distance(one, two)), ""));
            row(ui, "angle", number(pose.and_then(|q| q.angle(one, two)), "°"));
            let touching = pose.is_some_and(|q| q.touching(one, two));
            row(ui, "touching", if touching { "yes".into() } else { "no".into() });
        });
    });
}

fn sees_text(detection: &Detection, names: &[String]) -> String {
    if !detection.person {
        return "Sees: nobody".into();
    }
    let mut parts = vec!["a person".to_string()];
    if let Some(pose) = detection.builtin {
        parts.push(pose.label(names));
    }
    if let Some(i) = detection.custom {
        parts.push(CameraPose::Custom(i as u8).label(names));
    }
    format!("Sees: {}", parts.join(" · "))
}

pub fn camera_panel(ui: &mut Ui, camera: &Camera, view: &mut Preview, poses: &mut [PoseClass]) -> Vec<CameraAction> {
    let mut actions = Vec::new();
    let status = camera.status();
    let on = camera.is_on();
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("📷").size(16.0));
        ui.label(RichText::new("CAMERA").size(16.0).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if on {
                if ui.button("Turn off").clicked() {
                    actions.push(CameraAction::TurnOff);
                }
            } else if ui
                .button(RichText::new("Turn on").strong())
                .on_hover_text("Use the computer's camera. Pictures stay on this computer.")
                .clicked()
            {
                actions.push(CameraAction::TurnOn);
            }
            if matches!(status, CameraStatus::Starting) {
                ui.spinner();
            }
        });
    });
    ui.add_space(4.0);
    preview(ui, camera, view, &status);

    let names: Vec<String> = poses.iter().map(|p| p.name.clone()).collect();
    let detection = if matches!(status, CameraStatus::On { .. }) { camera.with_view(|v| v.detection.clone()) } else { Detection::default() };
    match &status {
        CameraStatus::On { name } => {
            ui.label(RichText::new(sees_text(&detection, &names)).strong()).on_hover_text(name);
        }
        CameraStatus::Failed(e) => {
            ui.label(RichText::new(format!("Camera problem: {e}")).color(theme::ERROR));
        }
        CameraStatus::Starting => {
            ui.label(RichText::new("Starting…").color(theme::TEXT_MUTED));
        }
        CameraStatus::Off => {
            ui.label(RichText::new("Turn the camera on to see what it sees.").color(theme::TEXT_MUTED));
        }
    }
    ui.add_space(6.0);

    egui::ScrollArea::vertical().id_salt("camera_scroll").auto_shrink(false).show(ui, |ui| {
        ui.label(RichText::new("BUILT-IN").size(12.5).strong().color(theme::TEXT_MUTED));
        ui.horizontal_wrapped(|ui| {
            for pose in CameraPose::BUILTIN {
                let lit = on && detection.matches(pose);
                let text = RichText::new(pose.label(&names)).color(if lit { theme::TEXT } else { theme::TEXT_MUTED });
                egui::Frame::new()
                    .fill(if lit { theme::HIGHLIGHT } else { theme::BG })
                    .corner_radius(12)
                    .inner_margin(egui::Margin::symmetric(8, 3))
                    .show(ui, |ui| ui.label(text));
            }
        });
        ui.add_space(8.0);
        measure(ui, view, detection.pose.as_ref().filter(|_| on));
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("MY POSES ({}/{MAX_CLASSES})", poses.len())).size(12.5).strong().color(theme::TEXT_MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(poses.len() < MAX_CLASSES, egui::Button::new("＋ Add pose")).clicked() {
                    actions.push(CameraAction::AddPose);
                }
            });
        });
        if poses.len() < 2 {
            ui.label(
                RichText::new(
                    "Teach the camera your own poses: add at least two (for example \"star\" and \"standing\"), \
                     then hold Record while you do each one.",
                )
                .color(theme::TEXT_MUTED),
            );
        }
        for (i, class) in poses.iter_mut().enumerate() {
            let lit = on && detection.custom == Some(i);
            card(ui, lit, |ui| {
                ui.horizontal(|ui| {
                    paint_thumbnail(ui, class.examples.first());
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            let name = egui::TextEdit::singleline(&mut class.name).desired_width(ui.available_width() - 34.0).char_limit(40);
                            if ui.add(name).changed() {
                                actions.push(CameraAction::Renamed);
                            }
                            if ui.small_button("✕").on_hover_text("Delete this pose").clicked() {
                                actions.push(CameraAction::DeletePose(i));
                            }
                        });
                        ui.horizontal(|ui| {
                            let record = egui::Button::new(RichText::new("● Record").color(theme::STOP_RED).strong())
                                .sense(Sense::click_and_drag());
                            let full = class.examples.len() >= MAX_EXAMPLES;
                            let resp = ui
                                .add_enabled(on && !full, record)
                                .on_hover_text("Hold while you do the pose. Move a little to give it different examples.")
                                .on_disabled_hover_text(if full { "This pose has as many examples as it can hold." } else { "Turn the camera on first." });
                            if resp.is_pointer_button_down_on() {
                                actions.push(CameraAction::Record(i));
                            }
                            ui.label(RichText::new(format!("{} examples", class.examples.len())).color(theme::TEXT_MUTED));
                            if !class.examples.is_empty() && ui.small_button("Clear").on_hover_text("Forget this pose's examples").clicked() {
                                actions.push(CameraAction::ClearPose(i));
                            }
                        });
                    });
                });
            });
        }
    });
    actions
}
