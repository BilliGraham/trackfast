use eframe::egui::{self, Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use jiff::Timestamp;
use jiff::civil::Date;

use super::{Clock, continue_action, description, digits, project_colour, style};
use crate::entries::week::{self, Block};
use crate::entries::{Snapshot, summary};
use crate::model::{Action, State};

const GUTTER: f32 = 72.0;
const HEADER_HEIGHT: f32 = 54.0;
const MIN_BLOCK_HEIGHT: f32 = 18.0;
const BLOCK_PADDING: f32 = 6.0;
const MENU_WIDTH: f32 = 16.0;
const MENU_HEIGHT: f32 = 18.0;
const DOT_RADIUS: f32 = 1.5;
const NOW_LINE: Color32 = Color32::from_rgb(226, 85, 85);
const DEFAULT_SCROLL_HOUR: f32 = 7.5;

pub fn navigator(ui: &mut egui::Ui, shown: Date, current: Date, actions: &mut Vec<Action>) {
    ui.spacing_mut().item_spacing.x = 2.0;
    if ui.button("‹").on_hover_text("Previous week").clicked() {
        actions.push(Action::ShowWeek(Some(week::shift(shown, -1))));
    }
    let label = if shown == current {
        "This week".to_owned()
    } else if shown == week::shift(current, -1) {
        "Last week".to_owned()
    } else {
        format!(
            "{} – {}",
            shown.strftime("%-d %b"),
            week::days(shown)[6].strftime("%-d %b")
        )
    };
    let title = format!("{label} · W{}", week::number(shown));
    let button = ui.add(egui::Button::new(title).min_size(vec2(190.0, 0.0)));
    if shown != current && button.on_hover_text("Show this week").clicked() {
        actions.push(Action::ShowWeek(None));
    }
    let next = ui
        .add_enabled(shown < current, egui::Button::new("›"))
        .on_hover_text("Next week");
    if next.clicked() {
        actions.push(Action::ShowWeek(Some(week::shift(shown, 1))));
    }
}

pub fn show(
    ui: &mut egui::Ui,
    state: &State,
    snapshot: &Snapshot,
    clock: &Clock,
    shown: Date,
    actions: &mut Vec<Action>,
) {
    let days = week::days(shown);
    let bounds: Vec<Option<Timestamp>> = days
        .iter()
        .copied()
        .chain([week::shift(shown, 1)])
        .map(|day| summary::start_of_day(day, &clock.tz))
        .collect();
    let (Some(week_from), Some(week_to)) = (bounds[0], bounds[7]) else {
        return;
    };
    if ui.available_width() >= 700.0 {
        project_bar(ui, snapshot, week_from, week_to, clock);
    }
    ui.add_space(10.0);
    let durations: Vec<_> = snapshot
        .sessions
        .iter()
        .filter(|s| {
            s.details.duration_seconds.is_some()
                && s.date(&clock.tz) >= shown
                && s.date(&clock.tz) < week::shift(shown, 1)
        })
        .collect();
    if !durations.is_empty() {
        egui::CollapsingHeader::new(format!("Duration entries ({})", durations.len()))
            .default_open(ui.available_width() >= 700.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("duration-entries")
                    .max_height(100.0)
                    .show(ui, |ui| {
                        for entry in durations {
                            ui.horizontal(|ui| {
                                ui.weak(entry.date(&clock.tz).strftime("%a %-d %b").to_string());
                                if ui.button(super::description(entry)).clicked() {
                                    actions.push(Action::EditEntry(Some(entry.id)));
                                }
                                super::duration(ui, &summary::clock(entry.effective(clock.now)));
                                super::project_label(ui, snapshot, entry.project_id);
                            });
                        }
                    });
            });
    }
    header(ui, state, snapshot, clock, &days, &bounds, actions);
    let loaded = week_from >= snapshot.loaded_from;
    let hour_height = state.calendar.hour_height();
    let blocks: Vec<Vec<Block>> = if loaded {
        days.iter()
            .map(|day| week::day_blocks(&snapshot.sessions, *day, &clock.tz, clock.now))
            .collect()
    } else {
        vec![Vec::new(); 7]
    };
    let scroll_key = (shown, state.calendar.zoom);
    let scroll_memory = egui::Id::new("calendar-scroll-key");
    let mut area = egui::ScrollArea::vertical()
        .id_salt("calendar")
        .auto_shrink(false);
    if ui.data(|data| data.get_temp::<(Date, usize)>(scroll_memory)) != Some(scroll_key) {
        ui.data_mut(|data| data.insert_temp(scroll_memory, scroll_key));
        let first_minutes = blocks
            .iter()
            .flatten()
            .map(|block| block.start)
            .reduce(f32::min)
            .map(|minutes| minutes / 60.0 - 0.5)
            .unwrap_or(DEFAULT_SCROLL_HOUR);
        area = area
            .vertical_scroll_offset(first_minutes.clamp(0.0, DEFAULT_SCROLL_HOUR) * hour_height);
    }
    area.show(ui, |ui| {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(vec2(width, hour_height * 24.0), Sense::hover());
        grid(ui, rect, hour_height);
        let column_width = (rect.width() - GUTTER) / 7.0;
        for (index, day_blocks) in blocks.iter().enumerate() {
            let left = rect.left() + GUTTER + column_width * index as f32;
            if loaded && !state.saving {
                let over_entry =
                    ui.input(|input| input.pointer.press_origin())
                        .is_some_and(|origin| {
                            origin.x >= left
                                && origin.x < left + column_width
                                && day_blocks.iter().any(|block| {
                                    let top = rect.top() + block.start / 60.0 * hour_height;
                                    let height = ((block.end - block.start) / 60.0 * hour_height)
                                        .max(MIN_BLOCK_HEIGHT);
                                    origin.y >= top && origin.y < top + height
                                })
                        });
                if !over_entry {
                    super::calendar_input::show(
                        ui,
                        Rect::from_min_max(
                            pos2(left, rect.top()),
                            pos2(left + column_width, rect.bottom()),
                        ),
                        days[index],
                        clock,
                        actions,
                    );
                }
            }
            for block in day_blocks {
                ui.push_id((days[index], block.entry.id), |ui| {
                    entry_block(
                        ui,
                        snapshot,
                        clock,
                        block,
                        left,
                        column_width,
                        rect.top(),
                        hour_height,
                        actions,
                    );
                });
            }
            if days[index] == clock.today() {
                now_line(
                    ui,
                    clock,
                    bounds[index],
                    left,
                    column_width,
                    rect.top(),
                    hour_height,
                );
            }
        }
        if !loaded {
            not_loaded(ui, rect);
        }
    });
}

