//! Background side of the app: the UI sends [`Command`]s, the engine works on a
//! tokio runtime in its own thread and reports back with [`Event`]s.

mod adding;
mod art;
mod games;
mod messages;
mod protons;
mod run;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use std::thread::JoinHandle;

use tokio::sync::mpsc;

use crate::error::{Error, Result};
use crate::library::Library;
use crate::paths::AppPaths;
use crate::settings::Settings;

pub use messages::{
	ArtOption, ArtSource, Command, Event, InspectError, NewGame, PendingStage, RunState, Source,
	SourceKind, Unpack,
};

/// Called after every event so the UI can wake up and repaint.
pub type Notifier = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub(crate) struct EventSink {
	tx: std_mpsc::Sender<Event>,
	notify: Notifier,
}

impl EventSink {
	pub(crate) fn emit(&self, event: Event) {
		if self.tx.send(event).is_ok() {
			(self.notify)();
		}
	}
}

pub struct Engine {
	commands: Option<mpsc::UnboundedSender<Command>>,
	events: std_mpsc::Receiver<Event>,
	thread: Option<JoinHandle<()>>,
}

impl Engine {
	pub fn start(paths: AppPaths, settings: Settings, notify: Notifier) -> Result<Self> {
		let runtime = tokio::runtime::Builder::new_multi_thread()
			.enable_all()
			.thread_name("engine-worker")
			.build()
			.map_err(Error::Runtime)?;

		let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
		let (event_tx, event_rx) = std_mpsc::channel();
		let sink = EventSink {
			tx: event_tx,
			notify,
		};

		let thread = std::thread::Builder::new()
			.name("engine".into())
			.spawn(move || runtime.block_on(run::run(paths, settings, cmd_rx, sink)))
			.map_err(Error::Runtime)?;

		Ok(Self {
			commands: Some(cmd_tx),
			events: event_rx,
			thread: Some(thread),
		})
	}

	pub fn send(&self, command: Command) {
		if let Some(tx) = &self.commands
			&& tx.send(command).is_err()
		{
			tracing::error!("engine is not running, command dropped");
		}
	}

	pub fn poll_events(&self) -> impl Iterator<Item = Event> + '_ {
		self.events.try_iter()
	}
}

impl Drop for Engine {
	fn drop(&mut self) {
		// Closing the channel ends the command loop, which lets the thread exit.
		self.commands.take();
		if let Some(thread) = self.thread.take()
			&& thread.join().is_err()
		{
			tracing::error!("engine thread panicked");
		}
	}
}

struct State {
	paths: AppPaths,
	settings: Settings,
	sink: EventSink,
	library: Library,
	/// The game list couldn't be read; saving would lose it.
	library_broken: bool,
	running: HashMap<String, games::Running>,
	finished: mpsc::UnboundedSender<games::Finished>,
	unpacked: mpsc::UnboundedSender<adding::AddStep>,
	art: crate::art::ArtStore,
	/// A build was installed or removed; the list is read again.
	protons_changed: mpsc::UnboundedSender<()>,
}

#[cfg(test)]
mod tests {
	use std::time::{Duration, Instant};

	use super::*;

	#[test]
	fn save_settings_roundtrip_through_engine() {
		let dir = tempfile::tempdir().unwrap();
		let paths = AppPaths::under(dir.path());
		let engine = Engine::start(paths.clone(), Settings::default(), Arc::new(|| {})).unwrap();

		let settings = Settings {
			check_app_updates: false,
			..Settings::default()
		};
		engine.send(Command::SaveSettings(settings.clone()));

		let deadline = Instant::now() + Duration::from_secs(5);
		let saved = loop {
			assert!(Instant::now() < deadline, "no SettingsSaved event");
			if let Some(s) = engine.poll_events().find_map(|e| match e {
				Event::SettingsSaved(s) => Some(s),
				_ => None,
			}) {
				break s;
			}
			std::thread::sleep(Duration::from_millis(10));
		};
		assert_eq!(saved, settings);
		assert_eq!(Settings::load(&paths).unwrap(), settings);
	}
}
