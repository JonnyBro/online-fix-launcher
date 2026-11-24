//! Downloading and removing GE-Proton builds.

use rust_i18n::t;

use super::*;
use crate::proton_releases::{self, Provider, Release};

impl State {
	pub(super) fn list_proton_releases(&self, provider: Provider) {
		let sink = self.sink.clone();
		tokio::spawn(async move {
			let result = proton_releases::releases(provider).await;
			sink.emit(Event::ProtonReleases { provider, result });
		});
	}

	pub(super) fn install_proton(&self, release: Release) {
		let sink = self.sink.clone();
		let done = self.protons_changed.clone();
		let dir = self.paths.protons_dir();
		tokio::spawn(async move {
			let tag = release.tag.clone();
			let report = |progress: Option<f32>| {
				sink.emit(Event::ProtonInstalling {
					tag: tag.clone(),
					progress,
				})
			};
			report(Some(0.));
			let result = proton_releases::install(&release, &dir, |p| {
				report(if p >= 1. { None } else { Some(p) })
			})
			.await;
			sink.emit(Event::ProtonInstallEnded(tag.clone()));
			match result {
				Ok(path) => {
					tracing::info!("installed {} into {}", tag, path.display());
					sink.emit(Event::Notice(
						t!("proton.installed", name = tag).into_owned(),
					));
				}
				Err(e) => sink.emit(Event::Error(
					t!("err.proton_install", name = tag, error = e).into_owned(),
				)),
			}
			let _ = done.send(());
		});
	}

	pub(super) fn remove_proton(&mut self, dir: PathBuf) {
		let own = self.paths.protons_dir();
		if !dir.starts_with(&own) || dir == own {
			self.sink
				.emit(Event::Error(t!("err.proton_not_ours").into_owned()));
			return;
		}
		// Games on this build go back to the default one.
		let users: Vec<_> = self
			.library
			.games
			.iter()
			.filter(|g| g.proton.as_deref() == Some(&dir))
			.cloned()
			.collect();
		for mut game in users {
			game.proton = None;
			self.library.upsert(game);
		}
		if self.settings.default_proton.as_deref() == Some(&dir) {
			let mut settings = self.settings.clone();
			settings.default_proton = None;
			self.settings = settings.clone();
			if settings.save(&self.paths).is_ok() {
				self.sink.emit(Event::SettingsSaved(settings));
			}
		}
		self.save_library();
		self.emit_library();
		let sink = self.sink.clone();
		let done = self.protons_changed.clone();
		tokio::task::spawn_blocking(move || {
			if let Err(e) = std::fs::remove_dir_all(&dir) {
				sink.emit(Event::Error(
					t!("err.delete_failed", path = dir.display(), error = e).into_owned(),
				));
			}
			let _ = done.send(());
		});
	}
}
