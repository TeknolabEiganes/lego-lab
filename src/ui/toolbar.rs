//! Top bar: logo, project, connection, RUN / STOP.

use egui::{Button, Color32, CornerRadius, RichText, Sense, Stroke, Ui, vec2};

use crate::hardware::ConnectionState;

use super::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Simulator,
    Bluetooth,
}

impl Backend {
    pub fn label(self) -> &'static str {
        match self {
            Backend::Simulator => "Simulator",
            Backend::Bluetooth => "LEGO Bluetooth",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarAction {
    New,
    Open,
    Save,
    SaveAs,
    Connect,
    Disconnect,
    SelectBackend(Backend),
    Run,
    Stop,
    ToggleSimulator,
}

pub struct ToolbarState<'a> {
    pub project_name: &'a mut String,
    pub dirty: bool,
    pub connection: ConnectionState,
    pub backend: Backend,
    pub running: bool,
    pub show_simulator: bool,
}

fn logo(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(34.0, 26.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect.translate(vec2(0.0, 2.0)), 4, Color32::from_rgb(0xA8, 0x00, 0x08));
    p.rect_filled(rect, 4, theme::LEGO_RED);
    for i in 0..2 {
        let c = rect.center() + vec2(-8.0 + 16.0 * i as f32, 0.0);
        p.circle_filled(c + vec2(0.0, 1.5), 5.5, Color32::from_rgb(0xA8, 0x00, 0x08));
        p.circle_filled(c, 5.5, Color32::from_rgb(0xFF, 0x3B, 0x3B));
    }
}

fn big_button(ui: &mut Ui, text: &str, fill: Color32, enabled: bool) -> bool {
    let text = RichText::new(text).size(16.0).strong().color(Color32::WHITE);
    let button = Button::new(text)
        .fill(if enabled { fill } else { theme::OFF_GREY })
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(12))
        .min_size(vec2(108.0, 38.0));
    ui.add_enabled(enabled, button).clicked()
}

pub fn connection_dot(ui: &mut Ui, state: ConnectionState) {
    connection_indicator(ui, state, true);
}

fn connection_indicator(ui: &mut Ui, state: ConnectionState, with_text: bool) {
    let (color, text) = match state {
        ConnectionState::Connected => (theme::OK_GREEN, "Connected"),
        ConnectionState::Connecting => (theme::WARN_AMBER, "Connecting…"),
        ConnectionState::Disconnected => (theme::OFF_GREY, "Not connected"),
    };
    let (r, resp) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
    ui.painter().circle_filled(r.center(), 6.0, color);
    if with_text {
        ui.label(RichText::new(text).size(14.5));
    } else {
        resp.on_hover_text(text);
    }
}

pub fn show(ui: &mut Ui, st: ToolbarState) -> Vec<ToolbarAction> {
    let mut actions = Vec::new();
    ui.horizontal_centered(|ui| {
        ui.add_space(6.0);
        logo(ui);
        ui.label(RichText::new("LEGO LAB").size(21.0).strong().color(theme::TEXT));
        ui.add_space(12.0);

        ui.menu_button(RichText::new("☰ File").size(14.5), |ui| {
            if ui.button("New").clicked() {
                actions.push(ToolbarAction::New);
            }
            if ui.button("Open…").clicked() {
                actions.push(ToolbarAction::Open);
            }
            if ui.button("Save").clicked() {
                actions.push(ToolbarAction::Save);
            }
            if ui.button("Save As…").clicked() {
                actions.push(ToolbarAction::SaveAs);
            }
        });
        let wide = ui.available_width() > 1100.0;
        if ui.available_width() > 820.0 {
            ui.add(
                egui::TextEdit::singleline(st.project_name)
                    .desired_width(if wide { 160.0 } else { 110.0 })
                    .hint_text("Project name"),
            );
        }
        if st.dirty {
            ui.label(RichText::new("•").size(20.0).color(theme::TEXT_MUTED))
                .on_hover_text("Unsaved changes");
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            if big_button(ui, "■  STOP", theme::STOP_RED, true) {
                actions.push(ToolbarAction::Stop);
            }
            if big_button(ui, "▶  RUN", theme::RUN_GREEN, !st.running) {
                actions.push(ToolbarAction::Run);
            }
            ui.add_space(16.0);

            let panel = if st.backend == Backend::Simulator { "simulator" } else { "devices" };
            let panel_text = match (st.show_simulator, wide) {
                (true, true) => format!("Hide {panel}"),
                (false, true) => format!("Show {panel}"),
                _ => panel[..1].to_uppercase() + &panel[1..],
            };
            if ui.button(panel_text).clicked() {
                actions.push(ToolbarAction::ToggleSimulator);
            }
            match st.connection {
                ConnectionState::Disconnected => {
                    if ui.button(RichText::new("Connect").strong()).clicked() {
                        actions.push(ToolbarAction::Connect);
                    }
                }
                _ => {
                    if ui.button("Disconnect").clicked() {
                        actions.push(ToolbarAction::Disconnect);
                    }
                }
            }
            egui::ComboBox::from_id_salt("backend")
                .selected_text(st.backend.label())
                .show_ui(ui, |ui| {
                    for b in [Backend::Simulator, Backend::Bluetooth] {
                        if ui.selectable_label(st.backend == b, b.label()).clicked() && st.backend != b {
                            actions.push(ToolbarAction::SelectBackend(b));
                        }
                    }
                });
            connection_indicator(ui, st.connection, wide);
        });
    });
    actions
}
