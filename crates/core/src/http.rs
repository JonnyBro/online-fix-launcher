use std::time::Duration;

pub(crate) fn client() -> Result<reqwest::Client, String> {
	reqwest::Client::builder()
		.user_agent(concat!("onlinefix-launcher/", env!("CARGO_PKG_VERSION")))
		.connect_timeout(Duration::from_secs(20))
		.read_timeout(Duration::from_secs(60))
		.build()
		.map_err(|e| e.to_string())
}
