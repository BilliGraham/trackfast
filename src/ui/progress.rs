use super::Clock;
use crate::entries::{Snapshot, summary, week};
use eframe::egui::{self, vec2};

pub fn show(ui: &mut egui::Ui, data: &Snapshot, clock: &Clock, start: jiff::civil::Date) {
    if summary::start_of_day(start, &clock.tz).is_some_and(|from| from < data.loaded_from) {
        ui.weak("Loading weekly total...");
        return;
    }
    let end = week::shift(start, 1);
    let total: i64 = data
        .sessions
        .iter()
        .filter(|s| s.date(&clock.tz) >= start && s.date(&clock.tz) < end)
        .map(|s| s.effective(clock.now).as_secs())
        .sum();
    let target = data.settings.weekly_target_hours;
    let hours = total as f64 / 3600.0;
    let fraction = (hours / target).clamp(0.0, 1.0) as f32;
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let (rect, response) = ui.allocate_exact_size(vec2(100.0, 8.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 3.0, super::style::BORDER);
        if fraction > 0.0 {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(rect.min, vec2(rect.width() * fraction, rect.height())),
                3.0,
                super::style::ACCENT,
            );
        }
        response.on_hover_text("Weekly target");

        ui.label(format!(
            "Week: {} / {target} h",
            summary::clock(jiff::SignedDuration::from_secs(total))
        ));
        if hours >= target {
            ui.weak("Target reached");
        } else {
            ui.weak(format!("{:.1} h remaining", target - hours));
        }
    });
}
