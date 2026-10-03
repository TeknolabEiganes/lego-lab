//! Status bar (connected devices) and the simulator panel.

use egui::{Color32, RichText, Sense, Stroke, Ui, vec2};

use crate::hardware::lego::FoundDevice;
use crate::hardware::mock::{MockDevice, MockMotor, MockState};
use crate::hardware::{AttachedDevice, CardGroup, ConnectionState, DeviceKind, DeviceSnapshot, LegoColor};

use super::icons::{self, IconTextures};
use super::theme;
use super::toolbar::{Backend, ToolbarAction};

pub struct StatusLine {
    pub text: String,
    pub color: Color32,
}

pub fn status_bar(
    ui: &mut Ui,
    backend: &str,
    connection: ConnectionState,
    devices: &[AttachedDevice],
    status: Option<&StatusLine>,
    mode: Backend,
    show_blocks: bool,
    show_right_panel: bool,
    icons: &IconTextures,
) -> Vec<ToolbarAction> {
    let mut actions = Vec::new();
    ui.horizontal_centered(|ui| {
        ui.add_space(6.0);
        let blocks = ui.add_sized(
            vec2(28.0, 24.0),
            egui::Button::new("").fill(Color32::TRANSPARENT).stroke(Stroke::NONE),
        );
        let blocks_color = if show_blocks { theme::TEXT } else { theme::TEXT_MUTED };
        icons.puzzle(ui.painter(), egui::Rect::from_center_size(blocks.rect.center(), vec2(16.0, 16.0)), blocks_color);
        if blocks.on_hover_text("Blocks pane (Ctrl/Cmd+1; Ctrl/Cmd+F toggles both)").clicked() {
            actions.push(ToolbarAction::ToggleBlocks);
        }
        ui.add_space(8.0);
        let dot = match connection {
            ConnectionState::Connected => theme::OK_GREEN,
            ConnectionState::Connecting => theme::WARN_AMBER,
            ConnectionState::Disconnected => theme::OFF_GREY,
        };
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 5.0, dot);
        ui.label(RichText::new(backend).strong());
        ui.add_space(14.0);
        // In narrow windows show just the dots (names on hover) so the
        // status message has room.
        let compact = ui.available_width() < 1000.0;
        for kind in DeviceKind::ALL {
            let found = devices.iter().any(|d| d.kind == kind);
            let hover = format!("{}: {}", kind.label(), if found { "connected" } else { "not connected" });
            let (r, dot) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            if found {
                ui.painter().circle_filled(r.center(), 4.5, theme::OK_GREEN);
            } else {
                ui.painter().circle_stroke(r.center(), 4.0, Stroke::new(1.5, theme::OFF_GREY));
            }
            if compact {
                dot.on_hover_text(hover);
                ui.add_space(2.0);
            } else {
                let text = RichText::new(kind.label()).color(if found { theme::TEXT } else { theme::TEXT_MUTED });
                ui.label(text).on_hover_text(hover);
                ui.add_space(10.0);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let panel_button = ui.add_sized(
                vec2(28.0, 24.0),
                egui::Button::new("").fill(Color32::TRANSPARENT).stroke(Stroke::NONE),
            );
            let panel_color = if show_right_panel { theme::TEXT } else { theme::TEXT_MUTED };
            if mode == Backend::Simulator {
                icons::gamepad(ui.painter(), egui::Rect::from_center_size(panel_button.rect.center(), vec2(16.0, 16.0)), panel_color);
            } else {
                icons.lego(ui.painter(), egui::Rect::from_center_size(panel_button.rect.center(), vec2(16.0, 16.0)), panel_color);
            }
            let panel_name = if mode == Backend::Simulator { "Simulator" } else { "Devices" };
            if panel_button
                .on_hover_text(format!("{panel_name} pane (Ctrl/Cmd+2; Ctrl/Cmd+F toggles both)"))
                .clicked()
            {
                actions.push(ToolbarAction::TogglePanel);
            }
            if let Some(status) = status {
                ui.add_space(8.0);
                ui.add(egui::Label::new(RichText::new(&status.text).color(status.color)).truncate())
                    .on_hover_text(&status.text);
            }
        });
    });
    actions
}

fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(theme::BG)
        .corner_radius(10)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    ui.add_space(6.0);
}

fn device_header(ui: &mut Ui, s: &mut MockState, kind: DeviceKind) -> bool {
    let mut on = s.is_attached(kind);
    ui.horizontal(|ui| {
        ui.label(RichText::new(kind.label()).size(15.0).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.checkbox(&mut on, "on").changed() {
                s.set_attached(kind, on);
            }
            let mut pressed = s.button(kind);
            let resp = ui
                .add_enabled(on, egui::Button::new("● button").selected(pressed).sense(Sense::click_and_drag()))
                .on_hover_text("Hold to press the device's button");
            pressed = resp.is_pointer_button_down_on();
            s.set_button(kind, pressed && on);
        });
    });
    ui.horizontal(|ui| {
        ui.label("Card group");
        let mut group = s.group(kind);
        egui::ComboBox::from_id_salt(("sim_card_group", kind))
            .selected_text(group.label())
            .width(125.0)
            .show_ui(ui, |ui| {
                for option in CardGroup::ASSIGNABLE {
                    ui.selectable_value(&mut group, option, option.label());
                }
            });
        if group != s.group(kind) {
            s.set_group(kind, group);
            s.log(format!("{} assigned to {}", kind.label(), group.label()));
        }
    });
    on
}

