use super::*;

pub(super) async fn run(
	paths: AppPaths,
	settings: Settings,
	mut commands: mpsc::UnboundedReceiver<Command>,
	sink: EventSink,
) {
	tracing::info!("engine started");
	let mut state = State {
		paths,
		settings,
		sink,
	};
	while let Some(command) = commands.recv().await {
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
