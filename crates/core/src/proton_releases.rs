//! Proton releases from GitHub (GE-Proton, Proton-CachyOS), downloaded
//! into the launcher's folder.

use std::path::{Path, PathBuf};

use rust_i18n::t;
use serde::Deserialize;
use sha2::{Digest, Sha512};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
	GeProton,
	CachyOs,
}

impl Provider {
	pub const ALL: [Provider; 2] = [Provider::GeProton, Provider::CachyOs];

	pub fn label(self) -> &'static str {
		match self {
			Self::GeProton => "GE-Proton",
			Self::CachyOs => "Proton-CachyOS",
		}
	}

	fn api(self) -> &'static str {
		match self {
			Self::GeProton => {
				"https://api.github.com/repos/GloriousEggroll/proton-ge-custom/releases?per_page=30"
			}
			Self::CachyOs => {
				"https://api.github.com/repos/CachyOS/proton-cachyos/releases?per_page=30"
			}
		}
	}

	/// Asset name endings for this computer, best first.
	fn suffixes(self, arch: &str, v3: bool) -> Vec<String> {
		match (self, arch) {
			(Self::GeProton, "x86_64") => vec!["-x86_64.tar.gz".into(), ".tar.gz".into()],
			(Self::GeProton, arch) => vec![format!("-{arch}.tar.gz")],
			(Self::CachyOs, "x86_64") if v3 => {
				vec!["-x86_64_v3.tar.xz".into(), "-x86_64.tar.xz".into()]
			}
			(Self::CachyOs, "x86_64") => vec!["-x86_64.tar.xz".into()],
			(Self::CachyOs, "aarch64") => vec!["-arm64.tar.xz".into()],
			(Self::CachyOs, _) => Vec::new(),
		}
	}
}

/// Whether the CPU runs x86-64-v3 code (AVX2 and friends), which
/// Proton-CachyOS has faster builds for.
pub fn cpu_has_v3() -> bool {
	#[cfg(target_arch = "x86_64")]
	{
		std::arch::is_x86_feature_detected!("avx2")
			&& std::arch::is_x86_feature_detected!("bmi1")
			&& std::arch::is_x86_feature_detected!("bmi2")
			&& std::arch::is_x86_feature_detected!("fma")
			&& std::arch::is_x86_feature_detected!("f16c")
			&& std::arch::is_x86_feature_detected!("lzcnt")
			&& std::arch::is_x86_feature_detected!("movbe")
	}
	#[cfg(not(target_arch = "x86_64"))]
	{
		false
	}
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
	pub provider: Provider,
	/// Like `GE-Proton11-14` or `cachyos-11.0-20261015-slr`.
	pub tag: String,
	/// The folder it unpacks to: the archive's name without the extension.
	pub dir_name: String,
	/// `2026-11-23`.
	pub date: String,
	pub size: u64,
	pub url: String,
	pub checksum_url: Option<String>,
}

#[derive(Deserialize)]
struct GhRelease {
	tag_name: String,
	published_at: Option<String>,
	assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
	name: String,
	size: u64,
	browser_download_url: String,
}

/// Old GE-Proton releases have no arch in the name and are x86_64 only,
/// hence the bare `.tar.gz` last, after the arch-specific ones.
fn pick_asset<'a>(assets: &'a [GhAsset], suffixes: &[String]) -> Option<&'a GhAsset> {
	suffixes.iter().find_map(|suffix| {
		assets.iter().find(|a| {
			a.name.ends_with(suffix.as_str())
				&& (suffix.starts_with('-') || !a.name.contains("aarch64"))
		})
	})
}

fn strip_tarball(name: &str) -> &str {
	name.trim_end_matches(".tar.gz").trim_end_matches(".tar.xz")
}

fn parse_releases(
	text: &str,
	provider: Provider,
	arch: &str,
	v3: bool,
) -> Result<Vec<Release>, String> {
	let releases: Vec<GhRelease> = serde_json::from_str(text).map_err(|e| e.to_string())?;
	let suffixes = provider.suffixes(arch, v3);
	Ok(releases
		.into_iter()
		.filter_map(|r| {
			let asset = pick_asset(&r.assets, &suffixes)?;
			let dir_name = strip_tarball(&asset.name).to_owned();
			let checksum_name = format!("{dir_name}.sha512sum");
			Some(Release {
				provider,
				dir_name,
				date: r
					.published_at
					.as_deref()
					.and_then(|d| d.get(..10))
					.unwrap_or_default()
					.to_owned(),
				size: asset.size,
				url: asset.browser_download_url.clone(),
				checksum_url: r
					.assets
					.iter()
					.find(|a| a.name == checksum_name)
					.map(|a| a.browser_download_url.clone()),
				tag: r.tag_name,
			})
		})
		.collect())
}