fn motor_row(ui: &mut Ui, name: &str, m: &MockMotor) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
        let p = ui.painter();
        let c = rect.center();
        p.circle_filled(c, 20.0, Color32::WHITE);
        p.circle_stroke(c, 20.0, Stroke::new(2.0, theme::PANEL_BORDER));
        let a = (m.position as f32).to_radians() - std::f32::consts::FRAC_PI_2;
        p.line_segment([c, c + vec2(a.cos(), a.sin()) * 16.0], Stroke::new(3.0, theme::category(crate::visual::Category::Motors).fill));
        p.circle_filled(c, 3.0, theme::TEXT);
        ui.vertical(|ui| {
            ui.label(RichText::new(name).strong());
            ui.label(format!("Speed     {:>4}%", m.speed));
            ui.label(format!("Position  {:>6.0}°", m.position));
        });
    });
}

pub fn simulator_panel(ui: &mut Ui, mock: &MockDevice) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
        icons::gamepad(ui.painter(), rect.shrink(2.0), theme::TEXT);
        ui.label(RichText::new("SIMULATOR").size(16.0).strong());
    });
    ui.add_space(4.0);
    mock.with_state(|s| {
        ui.horizontal(|ui| {
            super::toolbar::connection_dot(ui, s.connection);
        });
        ui.add_space(6.0);
        egui::ScrollArea::vertical().id_salt("sim_scroll").max_height(ui.available_height() - 190.0).show(ui, |ui| {
            card(ui, |ui| {
                if device_header(ui, s, DeviceKind::SingleMotor) {
                    motor_row(ui, "Motor", &s.single);
                }
            });
            card(ui, |ui| {
                if device_header(ui, s, DeviceKind::DoubleMotor) {
                    motor_row(ui, "Left", &s.left);
                    motor_row(ui, "Right", &s.right);
                }
            });
            card(ui, |ui| {
                if device_header(ui, s, DeviceKind::ColorSensor) {
                    ui.horizontal(|ui| {
                        ui.label("Simulated colour:");
                        let [r, g, b] = s.color.rgb();
                        let (sw, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                        ui.painter().circle_filled(sw.center(), 8.0, Color32::from_rgb(r, g, b));
                        egui::ComboBox::from_id_salt("sim_color")
                            .selected_text(s.color.label())
                            .show_ui(ui, |ui| {
                                for c in LegoColor::ALL {
                                    ui.selectable_value(&mut s.color, c, c.label());
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        ui.label("Reflection");
                        ui.add(egui::Slider::new(&mut s.reflection, 0..=100).suffix(" %"));
                    });
                }
            });
            card(ui, |ui| {
                if device_header(ui, s, DeviceKind::Controller) {
                    ui.horizontal(|ui| {
                        ui.label("Left lever ");
                        ui.add(egui::Slider::new(&mut s.lever_left, -100..=100).suffix(" %"));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Right lever");
                        ui.add(egui::Slider::new(&mut s.lever_right, -100..=100).suffix(" %"));
                    });
                }
            });
        });

        ui.add_space(4.0);
        ui.label(RichText::new("COMMAND LOG").size(12.5).strong().color(theme::TEXT_MUTED));
        egui::Frame::new().fill(theme::BG).corner_radius(10).inner_margin(8).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .id_salt("sim_log")
                .stick_to_bottom(true)
                .auto_shrink(false)
                .show(ui, |ui| {
                    for e in &s.log {
                        ui.label(
                            RichText::new(format!("{}  {}", e.time, e.text))
                                .monospace()
                                .size(12.0)
                        );
                    }
                });
        });
        if s.any_motor_running() {
            ui.ctx().request_repaint();
        }
    });
}

/// What the user asked for in the devices panel.
pub enum PanelAction {
    Scan,
    SetDeviceFilter(Option<CardGroup>),
    ConnectMatching(Option<CardGroup>),
    Connect(String),
    Disconnect(String),
}

/// Everything the devices panel shows about real hardware.
pub struct HardwareView<'a> {
    pub connection: ConnectionState,
    pub busy: bool,
    pub found: &'a [FoundDevice],
    pub connected: &'a [String],
    pub live: &'a [DeviceSnapshot],
    pub filter: Option<CardGroup>,
}

fn signal(rssi: Option<i16>) -> Option<(&'static str, &'static str)> {
    match rssi? {
        r if r >= -60 => Some(("▂▄▆", "strong signal")),
        r if r >= -75 => Some(("▂▄", "good signal")),
        _ => Some(("▂", "weak signal: move closer")),
    }
}

