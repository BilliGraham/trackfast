# Interface design

Trackfast uses dark neutral surfaces, small headings, and muted pink selection accents.
Project colours identify projects. They do not decorate unrelated controls.

The navigation uses full-width rows. The main view separates timer controls, totals, and sync status.
Time entries show the description and duration above the project and time range.
Long descriptions and project names shorten to fit the available width. Hover to read the full text.

The widget uses the same colours and three compact task rows.
Disabled controls explain their current limit on hover.
Empty screens use short messages without development plans or sample account data.

Keep theme values in `src/ui/style.rs`.
Check the main view at 1150 by 720 and 760 by 480 logical pixels.
Check the widget at its minimum size of 300 by 250 logical pixels.
Keep review captures outside the repository.