fn project_bar(
    ui: &mut egui::Ui,
    snapshot: &Snapshot,
    from: Timestamp,
    to: Timestamp,
    clock: &Clock,
) {
    let totals = week::project_totals(&snapshot.sessions, from, to, clock.now, &clock.tz);
    let sum: f64 = totals.iter().map(|(_, total)| total.as_secs_f64()).sum();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let painter = ui.painter_at(rect);
    if sum <= 0.0 {
        painter.hline(
            rect.x_range(),
            rect.bottom() - 2.0,
            Stroke::new(3.0, style::BORDER),
        );
        return;
    }
    let mut left = rect.left();
    for (project, total) in totals {
        let width = rect.width() * (total.as_secs_f64() / sum) as f32;
        let (name, colour) = match project.and_then(|id| snapshot.project(id)) {
            Some(project) => (project.name.to_uppercase(), project_colour(&project.colour)),
            None => ("NO PROJECT".to_owned(), super::NO_PROJECT),
        };
        let segment = Rect::from_min_max(pos2(left, rect.top()), pos2(left + width, rect.bottom()));
        painter.hline(
            segment.left() + 1.0..=segment.right() - 1.0,
            segment.bottom() - 2.0,
            Stroke::new(3.0, colour),
        );
        let label = painter.layout_no_wrap(name, FontId::proportional(11.0), colour);
        if label.size().x + 4.0 < width {
            painter.galley(pos2(segment.left() + 1.0, segment.top()), label, colour);
        }
        ui.interact(
            segment,
            ui.id().with(("project-share", project)),
            Sense::hover(),
        )
        .on_hover_text(summary::clock(total));
        left += width;
    }
}

