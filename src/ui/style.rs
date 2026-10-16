use eframe::egui::{self, Color32};

pub const BACKGROUND: Color32 = Color32::from_rgb(24, 25, 28);
pub const SIDEBAR: Color32 = Color32::from_rgb(19, 20, 23);
pub const SURFACE: Color32 = Color32::from_rgb(30, 31, 35);
pub const BORDER: Color32 = Color32::from_rgb(48, 49, 55);
pub const TEXT: Color32 = Color32::from_rgb(225, 225, 230);
pub const MUTED: Color32 = Color32::from_rgb(153, 155, 165);
pub const ACCENT: Color32 = Color32::from_rgb(208, 151, 194);

pub fn apply(ui: &mut egui::Ui) {
    let style = ui.style_mut();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size.y = 32.0;
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(20.0));
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
