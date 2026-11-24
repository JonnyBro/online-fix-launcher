//! Fetching and replacing game pictures.

use rust_i18n::t;

use super::*;
use crate::art::{self, ArtKind, SteamGridDb};
use crate::library::Game;

impl State {
	/// Pictures for games that have none, once per run.
	pub(super) fn fetch_missing_art(&mut self) {
		let missing: Vec<String> = self
			.library
			.games
			.iter()
			.filter(|g| {
				self.art.find(&g.id, ArtKind::Grid).is_none()
					&& self.art.find(&g.id, ArtKind::Icon).is_none()
			})
			.map(|g| g.id.clone())
			.collect();
		for id in missing {
			self.fetch_art(&id);
		}
	}

	pub(super) fn fetch_art(&self, id: &str) {
		let Some(game) = self.library.get(id).cloned() else {
			return;
		};
		let store = self.art.clone();
		let sink = self.sink.clone();
		let key = self.settings.steamgriddb_key.clone();
		tokio::spawn(async move {
			fetch(&game, &store, &key).await;
			sink.emit(Event::ArtChanged(game.id));
		});
	}

	pub(super) fn art_choices(&self, request: u64, id: &str, kind: ArtKind) {
		let Some(game) = self.library.get(id).cloned() else {
			return;
		};
		let key = self.settings.steamgriddb_key.clone();
		let sink = self.sink.clone();
		let thumbs = self.paths.cache_dir().join("thumbs");
		tokio::spawn(async move {
			let result = async {
				if key.is_empty() {
					return Err(t!("err.sgdb_no_key").into_owned());
				}
				let sgdb = SteamGridDb::new(key);
				let choices = match sgdb.find_game(game.real_app_id, &game.name).await? {
					Some(found) => sgdb.choices(found.id, kind).await?,
					None => Vec::new(),
				};
				Ok(with_thumbs(choices, &thumbs).await)
			}
			.await;
			sink.emit(Event::ArtChoices { request, result });
		});
	}

	pub(super) fn set_art(&self, id: String, kind: ArtKind, from: ArtSource) {
		let store = self.art.clone();
		let sink = self.sink.clone();
		tokio::spawn(async move {
			let saved = match from {
				ArtSource::File(file) => {
					store.copy_from(&id, kind, &file).map_err(|e| e.to_string())
				}
				ArtSource::Url(url) => match art::download(&url).await {
					Ok(Some((bytes, ext))) => store
						.save(&id, kind, &bytes, ext)
						.map_err(|e| e.to_string()),
					Ok(None) => Err(t!("err.not_a_picture").into_owned()),
					Err(e) => Err(e),
				},
			};
			match saved {
				Ok(_) => sink.emit(Event::ArtChanged(id)),
				Err(e) => sink.emit(Event::Error(e)),
			}
		});
	}

	pub(super) fn check_sgdb_key(&self, key: String) {
		let sink = self.sink.clone();
		tokio::spawn(async move {
			sink.emit(Event::SteamGridDbKey(SteamGridDb::new(key).check().await));
		});
	}
}

/// Steam's own pictures, SteamGridDB's top pick for what Steam lacks (with
/// a key), and the exe's icon in any case.
async fn fetch(game: &Game, store: &art::ArtStore, key: &str) {
	let save = |kind: ArtKind, found: Option<(Vec<u8>, &'static str)>| {
		if let Some((bytes, ext)) = found
			&& let Err(e) = store.save(&game.id, kind, &bytes, ext)
		{
			tracing::warn!("saving {kind:?} of {}: {e}", game.name);
		}
	};
	let exe = game.exe.clone();
	if let Ok(Some(icon)) = tokio::task::spawn_blocking(move || art::exe_icon(&exe)).await {
		save(ArtKind::Icon, Some((icon, "png")));
	}
	for kind in [ArtKind::Grid, ArtKind::Hero] {
		let mut found = None;
		if let Some(app_id) = game.real_app_id {
			found = art::first_picture(&art::steam_urls(app_id, kind)).await;
		}
		if found.is_none() && !key.is_empty() {
			match from_sgdb(game, key, kind).await {
				Ok(picked) => found = picked,
				Err(e) => tracing::warn!("SteamGridDB, {}: {e}", game.name),
			}
		}
		if found.is_none() {
			tracing::info!("no {kind:?} picture for {}", game.name);
		}
		save(kind, found);
	}
}

/// How many pictures the gallery shows.
const GALLERY_SIZE: usize = 24;

/// Downloads the previews side by side; ones that fail are left out.
async fn with_thumbs(choices: Vec<art::Choice>, dir: &std::path::Path) -> Vec<ArtOption> {
	let _ = std::fs::create_dir_all(dir);
	let mut tasks = tokio::task::JoinSet::new();
	for (ix, choice) in choices.into_iter().take(GALLERY_SIZE).enumerate() {
		let dir = dir.to_path_buf();
		tasks.spawn(async move {
			let (bytes, ext) = art::download(&choice.thumb).await.ok()??;
			let name: String = choice
				.thumb
				.rsplit('/')
				.next()
				.unwrap_or("thumb")
				.chars()
				.filter(|c| c.is_ascii_alphanumeric())
				.collect();
			let path = dir.join(format!("{name}.{ext}"));
			std::fs::write(&path, bytes).ok()?;
			Some((
				ix,
				ArtOption {
					choice,
					thumb: path,
				},
			))
		});
	}
	let mut done: Vec<(usize, ArtOption)> = tasks.join_all().await.into_iter().flatten().collect();
	done.sort_by_key(|(ix, _)| *ix);
	done.into_iter().map(|(_, option)| option).collect()
}

async fn from_sgdb(
	game: &Game,
	key: &str,
	kind: ArtKind,
) -> Result<Option<(Vec<u8>, &'static str)>, String> {
	let sgdb = SteamGridDb::new(key);
	let Some(found) = sgdb.find_game(game.real_app_id, &game.name).await? else {
		return Ok(None);
	};
	let urls: Vec<String> = sgdb
		.choices(found.id, kind)
		.await?
		.into_iter()
		.take(3)
		.map(|c| c.url)
		.collect();
	Ok(art::first_picture(&urls).await)
}