#[allow(clippy::too_many_arguments)]
fn header(
    ui: &mut egui::Ui,
    state: &State,
    snapshot: &Snapshot,
    clock: &Clock,
    days: &[Date; 7],
    bounds: &[Option<Timestamp>],
    actions: &mut Vec<Action>,
) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), HEADER_HEIGHT), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.hline(
        rect.x_range(),
        rect.bottom(),
        Stroke::new(1.0, style::BORDER),
    );
    let zoom = Rect::from_min_size(rect.left_top() + vec2(0.0, 12.0), vec2(GUTTER - 8.0, 28.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(zoom), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().button_padding = vec2(7.0, 2.0);
            ui.spacing_mut().item_spacing.x = 2.0;
            let calendar = &state.calendar;
            if ui
                .add_enabled(calendar.zoom > 0, egui::Button::new("−"))
                .on_hover_text("Zoom out")
                .clicked()
            {
                actions.push(Action::Zoom(-1));
            }
            if ui
                .add_enabled(
                    calendar.zoom + 1 < crate::model::HOUR_HEIGHTS.len(),
                    egui::Button::new("+"),
                )
                .on_hover_text("Zoom in")
                .clicked()
            {
                actions.push(Action::Zoom(1));
            }
        });
    });
    let column_width = (rect.width() - GUTTER) / 7.0;
    let today = clock.today();
    for (index, day) in days.iter().enumerate() {
        let left = rect.left() + GUTTER + column_width * index as f32;
        if column_width < 100.0 {
            let centre = left + column_width / 2.0;
            let colour = if *day == today {
                style::ACCENT
            } else {
                style::TEXT
            };
            painter.text(
                pos2(centre, rect.top() + 5.0),
                Align2::CENTER_TOP,
                day.strftime("%a %-d").to_string(),
                FontId::proportional(11.0),
                colour,
            );
            let total = match (bounds[index], bounds[index + 1]) {
                (Some(from), Some(to)) => {
                    week::total_between(&snapshot.sessions, from, to, clock.now, &clock.tz)
                }
                _ => jiff::SignedDuration::ZERO,
            };
            let text = format!(
                "{}:{:02}",
                total.as_secs() / 3600,
                total.as_secs() / 60 % 60
            );
            painter.text(
                pos2(centre, rect.top() + 26.0),
                Align2::CENTER_TOP,
                text,
                FontId::proportional(11.0),
                style::MUTED,
            );
            continue;
        }
        let number_centre = pos2(left + 22.0, rect.center().y);
        let is_today = *day == today;
        if is_today {
            painter.circle_filled(number_centre, 17.0, style::ACCENT.gamma_multiply(0.25));
        }
        painter.text(
            number_centre,
            Align2::CENTER_CENTER,
            day.day().to_string(),
            FontId::new(22.0, style::semibold()),
            if is_today { style::ACCENT } else { style::TEXT },
        );
        let weekday = day.strftime("%a").to_string().to_uppercase();
        painter.text(
            pos2(left + 44.0, rect.center().y - 1.0),
            Align2::LEFT_BOTTOM,
            weekday,
            FontId::proportional(11.0),
            if is_today {
                style::ACCENT
            } else {
                style::MUTED
            },
        );
        let total = match (bounds[index], bounds[index + 1]) {
            (Some(from), Some(to)) => {
                week::total_between(&snapshot.sessions, from, to, clock.now, &clock.tz)
            }
            _ => jiff::SignedDuration::ZERO,
        };
        digits::paint(
            &painter,
            pos2(left + 44.0, rect.center().y + 1.0),
            Align2::LEFT_TOP,
            &summary::clock(total),
            FontId::proportional(11.5),
            style::MUTED,
        );
    }
}