pub async fn releases(provider: Provider) -> Result<Vec<Release>, String> {
	let client = crate::http::client()?;
	let text = client
		.get(provider.api())
		.header("Accept", "application/vnd.github+json")
		.send()
		.await
		.map_err(|e| e.to_string())?
		.error_for_status()
		.map_err(|e| e.to_string())?
		.text()
		.await
		.map_err(|e| e.to_string())?;
	parse_releases(&text, provider, std::env::consts::ARCH, cpu_has_v3())
}

/// Downloads, checks and unpacks a release; returns the new build's folder.
/// `progress` gets 0..=1 while downloading.
pub async fn install(
	release: &Release,
	dir: &Path,
	progress: impl Fn(f32),
) -> Result<PathBuf, String> {
	std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
	let xz = release.url.ends_with(".xz");
	let ext = if xz { "xz" } else { "gz" };
	let tarball = dir.join(format!(".{}.tar.{ext}", release.dir_name));
	let staging = dir.join(format!(".{}.unpacking", release.dir_name));
	let result = async {
		let digest = download(&release.url, &tarball, &progress).await?;
		if let Some(url) = &release.checksum_url {
			let expected = expected_hash(url).await?;
			if !expected.eq_ignore_ascii_case(&digest) {
				return Err(t!("err.proton_checksum").into_owned());
			}
		}
		let (tarball, staging, dir) = (tarball.clone(), staging.clone(), dir.to_path_buf());
		tokio::task::spawn_blocking(move || unpack(&tarball, xz, &staging, &dir))
			.await
			.map_err(|e| e.to_string())?
	}
	.await;
	let _ = std::fs::remove_file(&tarball);
	let _ = std::fs::remove_dir_all(&staging);
	result
}

/// Streams to `dest`; returns the SHA-512 in hex.
async fn download(url: &str, dest: &Path, progress: &impl Fn(f32)) -> Result<String, String> {
	let client = crate::http::client()?;
	let mut response = client
		.get(url)
		.send()
		.await
		.map_err(|e| e.to_string())?
		.error_for_status()
		.map_err(|e| e.to_string())?;
	let total = response.content_length();
	let mut file = tokio::fs::File::create(dest)
		.await
		.map_err(|e| e.to_string())?;
	let mut hasher = Sha512::new();
	let mut done = 0u64;
	progress(0.);
	while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
		hasher.update(&chunk);
		file.write_all(&chunk).await.map_err(|e| e.to_string())?;
		done += chunk.len() as u64;
		if let Some(total) = total.filter(|t| *t > 0) {
			progress((done as f32 / total as f32).min(1.));
		}
	}
	file.flush().await.map_err(|e| e.to_string())?;
	Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
	bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The `.sha512sum` file: `<hash>  <name>`.
async fn expected_hash(url: &str) -> Result<String, String> {
	let client = crate::http::client()?;
	let text = client
		.get(url)
		.send()
		.await
		.map_err(|e| e.to_string())?
		.error_for_status()
		.map_err(|e| e.to_string())?
		.text()
		.await
		.map_err(|e| e.to_string())?;
	text.split_whitespace()
		.next()
		.map(str::to_owned)
		.ok_or_else(|| t!("err.proton_checksum").into_owned())
}

/// Unpacks into a staging folder first, so a half-unpacked build never
/// looks installed, then moves its one top folder into place.
fn unpack(tarball: &Path, xz: bool, staging: &Path, dir: &Path) -> Result<PathBuf, String> {
	let _ = std::fs::remove_dir_all(staging);
	std::fs::create_dir_all(staging).map_err(|e| e.to_string())?;
	let file = std::fs::File::open(tarball).map_err(|e| e.to_string())?;
	let reader: Box<dyn std::io::Read> = if xz {
		Box::new(xz2::read::XzDecoder::new(file))
	} else {
		Box::new(flate2::read::GzDecoder::new(file))
	};
	let mut archive = tar::Archive::new(reader);
	archive.set_preserve_permissions(true);
	archive.unpack(staging).map_err(|e| e.to_string())?;
	let mut tops = std::fs::read_dir(staging)
		.map_err(|e| e.to_string())?
		.flatten()
		.map(|e| e.path());
	let top = match (tops.next(), tops.next()) {
		(Some(top), None) if top.is_dir() => top,
		_ => return Err(t!("err.proton_layout").into_owned()),
	};
	let name = top
		.file_name()
		.ok_or_else(|| t!("err.proton_layout").into_owned())?;
	let target = dir.join(name);
	if target.exists() {
		std::fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
	}
	std::fs::rename(&top, &target).map_err(|e| e.to_string())?;
	Ok(target)
}

#[cfg(test)]
mod tests {
	use super::*;

