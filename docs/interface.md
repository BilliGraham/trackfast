# Interface design

Trackfast uses dark neutral surfaces, small headings, and muted pink selection accents.
Project colours identify projects. They do not decorate unrelated controls.

The navigation has Timer, Projects, and Settings. The navigation uses full-width rows.
The Timer screen shows the running sessions in a card, then a Calendar view or a List view.
The Calendar view is the default.
The Calendar view shows hour labels in 12-hour format and sessions as blocks in their project colour.
Overlapping sessions share the day width. Sessions that cross midnight show on both days.
A red line marks the current time. The − and + buttons change the hour height.
In the List view, sessions show the description and duration above the project and time range.
Long descriptions and project names shorten to fit the available width. Hover to read the full text.
Calendar blocks and List view sessions have menus with three vertical dots.
Calendar block menus appear only while the pointer is over the block.
Each menu has a Continue in widget action. The action is disabled when the widget has three rows.

The widget is a hub for current work, not a history.
Each row shows a round play or pause button in the project colour, the task, the project, the time, and ×.
The play and pause icons are painted shapes, because the bundled fonts lack those glyphs.
A running row shows the time of the current session. A paused row shows today's total for that task in grey.
The ⋮ menu appears at the lower left on hover or keyboard focus. It stays visible while a menu is open.
Its Add task submenu has a new task form and a list of recent tasks that are not already in the widget.
Add task is disabled when the widget has three rows. The menu also has a Close widget action.
The main button shows Open widget or Close widget to match the current window state.

The Projects screen lists projects with their colour and client. Archived projects are last.
The project form has a name, an optional client with suggestions from existing clients, and a colour palette.
The Settings screen has the Toggl import form.
Disabled controls explain their current limit on hover.
Empty screens use short messages without development plans or sample account data.

Text uses Inter. Headings, calendar day numbers, and calendar entry titles use Inter SemiBold.
Times and durations use Inter SemiBold. `src/ui/digits.rs` draws each digit in a cell of equal width,
so a running timer does not move. egui cannot turn on the tabular-figures feature of Inter.
JetBrains Mono is the monospace font for any other code-style text.
The fonts are Latin subsets from Fontsource in `assets/fonts/`. Each font has its SIL Open Font License file there.
The egui default fonts stay as fallbacks for other characters.

Keep theme values in `src/ui/style.rs`.
Check the main view at 1150 by 720 and 760 by 480 logical pixels.
Check one widget row at 300 by 160 logical pixels and three rows at 300 by 304 logical pixels.
Keep review captures outside the repository.
