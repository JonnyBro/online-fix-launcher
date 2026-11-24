//! Game pictures: a portrait cover for the grid, a wide hero for the
//! details, and the exe's own icon as a last resort. They come from
//! SteamGridDB when the user has a key, else from Steam's CDN.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rust_i18n::t;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtKind {
	/// 600x900, for the grid.
	Grid,
	/// Wide banner, for the details.
	Hero,
	/// From the exe, shown when there is no grid picture.
	Icon,
}

impl ArtKind {
	fn file_prefix(self) -> &'static str {
		match self {
			Self::Grid => "grid-",
			Self::Hero => "hero-",
			Self::Icon => "icon-",
		}
	}
}

/// `<data>/art/<game id>/`, one file per kind. Names change on every save
/// so the UI's image cache never shows the old picture.
#[derive(Debug, Clone)]
pub struct ArtStore {
	root: PathBuf,
}

impl ArtStore {
	pub fn new(root: PathBuf) -> Self {
		Self { root }
	}

	pub fn dir(&self, game_id: &str) -> PathBuf {
		self.root.join(game_id)
	}

	pub fn find(&self, game_id: &str, kind: ArtKind) -> Option<PathBuf> {
		let entries = std::fs::read_dir(self.dir(game_id)).ok()?;
		entries
			.flatten()
			.map(|e| e.path())
			.filter(|p| {
				p.file_name()
					.and_then(|n| n.to_str())
					.is_some_and(|n| n.starts_with(kind.file_prefix()))
			})
			.max()
	}

	pub fn save(
		&self,
		game_id: &str,
		kind: ArtKind,
		bytes: &[u8],
		ext: &str,
	) -> std::io::Result<PathBuf> {
		let dir = self.dir(game_id);
		std::fs::create_dir_all(&dir)?;
		let old = self.find(game_id, kind);
		let stamp = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map_or(0, |d| d.as_millis());
		let path = dir.join(format!("{}{stamp}.{ext}", kind.file_prefix()));
		std::fs::write(&path, bytes)?;
		if let Some(old) = old
			&& old != path
		{
			let _ = std::fs::remove_file(old);
		}
		Ok(path)
	}

	pub fn copy_from(&self, game_id: &str, kind: ArtKind, file: &Path) -> std::io::Result<PathBuf> {
		let bytes = std::fs::read(file)?;
		let ext = file
			.extension()
			.and_then(|e| e.to_str())
			.map(str::to_ascii_lowercase)
			.unwrap_or_else(|| "png".into());
		self.save(game_id, kind, &bytes, &ext)
	}

	pub fn remove_all(&self, game_id: &str) {
		let _ = std::fs::remove_dir_all(self.dir(game_id));
	}
}

/// The extension for downloaded bytes, by their first bytes.
fn sniff_ext(bytes: &[u8]) -> Option<&'static str> {
	match bytes {
		[0x89, b'P', b'N', b'G', ..] => Some("png"),
		[0xFF, 0xD8, 0xFF, ..] => Some("jpg"),
		[
			b'R',
			b'I',
			b'F',
			b'F',
			_,
			_,
			_,
			_,
			b'W',
			b'E',
			b'B',
			b'P',
			..,
		] => Some("webp"),
		_ => None,
	}
}

/// Downloads a picture; `None` for anything that isn't one (404 pages included).
pub async fn download(url: &str) -> Result<Option<(Vec<u8>, &'static str)>, String> {
	let client = crate::http::client()?;
	let response = client.get(url).send().await.map_err(|e| e.to_string())?;
	if !response.status().is_success() {
		return Ok(None);
	}
	let bytes = response.bytes().await.map_err(|e| e.to_string())?;
	Ok(sniff_ext(&bytes).map(|ext| (bytes.to_vec(), ext)))
}

/// Steam's own pictures of an app; newer apps only have the first host.
pub fn steam_urls(app_id: u32, kind: ArtKind) -> Vec<String> {
	let files: &[&str] = match kind {
		ArtKind::Grid => &["library_600x900.jpg"],
		ArtKind::Hero => &["library_hero.jpg", "header.jpg"],
		ArtKind::Icon => &[],
	};
	files
		.iter()
		.flat_map(|file| {
			[
				format!(
					"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{app_id}/{file}"
				),
				format!("https://cdn.akamai.steamstatic.com/steam/apps/{app_id}/{file}"),
			]
		})
		.collect()
}

/// The first of `urls` that gives a picture.
pub async fn first_picture(urls: &[String]) -> Option<(Vec<u8>, &'static str)> {
	for url in urls {
		match download(url).await {
			Ok(Some(found)) => return Some(found),
			Ok(None) => {}
			Err(e) => tracing::debug!("{url}: {e}"),
		}
	}
	None
}

