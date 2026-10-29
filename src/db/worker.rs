//! Runs every database call on one background thread.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use eframe::egui;
use jiff::Timestamp;

use super::{Import, ImportCount, ProjectDraft, Store};
use crate::entries::{Snapshot, Task};

pub enum Command {
    InspectDelivery(trackfast_sync::Scope),
    ApplyRemote(
        trackfast_sync::Scope,
        Box<trackfast_sync::native_download::Incoming>,
    ),
    ApplyResolution(
        trackfast_sync::Scope,
        uuid::Uuid,
        Box<trackfast_sync::native_resolution::Resolution>,
    ),
    DeliveryAttention(trackfast_sync::Scope, uuid::Uuid),
    PrepareDelivery(trackfast_sync::Scope),
    ConfirmDelivery(trackfast_sync::Scope, uuid::Uuid),
    Enroll(trackfast_sync::Scope),
    MapProject(trackfast_sync::Scope, i64, uuid::Uuid),
    AddTask(Task, Timestamp),
    StartEntry(Task, Timestamp),
    SaveEntry(crate::entries::Session),
    DeleteEntry(i64),
    SaveSettings(crate::entries::Settings),
    Play(i64, Timestamp),
    Pause(i64, Timestamp),
    RemoveRow(i64, Timestamp),
    SaveProject(ProjectDraft),
    SetArchived(i64, bool),
    /// Loads sessions from this time onward.
    LoadFrom(Timestamp),
    Import(Import),
}

#[derive(Clone, Copy)]
pub enum Saved {
    Started,
    Entry,
    Settings,
    Deletion,
}

pub enum Event {
    DeliveryInspected(
        trackfast_sync::Scope,
        Option<Box<trackfast_sync::transfer::Transfer>>,
    ),
    RemoteApplied(
        trackfast_sync::Scope,
        Box<trackfast_sync::native_download::Incoming>,
        Box<trackfast_sync::native_download::RemoteOutcome>,
    ),
    DeliveryPrepared(
        trackfast_sync::Scope,
        Option<Box<trackfast_sync::transfer::Transfer>>,
    ),
    DeliveryConfirmed,
    DeliveryFailed(String),
    SetupSaved,
    SetupFailed(String),
    Saved(Saved),
    Changed(Box<Snapshot>),
    Imported(ImportCount),
    Failed(String),
}

#[derive(Clone)]
pub struct Database {
    commands: Sender<Command>,
}

pub struct Events(Receiver<Event>);

impl Events {
    pub fn try_recv(&self) -> Option<Event> {
        self.0.try_recv().ok()
    }
}

