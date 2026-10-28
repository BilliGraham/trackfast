# Native sync setup

This step saves an account binding, a first-sync boundary, and explicit project mappings in the native database.
It does not send time entries or change API projects. Queue delivery remains the next step.

## Prepare new work

1. Start the local API and its database.
2. Start the configured native app and sign in.
3. Open Settings and find **Time-entry sync setup**.
4. Read the explanation and select **Prepare new work for sync**.

The account worker checks the API identity before the database worker saves the binding.
The binding contains the API address, tenant ID, and person ID. It contains no credentials.
The database transaction saves the greatest entry creation sequence as the boundary.
All entries that exist at that point remain local, including running timers.
Work created while the identity check runs also remains local if it exists before that transaction.
New backdated manual entries remain eligible because creation order, not the entry date, determines the boundary.
Imported history remains local. Running timers are not counted as completed work.

Restarting, signing out, or repeating setup with the same account does not move the boundary.
The database rejects another account, tenant, or API address. Account replacement is not supported in this step.
The card shows the number of new completed local entries. This number is not a delivery confirmation.

## Map projects

1. Select **Refresh API projects**.
2. Select an unmapped local project.
3. Select its API project.
4. Check both selections and select **Save mapping**.

The account worker follows project pages through the authenticated session client.
It rejects repeated pages and duplicate project IDs. The pilot limit is ten pages of 100 projects.
A failed or incomplete list is not published as a usable catalog.
Before saving, the worker checks the identity and fetches the project list again.
The selected API project must still be accessible and active. The local database also checks its account binding and local project state.
These checks run outside the interface thread. The mapping write is one transaction.
Later delivery must check API permissions again because access can change after setup.

There is no automatic mapping by project name. Archived projects cannot receive new mappings.
Saved mappings cannot be reassigned in this pilot step. This prevents pending work from silently changing destination.
The planned delivery step must retain work with an unmapped project until a mapping exists.
Entries without a project will use the API's unassigned entry option.

## Validation

The enrollment tests failed before the storage methods existed. The interface test failed before the setup card existed.
Tests cover a fixed boundary, account mismatch, invalid scopes, explicit mappings, archived projects, and persistence after a file reopen.
They also cover old running timers, new completed timers, imported history, and backdated work.
Layout and click tests cover widths of 340 and 600 pixels. Mapping controls are checked in both themes.
The native visual helper failed during initialization and after reset. A live visual check remains required.
On October 28, 2026, all 92 native tests passed on Windows and Linux, including seven timer tests.
Windows formatting and Clippy passed. Linux compiled successfully.
The running Windows app locked the standard executable. A separate executable was compiled with `cargo rustc`.
macOS checks did not run. Live enrollment and API project selection remain user checks.

## Next step

Connect the native database to the durable queue with replay-safe transfer and confirmation.
Preserve recorded rounding, stable entry keys, edits, and deletion records across failures.
Do not treat these setup records as evidence that any entry reached the API.
