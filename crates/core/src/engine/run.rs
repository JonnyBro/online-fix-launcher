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
	let (unpacked_tx, mut unpacked_rx) = mpsc::unbounded_channel();
	let (protons_tx, mut protons_rx) = mpsc::unbounded_channel();
	let art = crate::art::ArtStore::new(paths.art_dir());
	let mut state = State {
		paths,
		settings,
		sink,
		library,
		library_broken,
		running: HashMap::new(),
		finished: finished_tx,
		unpacked: unpacked_tx,
		art,
		protons_changed: protons_tx,
	};
	state.emit_library();
	state.refresh_protons();
	state.fetch_missing_art();

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
			Some((new, result)) = unpacked_rx.recv() => {
				state.unpacked(new, result);
				continue;
			}
			Some(()) = protons_rx.recv() => {
				state.refresh_protons();
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
			Command::InspectSource {
				request,
				path,
				password,
			} => state.inspect_source(request, path, password),
			Command::AddGame(new) => state.add_new_game(new),
			Command::UpdateGame(game) => state.update_game(*game),
			Command::RemoveGame {
				id,
				delete_game_dir,
				delete_prefix,
			} => state.remove_game(&id, delete_game_dir, delete_prefix),
			Command::Launch(id) => state.launch(&id),
			Command::Stop(id) => state.stop(&id),
			Command::RefreshProtons => state.refresh_protons(),
			Command::ListProtonReleases(provider) => state.list_proton_releases(provider),
			Command::InstallProton(release) => state.install_proton(release),
			Command::RemoveProton(dir) => state.remove_proton(dir),
			Command::FetchArt(id) => state.fetch_art(&id),
			Command::ArtChoices { request, id, kind } => state.art_choices(request, &id, kind),
			Command::SetArt { id, kind, from } => state.set_art(id, kind, from),
			Command::CheckSteamGridDbKey(key) => state.check_sgdb_key(key),
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
