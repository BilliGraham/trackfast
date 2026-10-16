use eframe::egui;

pub(super) fn menu(
    ui: &mut egui::Ui,
    label: &str,
    contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    menu_with_dots(ui, label, true, contents)
}

/// Shows the dots only while the pointer is over `slot` or the menu is open.
/// The button stays in place while hidden so that its open state survives.
pub(super) fn menu_on_hover(
    ui: &mut egui::Ui,
    label: &str,
    slot: egui::Rect,
    contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let hovered = ui.rect_contains_pointer(slot);
    menu_with_dots(ui, label, hovered, contents)
}

/// The three dots sit on project-coloured blocks, so the button draws no
/// fill, border, or hover state of its own.
fn menu_with_dots(
    ui: &mut egui::Ui,
    label: &str,
    show_dots: bool,
    contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let (response, open) = ui
        .scope(|ui| {
            let widgets = &mut ui.visuals_mut().widgets;
            for state in [
                &mut widgets.inactive,
                &mut widgets.hovered,
                &mut widgets.active,
                &mut widgets.open,
            ] {
                state.bg_fill = egui::Color32::TRANSPARENT;
                state.weak_bg_fill = egui::Color32::TRANSPARENT;
                state.bg_stroke = egui::Stroke::NONE;
                state.expansion = 0.0;
            }
            let menu = ui.menu_button(" ", contents);
            (menu.response, menu.inner.is_some())
        })
        .inner;
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), label)
    });
    if !(show_dots || open) {
        return response;
    }
    let colour = ui.visuals().text_color();
    for offset in [-5.0, 0.0, 5.0] {
        ui.painter().circle_filled(
            response.rect.center() + egui::vec2(0.0, offset),
            1.5,
            colour,
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dots(pointer: Option<egui::Pos2>) -> usize {
        let ctx = egui::Context::default();
        let slot = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 60.0));
        let mut count = 0;
        for frame in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 200.0),
                )),
                time: Some(frame as f64),
                events: pointer.map(egui::Event::PointerMoved).into_iter().collect(),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                menu_on_hover(ui, "Entry actions", slot, |_| {});
            });
            output.textures_delta.clear();
            count = output
                .shapes
                .iter()
                .filter(|shape| matches!(&shape.shape, egui::Shape::Circle(circle) if circle.radius == 1.5))
                .count();
        }
        count
    }

    #[test]
    fn dots_appear_only_while_the_slot_is_hovered() {
        assert_eq!(dots(None), 0);
        assert_eq!(dots(Some(egui::pos2(250.0, 150.0))), 0);
        assert_eq!(dots(Some(egui::pos2(50.0, 30.0))), 3);
    }
}
