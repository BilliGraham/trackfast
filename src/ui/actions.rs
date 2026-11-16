use eframe::egui;

pub(super) fn delete_entry(
    ui: &mut egui::Ui,
    entry: &crate::entries::Session,
    actions: &mut Vec<crate::model::Action>,
) -> bool {
    let clicked = ui
        .add_enabled(!entry.is_running(), egui::Button::new("Delete entry"))
        .on_disabled_hover_text("Stop the timer before deleting this entry.")
        .clicked();
    if clicked {
        actions.push(crate::model::Action::ReviewDeletion(entry.id));
    }
    clicked
}

pub(super) fn menu(
    ui: &mut egui::Ui,
    label: &str,
    contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    menu_with_dots(ui, label, true, contents)
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
