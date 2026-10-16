# Interface design

Trackfast uses dark neutral surfaces, small headings, and muted pink selection accents.
Project colours identify projects. They do not decorate unrelated controls.

The navigation uses full-width rows. The main view separates timer controls, totals, and sync status.
The Timer screen has a Calendar view and a List view. The Calendar view is the default.
The Calendar view shows hour labels in 12-hour format and entries as blocks in their project colour.
Overlapping entries share the day width. Entries that cross midnight show on both days.
A red line marks the current time. The − and + buttons change the hour height.
In the List view, time entries show the description and duration above the project and time range.
Long descriptions and project names shorten to fit the available width. Hover to read the full text.

The widget uses the same colours and starts with one compact row.
On first use, it selects the running entry, then the most recent entry, or shows an empty row.
The ⋮ menu appears at the lower left on hover or keyboard focus. It stays visible while a menu is open.
Its Add task submenu lists recent entries. Add task is disabled when three entries are selected.
The menu also has a Close widget action.
Calendar blocks and List view entries have menus with three vertical dots.
Hover over a calendar block to show its menu.
Each menu has an Add to widget action.
The action is disabled for duplicate selections or a full widget.
Each widget row has a remove button. Removing a row does not change the Toggl entry.
Widget closure and sync preserve selections. Sign-out and app exit clear them.
Selected entries outside the loaded history keep their last known time and show a history notice.
The main button shows Open widget or Close widget to match the current window state.
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
