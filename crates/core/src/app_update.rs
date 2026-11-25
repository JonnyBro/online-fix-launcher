//! Whether a newer release of the app is out on GitHub, and replacing the
//! running binary with it.

use std::io::Read;
use std::path::{Path, PathBuf};

use rust_i18n::t;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const LATEST_RELEASE_API: &str =
	"https://api.github.com/repos/JonnyBro/online-fix-launcher/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRelease {
	/// Without the leading `v`, like `2.1.0`.
	pub version: String,
	pub url: String,
	/// The Linux tarball and the release's `SHA256SUMS`, when both are there.
	pub download: Option<Download>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
	pub name: String,
	pub url: String,
	pub sums_url: String,
}

#[derive(Deserialize)]
struct LatestRelease {
	tag_name: String,
	html_url: String,
	#[serde(default)]
	assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
	name: String,
	browser_download_url: String,
}

/// Release workflow names it `onlinefix-launcher-<tag>-linux-x86_64.tar.gz`.
fn pick_download(assets: &[Asset], arch: &str) -> Option<Download> {
	let tarball = assets
		.iter()
		.find(|a| a.name.ends_with(&format!("-linux-{arch}.tar.gz")))?;
	let sums = assets.iter().find(|a| a.name == "SHA256SUMS")?;
	Some(Download {
		name: tarball.name.clone(),
		url: tarball.browser_download_url.clone(),
		sums_url: sums.browser_download_url.clone(),
	})
}

/// The latest release if it's newer than this build, `None` if it isn't or
/// the repository has no releases yet.
pub async fn check() -> Result<Option<AppRelease>, String> {
	let client = crate::http::client()?;
	let response = client
		.get(LATEST_RELEASE_API)
		.header("Accept", "application/vnd.github+json")
		.send()
		.await
		.map_err(|e| e.to_string())?;
	if response.status() == reqwest::StatusCode::NOT_FOUND {
		return Ok(None);
	}
	let text = response
		.error_for_status()
		.map_err(|e| e.to_string())?
		.text()
		.await
		.map_err(|e| e.to_string())?;
	let latest: LatestRelease = serde_json::from_str(&text).map_err(|e| e.to_string())?;
	Ok(
		is_newer(&latest.tag_name, env!("CARGO_PKG_VERSION")).then(|| AppRelease {
			version: latest.tag_name.trim_start_matches('v').to_owned(),
			download: pick_download(&latest.assets, std::env::consts::ARCH),
			url: latest.html_url,
		}),
	)
}

/// The launcher's binary on disk. Once an update renamed a new file over
/// it, Linux reports the running one as `<path> (deleted)`; the new binary
/// is at `<path>`.
pub fn current_exe() -> std::io::Result<PathBuf> {
	let exe = std::env::current_exe()?;
	strip_deleted(&exe).canonicalize()
}

fn strip_deleted(exe: &Path) -> PathBuf {
	match exe.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
		Some(path) => PathBuf::from(path),
		None => exe.to_path_buf(),
	}
}

/// The running binary, when the launcher may replace it: a release build
/// in a folder this user can write to (not a package in `/usr/bin`).
pub fn replaceable_binary() -> Option<PathBuf> {
	if cfg!(debug_assertions) {
		return None;
	}
	let exe = current_exe().ok()?;
	let dir = exe.parent()?;
	let probe = dir.join(".onlinefix-launcher-write-probe");
	std::fs::write(&probe, b"").ok()?;
	let _ = std::fs::remove_file(&probe);
	Some(exe)
}

/// Downloads the release, checks it against `SHA256SUMS` and puts its
/// binary in place of `exe`. The running copy keeps working until restarted.
pub async fn install(
	download: &Download,
	exe: &Path,
	progress: impl Fn(f32),
) -> Result<(), String> {
	let client = crate::http::client()?;
	let sums = client
		.get(&download.sums_url)
		.send()
		.await
		.map_err(|e| e.to_string())?
		.error_for_status()
		.map_err(|e| e.to_string())?
		.text()
		.await
		.map_err(|e| e.to_string())?;
	let expected =
		expected_sum(&sums, &download.name).ok_or_else(|| t!("err.update_no_sum").into_owned())?;

	let mut response = client
		.get(&download.url)
		.send()
		.await
		.map_err(|e| e.to_string())?
		.error_for_status()
		.map_err(|e| e.to_string())?;
	let total = response.content_length().filter(|t| *t > 0);
	let mut bytes = Vec::new();
	progress(0.);
	while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
		bytes.extend_from_slice(&chunk);
		if let Some(total) = total {
			progress((bytes.len() as f32 / total as f32).min(1.));
		}
	}
	let digest: String = Sha256::digest(&bytes)
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect();
	if !digest.eq_ignore_ascii_case(&expected) {
		return Err(t!("err.update_checksum").into_owned());
	}
	let exe = exe.to_path_buf();
	tokio::task::spawn_blocking(move || replace_binary(&bytes, &exe))
		.await
		.map_err(|e| e.to_string())?
}

fn expected_sum(sums: &str, name: &str) -> Option<String> {
	sums.lines().find_map(|line| {
		let mut parts = line.split_whitespace();
		let hash = parts.next()?;
		let file = parts.next()?.trim_start_matches('*');
		(file == name).then(|| hash.to_owned())
	})
}

