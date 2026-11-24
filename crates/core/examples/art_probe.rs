//! cargo run -p onlinefix-launcher-core --example art_probe -- <Game.exe> [steam app id]

use onlinefix_launcher_core::art::{self, ArtKind};

#[tokio::main]
async fn main() {
	let args: Vec<String> = std::env::args().skip(1).collect();
	let exe = std::path::Path::new(&args[0]);
	match art::exe_icon(exe) {
		Some(png) => println!("icon: {} bytes of PNG", png.len()),
		None => println!("icon: none"),
	}
	if let Some(id) = args.get(1).and_then(|a| a.parse().ok()) {
		for kind in [ArtKind::Grid, ArtKind::Hero] {
			let found = art::first_picture(&art::steam_urls(id, kind)).await;
			println!("{kind:?}: {:?}", found.map(|(b, ext)| (b.len(), ext)));
		}
	}
}