impl Database {
    pub fn start(path: PathBuf, loaded_from: Timestamp, ctx: egui::Context) -> (Self, Events) {
        let (commands, command_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        thread::Builder::new()
            .name("database".into())
            .spawn(move || run(path, loaded_from, command_rx, event_tx, ctx))
            .expect("the operating system must allow a new thread");
        (Self { commands }, Events(events))
    }

    pub fn send(&self, command: Command) {
        // A send fails only after the worker stops, and then the app has
        // already shown the reason.
        let _ = self.commands.send(command);
    }
}

fn run(
    path: PathBuf,
    mut loaded_from: Timestamp,
    commands: Receiver<Command>,
    events: Sender<Event>,
    ctx: egui::Context,
) {
    let notify = |event| {
        if events.send(event).is_ok() {
            ctx.request_repaint();
        }
    };
    let mut store = match Store::open(&path) {
        Ok(store) => store,
        Err(error) => {
            notify(Event::Failed(format!(
                "Trackfast cannot open {}: {error}",
                path.display()
            )));
            return;
        }
    };
    let publish = |store: &Store, loaded_from| match store.snapshot(loaded_from) {
        Ok(snapshot) => notify(Event::Changed(Box::new(snapshot))),
        Err(error) => notify(Event::Failed(error.to_string())),
    };
    publish(&store, loaded_from);
    while let Ok(command) = commands.recv() {
        let command = match command {
            Command::InspectDelivery(scope) => {
                match store.inspect_delivery(&scope) {
                    Ok(transfer) => notify(Event::DeliveryInspected(scope, transfer.map(Box::new))),
                    Err(error) => notify(Event::DeliveryFailed(error.to_string())),
                }
                continue;
            }
            Command::ApplyRemote(scope, incoming) => {
                match store.apply_remote(&scope, &incoming) {
                    Ok(outcome) => notify(Event::RemoteApplied(scope, incoming, Box::new(outcome))),
                    Err(error) => notify(Event::DeliveryFailed(error.to_string())),
                }
                publish(&store, loaded_from);
                continue;
            }
            Command::ApplyResolution(scope, operation, resolution) => {
                match store.apply_resolution(&scope, operation, &resolution) {
                    Ok(()) => notify(Event::DeliveryConfirmed),
                    Err(error) => notify(Event::DeliveryFailed(error.to_string())),
                }
                publish(&store, loaded_from);
                continue;
            }
            Command::DeliveryAttention(scope, operation) => {
                if let Err(error) = store.mark_delivery_attention(&scope, operation) {
                    notify(Event::DeliveryFailed(error.to_string()));
                }
                publish(&store, loaded_from);
                continue;
            }
            Command::PrepareDelivery(scope) => {
                match store.prepare_delivery(&scope) {
                    Ok(transfer) => notify(Event::DeliveryPrepared(scope, transfer.map(Box::new))),
                    Err(error) => notify(Event::DeliveryFailed(error.to_string())),
                }
                continue;
            }
            Command::ConfirmDelivery(scope, operation) => {
                match store.confirm_delivery(&scope, operation) {
                    Ok(()) => notify(Event::DeliveryConfirmed),
                    Err(error) => notify(Event::DeliveryFailed(error.to_string())),
                }
                publish(&store, loaded_from);
                continue;
            }
            other => other,
        };
        let setup = matches!(&command, Command::Enroll(_) | Command::MapProject(..));
        let saved = match &command {
            Command::StartEntry(_, _) => Some(Saved::Started),
            Command::SaveEntry(_) => Some(Saved::Entry),
            Command::SaveSettings(_) => Some(Saved::Settings),
            Command::DeleteEntry(_) => Some(Saved::Deletion),
            _ => None,
        };
        let result = match command {
            Command::InspectDelivery(_)
            | Command::ApplyRemote(..)
            | Command::ApplyResolution(..)
            | Command::PrepareDelivery(_)
            | Command::ConfirmDelivery(..)
            | Command::DeliveryAttention(..) => unreachable!(),
            Command::Enroll(scope) => store.enroll(&scope),
            Command::MapProject(scope, local, remote) => store.map_project(&scope, local, remote),
            Command::SaveEntry(entry) => store.save_entry(&entry),
            Command::DeleteEntry(id) => store.delete_entry(id),
            Command::SaveSettings(settings) => store.save_settings(&settings),
            Command::StartEntry(task, at) => store.add_task(&task, at),
            Command::AddTask(task, at) => store.add_task(&task, at),
            Command::Play(row, at) => store.play(row, at),
            Command::Pause(row, at) => store.pause(row, at),
            Command::RemoveRow(row, at) => store.remove_row(row, at),
            Command::SaveProject(draft) => store.save_project(&draft).map(|_| ()),
            Command::SetArchived(project, archived) => store.set_archived(project, archived),
            Command::LoadFrom(from) => {
                loaded_from = loaded_from.min(from);
                Ok(())
            }
            Command::Import(data) => store.import(&data).map(|count| {
                notify(Event::Imported(count));
            }),
        };
        if let Some(saved) = saved
            && result.is_ok()
        {
            notify(Event::Saved(saved));
        }
        if setup {
            match result {
                Ok(()) => notify(Event::SetupSaved),
                Err(error) => notify(Event::SetupFailed(error.to_string())),
            }
        } else if let Err(error) = result {
            notify(Event::Failed(error.to_string()));
        }
        publish(&store, loaded_from);
    }
}
