//! The INI files fixes ship (`OnlineFix.ini`, `SteamFix.ini`). Edits keep
//! every other line as it was, comments and odd formatting included.

#[derive(Debug, Clone, PartialEq)]
pub struct Ini {
	lines: Vec<String>,
}

impl Ini {
	pub fn parse(text: &str) -> Self {
		Self {
			lines: text.lines().map(str::to_owned).collect(),
		}
	}

	pub fn get(&self, section: &str, key: &str) -> Option<&str> {
		let range = self.section_range(section)?;
		self.lines[range].iter().find_map(|line| {
			let (k, v) = split_entry(line)?;
			k.eq_ignore_ascii_case(key).then_some(v)
		})
	}

	pub fn has_section(&self, section: &str) -> bool {
		self.section_range(section).is_some()
	}

	/// Replaces the value in place, or adds the key (and the section) at the end.
	pub fn set(&mut self, section: &str, key: &str, value: &str) {
		let entry = format!("{key}={value}");
		match self.section_range(section) {
			Some(range) => {
				let found = range.clone().find(|&i| {
					split_entry(&self.lines[i]).is_some_and(|(k, _)| k.eq_ignore_ascii_case(key))
				});
				match found {
					Some(i) => self.lines[i] = entry,
					None => {
						// After the section's last entry, not after trailing blank lines.
						let mut at = range.end;
						while at > range.start && self.lines[at - 1].trim().is_empty() {
							at -= 1;
						}
						self.lines.insert(at, entry);
					}
				}
			}
			None => {
				if self.lines.last().is_some_and(|l| !l.trim().is_empty()) {
					self.lines.push(String::new());
				}
				self.lines.push(format!("[{section}]"));
				self.lines.push(entry);
			}
		}
	}

	pub fn to_text(&self) -> String {
		let mut text = self.lines.join("\n");
		text.push('\n');
		text
	}

	/// Lines of the section's body: after its header, up to the next header.
	fn section_range(&self, section: &str) -> Option<std::ops::Range<usize>> {
		let start = self.lines.iter().position(|line| {
			section_name(line).is_some_and(|name| name.eq_ignore_ascii_case(section))
		})? + 1;
		let end = self.lines[start..]
			.iter()
			.position(|line| section_name(line).is_some())
			.map_or(self.lines.len(), |n| start + n);
		Some(start..end)
	}
}

fn section_name(line: &str) -> Option<&str> {
	let line = line.trim();
	line.strip_prefix('[')?.strip_suffix(']').map(str::trim)
}

fn split_entry(line: &str) -> Option<(&str, &str)> {
	let line = line.trim();
	if line.starts_with([';', '#']) {
		return None;
	}
	let (k, v) = line.split_once('=')?;
	Some((k.trim(), v.trim()))
}

#[cfg(test)]
mod tests {
	use super::*;

	const TEXT: &str =
		"; fix\n[Main]\nRealAppId = 480\nFakeAppId=1234\n\n[Misc]\nExtraProtection=true\n";

	#[test]
	fn reads_case_insensitively() {
		let ini = Ini::parse(TEXT);
		assert_eq!(ini.get("main", "realappid"), Some("480"));
		assert_eq!(ini.get("Main", "FakeAppId"), Some("1234"));
		assert_eq!(ini.get("Misc", "RealAppId"), None);
		assert!(!ini.has_section("OnlineFix Linux"));
	}

	#[test]
	fn edits_keep_the_rest() {
		let mut ini = Ini::parse(TEXT);
		ini.set("Misc", "ExtraProtection", "false");
		ini.set("Main", "Extra", "1");
		ini.set("OnlineFix Linux", "RealAppId", "480");
		assert_eq!(
			ini.to_text(),
			"; fix\n[Main]\nRealAppId = 480\nFakeAppId=1234\nExtra=1\n\n[Misc]\nExtraProtection=false\n\n[OnlineFix Linux]\nRealAppId=480\n"
		);
	}
}
