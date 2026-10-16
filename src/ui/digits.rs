//! Draws times with Inter digits in equal-width cells.
//!
//! egui cannot turn on the tabular-figures feature of a font, and Inter's
//! default digits have different widths. Without equal cells a running timer
//! would move each second.

use eframe::egui::{
    self, Align2, Color32, FontId, Painter, Pos2, Rect, Response, Sense, pos2, vec2,
};

use super::style;

pub fn font(size: f32) -> FontId {
    FontId::new(size, style::semibold())
}

fn advances(ctx: &egui::Context, text: &str, font: &FontId) -> (Vec<f32>, f32) {
    ctx.fonts_mut(|fonts| {
        let cell = ('0'..='9')
            .map(|digit| fonts.glyph_width(font, digit))
            .fold(0.0, f32::max);
        let widths = text
            .chars()
            .map(|glyph| {
                if glyph.is_ascii_digit() {
                    cell
                } else {
                    fonts.glyph_width(font, glyph)
                }
            })
            .collect();
        (widths, fonts.row_height(font))
    })
}

pub fn size(ctx: &egui::Context, text: &str, font: &FontId) -> egui::Vec2 {
    let (widths, height) = advances(ctx, text, font);
    vec2(widths.iter().sum(), height)
}

pub fn paint(
    painter: &Painter,
    anchor: Pos2,
    align: Align2,
    text: &str,
    font: FontId,
    colour: Color32,
) -> Rect {
    let (widths, height) = advances(painter.ctx(), text, &font);
    let rect = align.anchor_size(anchor, vec2(widths.iter().sum(), height));
    let mut left = rect.left();
    for (glyph, width) in text.chars().zip(widths) {
        painter.text(
            pos2(left + width / 2.0, rect.center().y),
            Align2::CENTER_CENTER,
            glyph,
            font.clone(),
            colour,
        );
        left += width;
    }
    rect
}

pub fn label(ui: &mut egui::Ui, text: &str, font: FontId, colour: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(size(ui.ctx(), text, &font), Sense::hover());
    paint(
        ui.painter(),
        rect.left_top(),
        Align2::LEFT_TOP,
        text,
        font,
        colour,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_with_different_digits_have_the_same_width() {
        let ctx = egui::Context::default();
        ctx.set_fonts(style::fonts());
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let font = font(13.0);
        assert_eq!(size(&ctx, "1:11:11", &font), size(&ctx, "8:00:48", &font));
        assert!(size(&ctx, "1:11:11", &font).x > 0.0);
    }
}