	const JSON: &str = r#"[
		{"tag_name": "GE-Proton11-14", "published_at": "2026-11-23T10:19:36Z", "assets": [
			{"name": "GE-Proton11-14-aarch64.tar.gz", "size": 2, "browser_download_url": "https://x/a.tar.gz"},
			{"name": "GE-Proton11-14-x86_64.sha512sum", "size": 1, "browser_download_url": "https://x/x.sha512sum"},
			{"name": "GE-Proton11-14-x86_64.tar.gz", "size": 3, "browser_download_url": "https://x/x.tar.gz"}
		]},
		{"tag_name": "GE-Proton9-20", "published_at": "2024-11-01T00:00:00Z", "assets": [
			{"name": "GE-Proton9-20.tar.gz", "size": 5, "browser_download_url": "https://x/9.tar.gz"}
		]}
	]"#;

	const CACHY: &str = r#"[
		{"tag_name": "cachyos-11.0-20261015-slr", "published_at": "2026-10-20T00:00:00Z", "assets": [
			{"name": "proton-cachyos-11.0-20261015-slr-arm64.tar.xz", "size": 1, "browser_download_url": "https://c/arm"},
			{"name": "proton-cachyos-11.0-20261015-slr-x86_64.sha512sum", "size": 1, "browser_download_url": "https://c/x.sum"},
			{"name": "proton-cachyos-11.0-20261015-slr-x86_64.tar.xz", "size": 2, "browser_download_url": "https://c/x.tar.xz"},
			{"name": "proton-cachyos-11.0-20261015-slr-x86_64_v3.sha512sum", "size": 1, "browser_download_url": "https://c/v3.sum"},
			{"name": "proton-cachyos-11.0-20261015-slr-x86_64_v3.tar.xz", "size": 3, "browser_download_url": "https://c/v3.tar.xz"}
		]}
	]"#;

	#[test]
	fn picks_cachyos_variant() {
		let v3 = parse_releases(CACHY, Provider::CachyOs, "x86_64", true).unwrap();
		assert_eq!(v3[0].url, "https://c/v3.tar.xz");
		assert_eq!(v3[0].checksum_url.as_deref(), Some("https://c/v3.sum"));
		assert_eq!(v3[0].dir_name, "proton-cachyos-11.0-20261015-slr-x86_64_v3");
		let plain = parse_releases(CACHY, Provider::CachyOs, "x86_64", false).unwrap();
		assert_eq!(plain[0].url, "https://c/x.tar.xz");
		assert_eq!(plain[0].dir_name, "proton-cachyos-11.0-20261015-slr-x86_64");
		let arm = parse_releases(CACHY, Provider::CachyOs, "aarch64", false).unwrap();
		assert_eq!(arm[0].url, "https://c/arm");
	}

	#[test]
	fn picks_assets_for_the_arch() {
		let x86 = parse_releases(JSON, Provider::GeProton, "x86_64", true).unwrap();
		assert_eq!(x86.len(), 2);
		assert_eq!(x86[0].url, "https://x/x.tar.gz");
		assert_eq!(
			x86[0].checksum_url.as_deref(),
			Some("https://x/x.sha512sum")
		);
		assert_eq!(x86[0].date, "2026-11-23");
		assert_eq!(x86[1].url, "https://x/9.tar.gz");
		assert_eq!(x86[1].checksum_url, None);
		assert_eq!(x86[0].dir_name, "GE-Proton11-14-x86_64");
		assert_eq!(x86[1].dir_name, "GE-Proton9-20");
		let arm = parse_releases(JSON, Provider::GeProton, "aarch64", false).unwrap();
		assert_eq!(arm.len(), 1);
		assert_eq!(arm[0].size, 2);
	}

	#[test]
	fn unpacks_one_top_folder() {
		let dir = tempfile::tempdir().unwrap();
		let tarball = dir.path().join("b.tar.gz");
		{
			let file = std::fs::File::create(&tarball).unwrap();
			let gz = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
			let mut builder = tar::Builder::new(gz);
			let data = b"#!/bin/sh\n";
			let mut header = tar::Header::new_gnu();
			header.set_size(data.len() as u64);
			header.set_mode(0o755);
			header.set_cksum();
			builder
				.append_data(&mut header, "GE-Proton1-1/proton", &data[..])
				.unwrap();
			builder.into_inner().unwrap().finish().unwrap();
		}
		let protons = dir.path().join("protons");
		std::fs::create_dir_all(&protons).unwrap();
		let staging = protons.join(".staging");
		let built = unpack(&tarball, false, &staging, &protons).unwrap();
		assert_eq!(built, protons.join("GE-Proton1-1"));
		assert!(built.join("proton").is_file());
		assert_eq!(hex(&[0, 255]), "00ff");

		let xz_ball = dir.path().join("c.tar.xz");
		{
			let file = std::fs::File::create(&xz_ball).unwrap();
			let mut builder = tar::Builder::new(xz2::write::XzEncoder::new(file, 1));
			let mut header = tar::Header::new_gnu();
			header.set_size(1);
			header.set_mode(0o755);
			header.set_cksum();
			builder
				.append_data(&mut header, "proton-cachyos-1/proton", &b"x"[..])
				.unwrap();
			builder.into_inner().unwrap().finish().unwrap();
		}
		let built = unpack(&xz_ball, true, &staging, &protons).unwrap();
		assert!(built.join("proton").is_file());
	}
}