/// The exe's largest icon as PNG.
pub fn exe_icon(exe: &Path) -> Option<Vec<u8>> {
	let map = pelite::FileMap::open(exe).ok()?;
	let file = pelite::PeFile::from_bytes(&map).ok()?;
	let resources = file.resources().ok()?;
	let (_, group) = resources.icons().flatten().next()?;
	let mut ico = Vec::new();
	group.write(&mut ico).ok()?;
	let image = image::load_from_memory_with_format(&ico, image::ImageFormat::Ico).ok()?;
	let mut png = std::io::Cursor::new(Vec::new());
	image.write_to(&mut png, image::ImageFormat::Png).ok()?;
	Some(png.into_inner())
}

const SGDB_API: &str = "https://www.steamgriddb.com/api/v2";

/// One picture SteamGridDB offers.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Choice {
	pub url: String,
	pub thumb: String,
	#[serde(default)]
	pub width: u32,
	#[serde(default)]
	pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SgdbGame {
	pub id: u64,
	pub name: String,
}

#[derive(Deserialize)]
struct Reply<T> {
	success: bool,
	data: Option<T>,
}

pub struct SteamGridDb {
	key: String,
}

impl SteamGridDb {
	pub fn new(key: impl Into<String>) -> Self {
		Self { key: key.into() }
	}

	async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<Option<T>, String> {
		let client = crate::http::client()?;
		let response = client
			.get(format!("{SGDB_API}{path}"))
			.bearer_auth(&self.key)
			.send()
			.await
			.map_err(|e| e.to_string())?;
		match response.status().as_u16() {
			401 | 403 => return Err(t!("err.sgdb_key").into_owned()),
			404 => return Ok(None),
			_ => {}
		}
		let text = response
			.error_for_status()
			.map_err(|e| e.to_string())?
			.text()
			.await
			.map_err(|e| e.to_string())?;
		let reply: Reply<T> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
		Ok(if reply.success { reply.data } else { None })
	}

	/// Whether the key is accepted.
	pub async fn check(&self) -> Result<(), String> {
		self.get::<Vec<SgdbGame>>("/search/autocomplete/portal")
			.await
			.map(|_| ())
	}

	/// The game by its Steam id, else the best name match.
	pub async fn find_game(
		&self,
		app_id: Option<u32>,
		name: &str,
	) -> Result<Option<SgdbGame>, String> {
		if let Some(id) = app_id
			&& let Some(game) = self.get::<SgdbGame>(&format!("/games/steam/{id}")).await?
		{
			return Ok(Some(game));
		}
		let term = percent_encode(name);
		Ok(self
			.get::<Vec<SgdbGame>>(&format!("/search/autocomplete/{term}"))
			.await?
			.and_then(|games| games.into_iter().next()))
	}

	pub async fn choices(&self, game: u64, kind: ArtKind) -> Result<Vec<Choice>, String> {
		let path = match kind {
			ArtKind::Grid => {
				format!("/grids/game/{game}?dimensions=600x900&types=static&nsfw=false&humor=false")
			}
			ArtKind::Hero => format!("/heroes/game/{game}?types=static&nsfw=false&humor=false"),
			ArtKind::Icon => format!("/icons/game/{game}?types=static&nsfw=false&humor=false"),
		};
		Ok(self.get::<Vec<Choice>>(&path).await?.unwrap_or_default())
	}
}

fn percent_encode(text: &str) -> String {
	text.bytes()
		.map(|b| match b {
			b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
				(b as char).to_string()
			}
			_ => format!("%{b:02X}"),
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn store_replaces_files() {
		let dir = tempfile::tempdir().unwrap();
		let store = ArtStore::new(dir.path().to_path_buf());
		assert_eq!(store.find("g", ArtKind::Grid), None);
		let first = store.save("g", ArtKind::Grid, b"a", "png").unwrap();
		std::thread::sleep(std::time::Duration::from_millis(2));
		let second = store.save("g", ArtKind::Grid, b"b", "jpg").unwrap();
		assert_ne!(first, second);
		assert!(!first.exists());
		assert_eq!(store.find("g", ArtKind::Grid), Some(second));
		assert_eq!(store.find("g", ArtKind::Hero), None);
		store.remove_all("g");
		assert!(!store.dir("g").exists());
	}

	#[test]
	fn sniffs_and_encodes() {
		assert_eq!(sniff_ext(b"\x89PNG\r\n"), Some("png"));
		assert_eq!(sniff_ext(b"\xFF\xD8\xFF\xE0"), Some("jpg"));
		assert_eq!(sniff_ext(b"<html>"), None);
		assert_eq!(percent_encode("Half-Life 2: Ep"), "Half-Life%202%3A%20Ep");
		assert_eq!(steam_urls(10, ArtKind::Hero).len(), 4);
	}

	#[test]
	fn icon_from_non_pe_is_none() {
		let dir = tempfile::tempdir().unwrap();
		let fake = dir.path().join("x.exe");
		std::fs::write(&fake, "not a pe").unwrap();
		assert_eq!(exe_icon(&fake), None);
	}
}