fn grid(ui: &egui::Ui, rect: Rect, hour_height: f32) {
    let painter = ui.painter_at(rect);
    let line = Stroke::new(1.0, style::BORDER);
    for hour in 0..24 {
        let y = rect.top() + hour as f32 * hour_height;
        if hour > 0 {
            painter.hline(rect.left() + GUTTER - 4.0..=rect.right(), y, line);
        }
        painter.text(
            pos2(rect.left() + GUTTER - 10.0, y.max(rect.top() + 7.0)),
            Align2::RIGHT_CENTER,
            hour_label(hour),
            FontId::proportional(11.0),
            style::MUTED,
        );
    }
    let column_width = (rect.width() - GUTTER) / 7.0;
    for day in 0..=7 {
        let x = rect.left() + GUTTER + column_width * day as f32;
        painter.vline(x, rect.y_range(), line);
    }
}

fn hour_label(hour: u8) -> String {
    let suffix = if hour < 12 { "AM" } else { "PM" };
    format!("{}:00 {suffix}", (hour + 11) % 12 + 1)
}

/// Mixes the project colour into the background so text stays readable.
fn block_fill(colour: Color32) -> Color32 {
    let mix = |background: u8, project: u8| {
        (f32::from(background) * 0.68 + f32::from(project) * 0.32).round() as u8
    };
    let [r, g, b, _] = colour.to_array();
    let [br, bg, bb, _] = style::BACKGROUND.to_array();
    Color32::from_rgb(mix(br, r), mix(bg, g), mix(bb, b))
}

#[allow(clippy::too_many_arguments)]
fn entry_block(
    ui: &mut egui::Ui,
    snapshot: &Snapshot,
    clock: &Clock,
    block: &Block,
    column_left: f32,
    column_width: f32,
    top: f32,
    hour_height: f32,
    actions: &mut Vec<Action>,
) {
    let entry = block.entry;
    let lane_width = (column_width - 4.0) / block.columns as f32;
    let left = column_left + 2.0 + lane_width * block.column as f32;
    let y = top + block.start / 60.0 * hour_height;
    let height = ((block.end - block.start) / 60.0 * hour_height).max(MIN_BLOCK_HEIGHT);
    let rect = Rect::from_min_size(pos2(left, y + 1.0), vec2(lane_width - 2.0, height - 2.0));
    let project = entry.project_id.and_then(|id| snapshot.project(id));
    let colour = project
        .map(|project| project_colour(&project.colour))
        .unwrap_or(super::NO_PROJECT);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, block_fill(colour));
    if entry.is_running() {
        painter.rect_stroke(
            rect,
            3.0,
            Stroke::new(1.5, colour),
            egui::StrokeKind::Inside,
        );
    }
    if !ui.is_rect_visible(rect) {
        return;
    }
    let room_for_duration = height > 64.0;
    let mut text_rect = rect.shrink(BLOCK_PADDING);
    if !room_for_duration {
        text_rect.max.x = (text_rect.right() - MENU_WIDTH).max(text_rect.left() + 1.0);
    }
    let text_painter = ui.painter_at(text_rect.expand2(vec2(0.0, BLOCK_PADDING - 2.0)));
    let wrap = text_rect.width().max(1.0);
    let duration = summary::clock(entry.effective(clock.now));
    let duration_font = digits::font(11.5);
    let duration_height = digits::size(ui.ctx(), &duration, &duration_font).y;
    let mut cursor = rect.top() + if height < 26.0 { 2.0 } else { BLOCK_PADDING };
    let title_font = FontId::new(12.5, style::semibold());
    let first_line_centre =
        cursor + ui.ctx().fonts_mut(|fonts| fonts.row_height(&title_font)) / 2.0;
    let text_bottom = if room_for_duration {
        rect.bottom() - BLOCK_PADDING - duration_height - 2.0
    } else {
        rect.bottom()
    };
    let title = text_painter.layout(description(entry).to_owned(), title_font, style::TEXT, wrap);
    let title_height = title.size().y;
    ui.painter_at(Rect::from_min_max(
        pos2(text_rect.left(), rect.top()),
        pos2(text_rect.right(), text_bottom),
    ))
    .galley(pos2(text_rect.left(), cursor), title, style::TEXT);
    cursor += title_height + 1.0;
    if let Some(project) = project {
        let name = text_painter.layout(
            project.name.clone(),
            FontId::proportional(11.5),
            colour,
            wrap,
        );
        if cursor + name.size().y <= text_bottom {
            text_painter.galley(pos2(text_rect.left(), cursor), name, colour);
        }
    }
    if room_for_duration {
        digits::paint(
            &text_painter,
            pos2(text_rect.left(), rect.bottom() - BLOCK_PADDING),
            Align2::LEFT_BOTTOM,
            &duration,
            duration_font,
            style::TEXT,
        );
    }
    let end = if entry.is_running() {
        "now".to_owned()
    } else {
        clock.local_time(entry.start + entry.elapsed(clock.now))
    };
    let entry_response = ui.interact(
        rect,
        ui.id().with(("entry", entry.start, block.start as i32)),
        Sense::click(),
    );
    if entry_response.clicked() && !entry.is_running() {
        actions.push(Action::EditEntry(Some(entry.id)));
    }
    entry_response.on_hover_ui(|ui| {
        ui.strong(description(entry));
        if let Some(project) = project {
            ui.colored_label(colour, &project.name);
        }
        ui.label(format!(
            "{} – {end}  ·  {}",
            clock.local_time(entry.start),
            summary::clock(entry.elapsed(clock.now))
        ));
    });
    // The dots line up with the duration row in tall blocks and with the
    // title in short ones, inset by the same padding as the text.
    let menu_centre = pos2(
        rect.right() - BLOCK_PADDING - DOT_RADIUS,
        if room_for_duration {
            rect.bottom() - BLOCK_PADDING - duration_height / 2.0
        } else {
            first_line_centre
        },
    );
    let menu_rect =
        Rect::from_center_size(menu_centre, vec2(MENU_WIDTH, MENU_HEIGHT)).intersect(rect);
    ui.scope_builder(
        egui::UiBuilder::new()
            .id_salt("entry-actions")
            .max_rect(menu_rect),
        |ui| {
            ui.spacing_mut().button_padding = egui::Vec2::ZERO;
            ui.spacing_mut().interact_size = menu_rect.size();
            super::actions::menu_on_hover(ui, "Entry actions", rect, |ui| {
                continue_action(ui, snapshot, entry, actions);
                if ui
                    .add_enabled(!entry.is_running(), egui::Button::new("Edit entry"))
                    .clicked()
                {
                    actions.push(Action::EditEntry(Some(entry.id)));
                    ui.close();
                }
            });
        },
    );
}