/// Writes the tarball's `onlinefix-launcher` next to `exe`, then renames it
/// over `exe`, so a failed update never leaves half a binary behind.
fn replace_binary(tarball: &[u8], exe: &Path) -> Result<(), String> {
	use std::os::unix::fs::PermissionsExt;
	let name = exe
		.file_name()
		.ok_or_else(|| t!("err.update_no_binary").into_owned())?;
	let staged = exe.with_file_name(".onlinefix-launcher.update");
	let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tarball));
	let mut found = false;
	for entry in archive.entries().map_err(|e| e.to_string())? {
		let mut entry = entry.map_err(|e| e.to_string())?;
		let path = entry.path().map_err(|e| e.to_string())?.into_owned();
		if path.file_name() != Some(std::ffi::OsStr::new("onlinefix-launcher")) {
			continue;
		}
		let mut data = Vec::new();
		entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
		std::fs::write(&staged, data).map_err(|e| e.to_string())?;
		found = true;
		break;
	}
	if !found {
		return Err(t!("err.update_no_binary").into_owned());
	}
	let result = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
		.and_then(|()| std::fs::rename(&staged, exe.with_file_name(name)));
	if result.is_err() {
		let _ = std::fs::remove_file(&staged);
	}
	result.map_err(|e| e.to_string())
}

/// `v2.1.0`, `2.1` or `2.1.0-beta` as numbers; anything after `-` is ignored.
fn parse_version(text: &str) -> Option<[u64; 3]> {
	let core = text
		.trim()
		.trim_start_matches('v')
		.split(['-', '+'])
		.next()?;
	let mut parts = [0u64; 3];
	for (slot, part) in parts.iter_mut().zip(core.split('.')) {
		*slot = part.parse().ok()?;
	}
	Some(parts)
}

fn is_newer(tag: &str, current: &str) -> bool {
	match (parse_version(tag), parse_version(current)) {
		(Some(tag), Some(current)) => tag > current,
		_ => false,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn picks_the_tarball_and_its_sum() {
		let assets = vec![
			Asset {
				name: "SHA256SUMS".into(),
				browser_download_url: "https://x/SHA256SUMS".into(),
			},
			Asset {
				name: "onlinefix-launcher-v0.2.0-linux-x86_64.tar.gz".into(),
				browser_download_url: "https://x/t.tar.gz".into(),
			},
		];
		let download = pick_download(&assets, "x86_64").unwrap();
		assert_eq!(download.url, "https://x/t.tar.gz");
		assert!(pick_download(&assets, "aarch64").is_none());
		let sums = "abc  other.tar.gz\nDEF  onlinefix-launcher-v0.2.0-linux-x86_64.tar.gz\n";
		assert_eq!(expected_sum(sums, &download.name).as_deref(), Some("DEF"));
	}

	#[test]
	fn replaces_the_binary_from_a_tarball() {
		let dir = tempfile::tempdir().unwrap();
		let exe = dir.path().join("onlinefix-launcher");
		std::fs::write(&exe, "old").unwrap();
		let mut tarball = Vec::new();
		{
			let gz = flate2::write::GzEncoder::new(&mut tarball, flate2::Compression::fast());
			let mut builder = tar::Builder::new(gz);
			for (path, data) in [
				(
					"onlinefix-launcher-v0.2.0-linux-x86_64/README.md",
					&b"readme"[..],
				),
				(
					"onlinefix-launcher-v0.2.0-linux-x86_64/onlinefix-launcher",
					&b"new"[..],
				),
			] {
				let mut header = tar::Header::new_gnu();
				header.set_size(data.len() as u64);
				header.set_mode(0o644);
				header.set_cksum();
				builder.append_data(&mut header, path, data).unwrap();
			}
			builder.into_inner().unwrap().finish().unwrap();
		}
		replace_binary(&tarball, &exe).unwrap();
		assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
		use std::os::unix::fs::PermissionsExt;
		assert_eq!(
			std::fs::metadata(&exe).unwrap().permissions().mode() & 0o777,
			0o755
		);
		assert!(!dir.path().join(".onlinefix-launcher.update").exists());
	}

	#[test]
	fn replaced_binary_path() {
		assert_eq!(
			strip_deleted(Path::new("/opt/ofl/onlinefix-launcher (deleted)")),
			Path::new("/opt/ofl/onlinefix-launcher")
		);
		assert_eq!(
			strip_deleted(Path::new("/opt/ofl/onlinefix-launcher")),
			Path::new("/opt/ofl/onlinefix-launcher")
		);
	}

	#[test]
	fn compares_versions() {
		assert!(is_newer("v2.1.0", "2.0.0"));
		assert!(is_newer("v2.0.1", "2.0.0"));
		assert!(is_newer("3", "2.9.9"));
		assert!(!is_newer("v2.0.0", "2.0.0"));
		assert!(!is_newer("v1.9.0", "2.0.0"));
		assert!(!is_newer("v2.0.0-beta", "2.0.0"));
		assert!(!is_newer("nightly", "2.0.0"));
		assert_eq!(parse_version("v2.1"), Some([2, 1, 0]));
	}
}
