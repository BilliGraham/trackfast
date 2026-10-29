# Local entries and settings

The native app keeps its data in SQLite. These changes do not connect it to the API, sync queue, or Xero.

## Enter and correct time

Select **Add entry**, or press Ctrl+N (Cmd+N on macOS).
Choose **Duration only** or **Start and stop**.
The form expands to fit its fields and buttons without scrolling.
A duration uses H:MM or H:MM:SS. Three hours remains one entry.
A timed entry uses start and stop dates, local times, and an IANA time zone.
The time zone defaults to the system zone when it has an IANA name. Otherwise it defaults to UTC.
Dates use YYYY-MM-DD. Times use HH:MM or HH:MM:SS.
The stop must be later than the start. Use a later stop date for work across midnight.
Repeated or skipped local times at daylight-saving changes are rejected. The form does not guess which instant the user means.
The local store accepts whole seconds and positive durations of no more than ten years.

The form shows recorded time and the effective total before saving.
Press Ctrl+Enter or Cmd+Enter to save. Press Escape to cancel.
A failed save retains the draft and shows the error. A successful save shows confirmation.

Use the entry menu to edit completed timed entries. Select a duration entry above the calendar to edit it.
The list menu also provides deletion with confirmation. Stop a running timer before editing or deleting it.
New or edited project assignments require an active project.

## Add time in Day blocks

Select **Add time** in the Timer header while Day blocks is selected.
The form defaults to today in the local time zone, even when an older week is visible.
Choose 30 minutes, one, two, three, or four hours, or enter a custom duration.
Select **Add time** to save one completed duration entry. No start or stop time is required.
Description and project are optional. A blank description is saved as **Work**.
The date remains editable. The per-day **Add duration** buttons use the same form for their selected day.
Ctrl+N or Cmd+N opens this form from Day blocks. Ctrl+Enter or Cmd+Enter saves it.
After saving, Day blocks shows the week that contains the saved date.

## Rounding and totals

The default rounds new entries to the nearest five minutes. Exact half-intervals round up.
With rounding off, the effective total equals the recorded seconds.
For example, 09:03 to 09:16 records 13 minutes. Five-minute rounding gives a 15-minute total.
The original timestamps and duration stay unchanged. Calendar positions use the recorded times.
A zero rounded total stays visible. No export exists in the local app yet.

Settings changes apply to new entries only. A running timer captures the setting when it starts.
Editing an existing entry retains its applied rounding rule.
Entries from the old schema and imported entries keep exact totals.
Day, week, project, and paused-widget totals use effective time. Running timers show live recorded time.
New entries retain their start date even if the display time zone changes.
Calendar segments can cross midnight, but storage and totals keep one entry on its start date.
Duration entries appear separately above the calendar and in the list, without invented working hours.

## Weekly target

Settings contains a weekly target in hours, defaulting to 35.
The app shows a progress bar. The former week-glass choice is removed.
Existing saved glass preferences remain readable but do not change the indicator.
The indicator fills up to the target. The text still shows time above the target.
The calendar and day blocks use the selected week. The list uses the current week.
Calendar holidays and leave do not adjust the target in this step.

## Calendar gestures and day blocks

In Calendar, drag on empty space to select a completed time range. The selection uses five-minute steps.
The entry form opens with that range. Review it, add the description and project, then save.
An upward drag works too. Escape cancels the selection. A click prepares a 30-minute entry at that position.
Select **Start now** in the form to start a live timer instead. This uses the current time, not the selected range.
The form stays open if the start fails. The widget opens after the worker confirms the start.
Click a completed calendar block to edit it. Existing blocks do not start a new selection.

Select **Day blocks** beside Calendar and List for work grouped by day without an hour grid.
Each day has duration cards and an Add duration button. Three hours remains one card.
This view includes both timed and duration-only entries. It hides start and stop times without changing stored data.
Use Start on a card to continue its task now, or Pause on a running card.
The view scrolls horizontally on narrow windows. Week navigation and the progress bar use the selected week.


## Validation

New storage and form checks failed before their implementations existed.
A later timestamp check failed because the store accepted fractional seconds. The store now rejects them.
A list-rendering check failed because duration entries showed a time range. They now show their mode and recorded duration.
A week-loading check failed when the UTC anchor preceded the saved date. The query now also uses the saved date.
Tests cover migration, restart, exact and rounded totals, zero totals, settings changes during a timer,
entry correction and deletion, invalid input, midnight, ambiguous times, saved dates, and keyboard saving.
The keyboard and saved-date regression checks passed when added. No defect was introduced to force a failure.

Debug builds accept `TRACKFAST_REVIEW_DATABASE` to use an isolated database for interface checks.
Release builds ignore this variable. Review data and captures stay outside the repository.

The calendar selection and day-view checks failed before their implementations existed.
Pointer tests cover dragging, clicking, and Escape cancellation.
Windows and Linux each passed 66 native tests, including the seven standalone timer tests.
Formatting, Clippy with warnings denied, and native builds passed on both platforms.
Linux visual checks used an isolated database and a virtual display at 1150 by 720 and 760 by 480 pixels.
Earlier checks covered the calendar, list, settings, entry forms, keyboard saving, and widget opening and closing.
The latest checks covered calendar dragging, day blocks, duration saving, and Start now with widget opening.
The 09:03 to 09:16 example showed 13 recorded minutes and a 15-minute effective total.
The Windows UI helper failed during sandbox setup, so no Windows visual check ran.
macOS was not compiled, tested, or visually checked in this step. The app supports its existing dark theme only.

The later form-size fix passed 67 native tests on Windows, plus formatting, Clippy, and a build.
Its new layout check failed before the fix and passed afterward at 1150 by 720 and 760 by 480 pixels.
Linux and macOS checks were not repeated for this layout fix.

## Day blocks form validation

On October 29, 2026, the new draft and shortcut tests failed before implementation.
The form test selects the three-hour preset and saves one duration entry for the local day.
It checks both themes at 760 × 480 and 1150 × 720 pixels, with visible save and cancel controls.
Windows and Linux passed all 120 native tests, including the seven timer tests. Both builds passed.
Windows formatting and Clippy checks passed. The live visual helper failed during initialization on both attempts.
Live visual checks and macOS checks did not run.