fn readings_grid(ui: &mut Ui, d: &DeviceSnapshot) {
    egui::Grid::new(("readings", d.kind.label(), &d.card)).num_columns(2).show(ui, |ui| {
        for (k, v) in &d.readings {
            ui.label(RichText::new(k).color(theme::TEXT_MUTED));
            ui.label(RichText::new(v).strong());
            ui.end_row();
        }
    });
}

/// Scan, pick devices and see live readings from real hardware.
pub fn hardware_panel(ui: &mut Ui, view: HardwareView, icons: &IconTextures) -> Option<PanelAction> {
    let mut action = None;
    let found_group = |d: &FoundDevice| {
        view.live
            .iter()
            .find(|live| live.address == d.address)
            .map_or(d.group, |live| live.group)
    };
    let has_matching_found = view.found.iter().any(|d| view.filter.is_none_or(|g| g.matches_device_group(found_group(d))));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
        icons.lego(ui.painter(), rect.shrink(2.0), theme::TEXT);
        ui.label(RichText::new("DEVICES").size(16.0).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let scan = egui::Button::new(RichText::new("🔍 Scan").strong());
            if ui.add_enabled(!view.busy, scan).on_hover_text("Look for devices nearby (4 seconds)").clicked() {
                action = Some(PanelAction::Scan);
            }
            let connect_text = if view.filter.is_some() { "Connect filtered" } else { "Connect all" };
            let connect = egui::Button::new(RichText::new(connect_text).strong());
            if ui
                .add_enabled(!view.busy && has_matching_found, connect)
                .on_hover_text(if view.filter.is_some() {
                    "Connect devices matching the selected card colour"
                } else {
                    "Connect every device found in the latest scan"
                })
                .clicked()
            {
                action = Some(PanelAction::ConnectMatching(view.filter));
            }
            if view.busy {
                ui.spinner();
            }
        });
    });
    let mut filter = view.filter;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        egui::ComboBox::from_id_salt("device_card_filter")
            .selected_text(filter.map_or_else(|| "All".to_string(), |g| g.label().to_string()))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut filter, None, "All");
                for group in CardGroup::ASSIGNABLE {
                    ui.selectable_value(&mut filter, Some(group), group.label());
                }
            });
    });
    if filter != view.filter {
        action = Some(PanelAction::SetDeviceFilter(filter));
    }
    ui.add_space(6.0);

    egui::ScrollArea::vertical().id_salt("hw_scroll").show(ui, |ui| {
        if view.found.is_empty() && view.live.is_empty() && !view.busy {
            ui.label(
                RichText::new(
                    "Switch on your devices (their light blinks when they are ready), then press Scan."
                )
                .color(theme::TEXT_MUTED)
            );
        }
        if !view.found.is_empty() && !has_matching_found {
            if let Some(group) = view.filter {
                ui.label(RichText::new(format!("No devices match the {} filter.", group.label())).color(theme::TEXT_MUTED));
            }
        }
        for d in view.found.iter().filter(|d| view.filter.is_none_or(|g| g.matches_device_group(found_group(d)))) {
            let connected = view.connected.contains(&d.address);
            let live = view.live.iter().find(|l| l.address == d.address);
            // A card scanned while connected is newer than the scan result.
            let card_name = live.map_or(d.card.as_str(), |l| l.card.as_str());
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(d.kind.label()).size(15.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if connected {
                            if ui.add_enabled(!view.busy, egui::Button::new("Disconnect")).clicked() {
                                action = Some(PanelAction::Disconnect(d.address.clone()));
                            }
                        } else if ui
                            .add_enabled(!view.busy, egui::Button::new(RichText::new("Connect").strong()))
                            .on_hover_text(format!("Use this {} in your program", d.kind.label()))
                            .clicked()
                        {
                            action = Some(PanelAction::Connect(d.address.clone()));
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Card: {card_name}")).color(theme::TEXT_MUTED));
                    if let Some((bars, text)) = signal(d.rssi) {
                        ui.label(RichText::new(bars).color(theme::TEXT_MUTED)).on_hover_text(text);
                    }
                    if connected {
                        ui.label(RichText::new("✔ connected").color(theme::OK_GREEN));
                    }
                    if let Some(b) = live.and_then(|l| l.battery) {
                        let color = if b < 20 { theme::STOP_RED } else { theme::TEXT_MUTED };
                        ui.label(RichText::new(format!("🔋 {b}%")).color(color));
                    }
                });
                if connected && let Some(live) = live {
                    readings_grid(ui, live);
                }
            });
        }
        // Connected devices that aren't in the scan list (e.g. after a re-scan).
        for live in view.live.iter().filter(|l| {
            view.filter.is_none_or(|g| g.matches_device_group(l.group)) && !view.found.iter().any(|f| f.address == l.address)
        }) {
            card(ui, |ui| {
                ui.label(RichText::new(live.kind.label()).size(15.0).strong());
                ui.label(RichText::new(format!("Card {} · ✔ connected", live.card)).color(theme::TEXT_MUTED));
                readings_grid(ui, live);
            });
        }
    });
    if view.connection != ConnectionState::Disconnected || view.busy {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    action
}
