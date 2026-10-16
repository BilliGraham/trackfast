use eframe::egui::{self, Color32};

pub const BACKGROUND: Color32 = Color32::from_rgb(24, 25, 28);
pub const SIDEBAR: Color32 = Color32::from_rgb(19, 20, 23);
pub const SURFACE: Color32 = Color32::from_rgb(30, 31, 35);
pub const BORDER: Color32 = Color32::from_rgb(48, 49, 55);
pub const TEXT: Color32 = Color32::from_rgb(225, 225, 230);
pub const MUTED: Color32 = Color32::from_rgb(153, 155, 165);
pub const ACCENT: Color32 = Color32::from_rgb(208, 151, 194);

const INTER: &str = "Inter";
const INTER_SEMIBOLD: &str = "Inter SemiBold";
const JETBRAINS_MONO: &str = "JetBrains Mono";

pub fn semibold() -> egui::FontFamily {
    egui::FontFamily::Name(INTER_SEMIBOLD.into())
}

/// Inter for text and JetBrains Mono for times. JetBrains Mono digits all
/// have the same width, so a running timer does not move. The egui fonts
/// stay as fallbacks for symbols outside the bundled Latin subsets.
pub fn fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    for (name, data) in [
        (
            INTER,
            &include_bytes!("../../assets/fonts/Inter-Regular.ttf")[..],
        ),
        (
            INTER_SEMIBOLD,
            &include_bytes!("../../assets/fonts/Inter-SemiBold.ttf")[..],
        ),
        (
            JETBRAINS_MONO,
            &include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), egui::FontData::from_static(data).into());
    }
    let proportional = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let monospace = fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default();
    monospace.insert(0, JETBRAINS_MONO.into());
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, INTER.into());
    fonts.families.insert(
        semibold(),
        std::iter::once(INTER_SEMIBOLD.to_owned())
            .chain(proportional)
            .collect(),
    );
    fonts
}

pub fn apply(ui: &mut egui::Ui) {
    let style = ui.style_mut();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size.y = 32.0;
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(20.0, semibold()),
    );
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.weak_text_color = Some(MUTED);
    style.visuals.panel_fill = BACKGROUND;
    style.visuals.hyperlink_color = ACCENT;
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    style.visuals.widgets.inactive.weak_bg_fill = SURFACE;
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(43, 44, 50);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(52, 48, 56);
    style.visuals.selection.bg_fill = Color32::from_rgb(53, 39, 51);
    style.visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
}

pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(4)
        .inner_margin(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_fonts_render_the_interface_glyphs() {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts());
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let text = "0123456789:00 – ‹ › − · AM PM";
        ctx.fonts_mut(|fonts| {
            for family in [
                egui::FontFamily::Proportional,
                egui::FontFamily::Monospace,
                semibold(),
            ] {
                let font = egui::FontId::new(14.0, family);
                for glyph in text.chars() {
                    assert!(fonts.has_glyph(&font, glyph), "{glyph:?} is missing");
                }
            }
            let mono = egui::FontId::monospace(14.0);
            let mut width = |digits: &str| {
                fonts
                    .layout_no_wrap(digits.into(), mono.clone(), TEXT)
                    .size()
                    .x
            };
            assert_eq!(width("1:11:11"), width("8:88:88"));
        });
    }
}
