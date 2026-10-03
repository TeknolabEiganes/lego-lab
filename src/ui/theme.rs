//! Colours and visual style.

use egui::{Color32, CornerRadius, Stroke, Visuals};

use crate::visual::Category;

pub const BG: Color32 = Color32::from_rgb(0xF4, 0xF6, 0xFB);
pub const PANEL: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
pub const PANEL_BORDER: Color32 = Color32::from_rgb(0xE2, 0xE6, 0xEF);
pub const GRID_DOT: Color32 = Color32::from_rgb(0xD5, 0xDB, 0xE7);
pub const TEXT: Color32 = Color32::from_rgb(0x1F, 0x29, 0x37);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x6B, 0x72, 0x80);
pub const LEGO_RED: Color32 = Color32::from_rgb(0xE3, 0x00, 0x0B);
pub const RUN_GREEN: Color32 = Color32::from_rgb(0x22, 0xA5, 0x5B);
pub const STOP_RED: Color32 = Color32::from_rgb(0xE5, 0x48, 0x4D);
pub const OK_GREEN: Color32 = Color32::from_rgb(0x22, 0xC5, 0x5E);
pub const WARN_AMBER: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B);
pub const OFF_GREY: Color32 = Color32::from_rgb(0x9C, 0xA3, 0xAF);
pub const HIGHLIGHT: Color32 = Color32::from_rgb(0xFF, 0xE0, 0x3D);
pub const SELECT: Color32 = Color32::from_rgb(0x1E, 0x40, 0xAF);
pub const ERROR: Color32 = Color32::from_rgb(0xDC, 0x26, 0x26);

pub struct CategoryStyle {
    pub name: &'static str,
    pub fill: Color32,
    pub dark: Color32,
    pub text: Color32,
}

pub fn category(cat: Category) -> CategoryStyle {
    match cat {
        Category::Events => CategoryStyle {
            name: "EVENTS",
            fill: Color32::from_rgb(0xFF, 0xC1, 0x07),
            dark: Color32::from_rgb(0xD9, 0x9A, 0x00),
            text: Color32::from_rgb(0x3D, 0x2C, 0x00),
        },
        Category::Motors => CategoryStyle {
            name: "MOTORS",
            fill: Color32::from_rgb(0x3B, 0x82, 0xF6),
            dark: Color32::from_rgb(0x25, 0x5F, 0xC9),
            text: Color32::WHITE,
        },
        Category::Sounds => CategoryStyle {
            name: "SOUNDS",
            fill: Color32::from_rgb(0x8B, 0x5C, 0xF6),
            dark: Color32::from_rgb(0x6D, 0x28, 0xD9),
            text: Color32::WHITE,
        },
        Category::Control => CategoryStyle {
            name: "CONTROL",
            fill: Color32::from_rgb(0xF9, 0x73, 0x16),
            dark: Color32::from_rgb(0xC9, 0x55, 0x08),
            text: Color32::WHITE,
        },
        Category::Sensors => CategoryStyle {
            name: "SENSORS",
            fill: Color32::from_rgb(0x0E, 0xA5, 0x8F),
            dark: Color32::from_rgb(0x0A, 0x7A, 0x6A),
            text: Color32::WHITE,
        },
    }
}

pub fn apply(ctx: &egui::Context) {
    let mut visuals = Visuals::light();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = Color32::WHITE;
    visuals.override_text_color = Some(TEXT);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, PANEL_BORDER);
    for w in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(8);
    }
    ctx.set_visuals(visuals);
    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = 24.0;
    });
}
