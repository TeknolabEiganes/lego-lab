//! Shared pane icons. The puzzle and device images are rasterized from the
//! SVG files supplied in `assets/` and tinted by egui at draw time.

use egui::{Color32, Context, Painter, Pos2, Rect, Stroke, TextureHandle, TextureOptions, vec2};

#[derive(Clone)]
pub struct IconTextures {
    puzzle: TextureHandle,
    lego: TextureHandle,
}

impl IconTextures {
    pub fn load(ctx: &Context) -> Self {
        Self {
            puzzle: load_icon(ctx, "puzzle", include_bytes!("../../assets/puzzle.png")),
            lego: load_icon(ctx, "lego-device", include_bytes!("../../assets/lego-device.png")),
        }
    }

    pub fn puzzle(&self, painter: &Painter, rect: Rect, tint: Color32) {
        paint_texture(painter, &self.puzzle, rect, tint);
    }

    pub fn lego(&self, painter: &Painter, rect: Rect, tint: Color32) {
        paint_texture(painter, &self.lego, rect, tint);
    }
}

fn load_icon(ctx: &Context, name: &str, bytes: &[u8]) -> TextureHandle {
    let image = image::load_from_memory(bytes)
        .unwrap_or_else(|e| panic!("failed to decode embedded {name} icon: {e}"))
        .to_rgba8();
    let [width, height] = [image.width() as usize, image.height() as usize];
    let mut pixels = image.into_raw();
    // The source SVGs are black. White RGB with the original alpha makes the
    // texture tintable for active and inactive states.
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[..3].fill(255);
    }
    ctx.load_texture(
        format!("pane-icon-{name}"),
        egui::ColorImage::from_rgba_unmultiplied([width, height], &pixels),
        TextureOptions::LINEAR,
    )
}

fn paint_texture(painter: &Painter, texture: &TextureHandle, rect: Rect, tint: Color32) {
    let size = rect.width().min(rect.height());
    let rect = Rect::from_center_size(rect.center(), vec2(size, size));
    painter.image(
        texture.id(),
        rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        tint,
    );
}

/// Draw a compact gamepad outline for the Simulator pane.
pub fn gamepad(painter: &Painter, rect: Rect, color: Color32) {
    let size = rect.width().min(rect.height());
    let center = rect.center();
    let left = center.x - size * 0.5 + size * 0.12;
    let right = center.x + size * 0.5 - size * 0.12;
    let top = center.y - size * 0.5 + size * 0.27;
    let bottom = center.y + size * 0.5 - size * 0.16;
    let stroke = Stroke::new((size * 0.1).max(1.2), color);
    let outline = vec![
        Pos2::new(left + size * 0.17, top),
        Pos2::new(right - size * 0.17, top),
        Pos2::new(right - size * 0.05, top + size * 0.08),
        Pos2::new(right, bottom - size * 0.12),
        Pos2::new(right - size * 0.12, bottom),
        Pos2::new(center.x + size * 0.12, bottom - size * 0.1),
        Pos2::new(center.x - size * 0.12, bottom - size * 0.1),
        Pos2::new(left + size * 0.12, bottom),
        Pos2::new(left, bottom - size * 0.12),
        Pos2::new(left + size * 0.05, top + size * 0.08),
    ];
    painter.add(egui::Shape::closed_line(outline, stroke));
    painter.line_segment([center + vec2(-size * 0.27, 0.0), center + vec2(-size * 0.1, 0.0)], stroke);
    painter.line_segment([center + vec2(-size * 0.185, -size * 0.085), center + vec2(-size * 0.185, size * 0.085)], stroke);
    painter.circle_filled(center + vec2(size * 0.19, -size * 0.045), size * 0.045, color);
    painter.circle_filled(center + vec2(size * 0.28, size * 0.045), size * 0.045, color);
}
