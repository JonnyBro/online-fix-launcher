use super::*;

pub(super) async fn run(
	paths: AppPaths,
	settings: Settings,
	mut commands: mpsc::UnboundedReceiver<Command>,
	sink: EventSink,
) {
	tracing::info!("engine started");
	let (library, library_broken) = match Library::load(&paths) {
		Ok(library) => (library, false),
		Err(e) => {
			sink.emit(Event::Error(e.to_string()));
			(Library::default(), true)
		}
	};
	let (finished_tx, mut finished_rx) = mpsc::unbounded_channel();
	let mut state = State {
		paths,
		settings,
		sink,
		library,
		library_broken,
		running: HashMap::new(),
		finished: finished_tx,
	};
	state.emit_library();
	state.refresh_protons();

	loop {
		let command = tokio::select! {
			command = commands.recv() => match command {
				Some(command) => command,
				None => break,
			},
			Some(finished) = finished_rx.recv() => {
				state.finished(finished);
				continue;
			}
		};
		match command {
			Command::SaveSettings(settings) => state.save_settings(settings).await,
			Command::CheckAppUpdate => {
				let sink = state.sink.clone();
				tokio::spawn(async move {
					match crate::app_update::check().await {
						Ok(Some(release)) => sink.emit(Event::AppUpdate(release)),
						Ok(None) => {}
						Err(e) => tracing::warn!("update check failed: {e}"),
					}
				});
			}
			Command::AddGame { exe } => state.add_game(exe),
			Command::UpdateGame(game) => state.update_game(*game),
			Command::RemoveGame {
				id,
				delete_game_dir,
				delete_prefix,
			} => state.remove_game(&id, delete_game_dir, delete_prefix),
			Command::Launch(id) => state.launch(&id),
			Command::Stop(id) => state.stop(&id),
			Command::RefreshProtons => state.refresh_protons(),
		}
	}
	tracing::info!("engine stopped");
}

impl State {
	async fn save_settings(&mut self, settings: Settings) {
		let paths = self.paths.clone();
		let to_save = settings.clone();
		let result = tokio::task::spawn_blocking(move || to_save.save(&paths)).await;
		match result {
			Ok(Ok(())) => {
				self.settings = settings.clone();
				self.sink.emit(Event::SettingsSaved(settings));
			}
			Ok(Err(e)) => self.sink.emit(Event::SettingsSaveFailed(e.to_string())),
			Err(e) => self.sink.emit(Event::SettingsSaveFailed(e.to_string())),
		}
	}
}