fn now_line(
    ui: &egui::Ui,
    clock: &Clock,
    day_start: Option<Timestamp>,
    left: f32,
    width: f32,
    top: f32,
    hour_height: f32,
) {
    let Some(day_start) = day_start else {
        return;
    };
    let minutes = clock.now.duration_since(day_start).as_secs_f64() as f32 / 60.0;
    if !(0.0..week::MINUTES_PER_DAY).contains(&minutes) {
        return;
    }
    let y = top + minutes / 60.0 * hour_height;
    let painter = ui.painter();
    painter.hline(left..=left + width, y, Stroke::new(1.5, NOW_LINE));
    painter.circle_filled(pos2(left, y), 4.0, NOW_LINE);
}

fn not_loaded(ui: &mut egui::Ui, grid: Rect) {
    let visible = ui.clip_rect().intersect(grid);
    let panel = Rect::from_center_size(visible.center(), vec2(240.0, 64.0));
    ui.painter().rect_filled(panel, 6.0, style::SURFACE);
    ui.scope_builder(egui::UiBuilder::new().max_rect(panel.shrink(14.0)), |ui| {
        ui.vertical_centered(|ui| {
            ui.spinner();
            ui.label("Loading this week");
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-10-16T12:00:00Z";

    /// Four completed sessions, and `rows` widget rows.
    fn snapshot(rows: usize) -> Snapshot {
        let sessions: Vec<crate::entries::Session> = (1..=4)
            .map(|id| crate::entries::Session {
                details: Default::default(),
                id,
                description: format!("Task {id}"),
                project_id: None,
                start: "2026-10-16T08:00:00Z".parse().unwrap(),
                stop: Some("2026-10-16T10:00:00Z".parse().unwrap()),
            })
            .collect();
        Snapshot {
            rows: (0..rows)
                .map(|id| crate::entries::WidgetRow {
                    id: id as i64,
                    task: crate::entries::Task::default(),
                    session: None,
                })
                .collect(),
            sessions,
            loaded_from: "2026-10-05T00:00:00Z".parse().unwrap(),
            ..Default::default()
        }
    }

    fn draw_entry(
        ctx: &egui::Context,
        rows: usize,
        end: f32,
        events: Vec<egui::Event>,
    ) -> (Vec<Action>, egui::FullOutput) {
        let snapshot = snapshot(rows);
        let clock = Clock {
            now: NOW.parse().unwrap(),
            tz: jiff::tz::TimeZone::UTC,
        };
        let block = Block {
            entry: &snapshot.sessions[3],
            start: 0.0,
            end,
            column: 0,
            columns: 1,
        };
        let mut actions = Vec::new();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 260.0))),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    style::apply(ui);
                    entry_block(
                        ui,
                        &snapshot,
                        &clock,
                        &block,
                        10.0,
                        120.0,
                        10.0,
                        60.0,
                        &mut actions,
                    );
                });
            },
        );
        output.textures_delta.clear();
        (actions, output)
    }

    fn click(pos: egui::Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ]
    }

    fn hover_block() -> Vec<egui::Event> {
        vec![egui::Event::PointerMoved(egui::pos2(30.0, 15.0))]
    }

    fn menu_position(output: &egui::FullOutput) -> egui::Pos2 {
        let dots: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Circle(circle) if circle.radius == 1.5 => Some(circle.center),
                _ => None,
            })
            .collect();
        assert_eq!(dots.len(), 3);
        assert!(
            dots.windows(2)
                .all(|pair| pair[0].x == pair[1].x && pair[0].y < pair[1].y)
        );
        dots[1]
    }

    #[test]
    fn calendar_menu_continues_an_entry_while_the_widget_has_room() {
        for rows in [0, 3] {
            let ctx = egui::Context::default();
            crate::ui::install_fonts(&ctx);
            let (_, output) = draw_entry(&ctx, rows, 120.0, hover_block());
            let menu = menu_position(&output);
            draw_entry(&ctx, rows, 120.0, click(menu));
            let (_, output) = draw_entry(&ctx, rows, 120.0, vec![]);
            let item = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == "Continue in widget" => {
                        Some(text.pos + text.galley.rect.center().to_vec2())
                    }
                    _ => None,
                })
                .expect("The calendar menu must show Continue in widget.");
            let (actions, _) = draw_entry(&ctx, rows, 120.0, click(item));
            let continued = actions.iter().any(
                |action| matches!(action, Action::AddTask(task) if task.description == "Task 4"),
            );
            assert_eq!(continued, rows == 0);
        }
    }

    #[test]
    fn short_calendar_entries_show_a_vertical_menu() {
        let ctx = egui::Context::default();
        crate::ui::install_fonts(&ctx);
        let (_, output) = draw_entry(&ctx, 0, 1.0, hover_block());
        let pos = menu_position(&output);
        assert!((11.0..27.0).contains(&pos.y));
    }

    #[test]
    fn calendar_menu_hides_until_the_entry_is_hovered() {
        let ctx = egui::Context::default();
        crate::ui::install_fonts(&ctx);
        let (_, output) = draw_entry(&ctx, 0, 120.0, vec![]);
        let dots = output
            .shapes
            .iter()
            .filter(
                |shape| matches!(&shape.shape, egui::Shape::Circle(circle) if circle.radius == 1.5),
            )
            .count();
        assert_eq!(dots, 0);
    }

    #[test]
    fn hour_labels_use_a_twelve_hour_clock() {
        assert_eq!(hour_label(0), "12:00 AM");
        assert_eq!(hour_label(8), "8:00 AM");
        assert_eq!(hour_label(12), "12:00 PM");
        assert_eq!(hour_label(17), "5:00 PM");
    }
}
