# Timer model: first TDD step

The first step adds one named Task with an independent timer.
It uses only the Rust standard library. It has no system-clock dependency.

## Read the Rust code

- Task is a struct. It keeps a description and its timer state together.
- TimerState is an enum. A timer can be stopped or running.
- impl Task defines the operations available on a task.
- Duration stores elapsed time and timestamps from one monotonic origin.
- The tests module contains the seven acceptance tests.

Start with the tests at the end of src/timer.rs.
Then read new, start, stop, and elapsed_at.

## Behaviour

| Test | Expected result |
| --- | --- |
| Create a task | Stopped, with zero elapsed time. |
| Start at 100 seconds and check at 130 | 30 seconds elapsed. |
| Stop at 140 and check at 200 | 40 seconds elapsed. |
| Resume at 210 and stop at 230 | 60 seconds accumulated. The pause does not count. |
| Run two overlapping tasks and stop one | The other task continues independently. |
| Start an already running task | The active interval keeps its original start. |
| Stop an already stopped task | Accumulated time stays unchanged. |

Each task owns its state. There is no global active timer.
The interface will enforce the three-slot limit in a later step.

## Time contract

The caller supplies time explicitly. Use timestamps from a common monotonic origin.
Do not pass Unix timestamps or calendar dates to this model.
A running timer rejects an elapsed-time read or stop before its start with a panic.
Tests use fixed timestamps. They need no sleeps and cannot fail because a machine runs slowly.
Clock sampling, sleep handling, restart recovery, and stored dates require later integration work.

## TDD sequence

| Step | Result before implementation | Result after implementation |
| --- | --- | --- |
| New task | Compilation failed because Task did not exist. | 1 test passed. |
| Running time | Compilation failed because start did not exist. | 2 tests passed. |
| Stop | Compilation failed because stop did not exist. | 3 tests passed. |
| Resume | The test returned 20 seconds instead of 60. | 4 tests passed. |
| Overlap | Existing behaviour passed the new test. | 5 tests passed without an implementation change. |
| Repeated start | The test returned 10 seconds instead of 30. | 6 tests passed. |
| Repeated stop | Existing behaviour passed the new test. | 7 tests passed without an implementation change. |

The final refactor kept all seven tests green.
Some tests already passed because earlier implementation steps supplied the required behaviour.
No defect was added to manufacture a failing test.

## Run the tests

After dependency resolution, use:

    cargo test --locked --lib

The library source uses no external crates.
You can also compile it directly with a configured Rust toolchain and native linker:

    rustc --edition=2024 --crate-name trackfast --test src/lib.rs -o target/timer-tests.exe
    ./target/timer-tests.exe

Create the target directory first if it does not exist.
The direct command above uses a Windows executable name.
On other platforms, select a suitable output filename.

## Platforms

Linux and Windows are the main targets. macOS has lower priority.
The timer model has no platform-specific code.
CI compiles and runs the standalone tests on Linux and Windows before desktop dependencies are needed.
Local tests passed on Windows with Rust 1.98.0 and its GNU toolchain.
Linux and macOS execution remain unverified in this session.

## Current limit

This step does not change the interface or connect a Toggl account.
The next proposed step connects one widget row to this model.
