//! Lists installed Proton builds and the releases each source offers.
//!
//! cargo run -p onlinefix-launcher-core --example proton_list

use onlinefix_launcher_core::proton_releases::{self, Provider};
use onlinefix_launcher_core::{AppPaths, proton, steam};

#[tokio::main]
async fn main() {
	let paths = AppPaths::detect().unwrap();
	let root = steam::root();
	let libraries = root
		.as_deref()
		.map(steam::library_folders)
		.unwrap_or_default();
	for build in proton::find_installed(&paths.protons_dir(), root.as_deref(), &libraries) {
		let runtime = build
			.runtime_app_id
			.map(|id| (id, steam::runtime(&libraries, id).is_some()));
		println!("{} ({:?}), runtime {runtime:?}", build.name, build.source);
	}
	println!("x86-64-v3: {}", proton_releases::cpu_has_v3());
	for provider in Provider::ALL {
		match proton_releases::releases(provider).await {
			Ok(releases) => {
				for r in releases.iter().take(3) {
					println!(
						"{}: {} {} {} MB, {}, checksum: {}",
						provider.label(),
						r.tag,
						r.date,
						r.size / 1_000_000,
						r.dir_name,
						r.checksum_url.is_some()
					);
				}
			}
			Err(e) => println!("{}: {e}", provider.label()),
		}
	}
}
