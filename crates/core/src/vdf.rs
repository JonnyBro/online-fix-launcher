//! Valve's KeyValues text format (`.vdf`, `.acf`): just enough to read
//! library folders, app manifests and Proton tool manifests.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
	Str(String),
	Obj(Obj),
}

/// Keys compare case-insensitively in Valve's files, so they are stored lowercased.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Obj(BTreeMap<String, Value>);

impl Obj {
	pub fn get(&self, key: &str) -> Option<&Value> {
		self.0.get(&key.to_ascii_lowercase())
	}

	pub fn str(&self, key: &str) -> Option<&str> {
		match self.get(key)? {
			Value::Str(s) => Some(s),
			Value::Obj(_) => None,
		}
	}

	pub fn obj(&self, key: &str) -> Option<&Obj> {
		match self.get(key)? {
			Value::Obj(o) => Some(o),
			Value::Str(_) => None,
		}
	}

	pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
		self.0.iter().map(|(k, v)| (k.as_str(), v))
	}
}

/// The whole file as one object; it usually holds a single root key.
pub fn parse(text: &str) -> Option<Obj> {
	let mut tokens = Tokens {
		chars: text.chars().peekable(),
	};
	let obj = parse_obj(&mut tokens, false)?;
	Some(obj)
}

enum Token {
	Str(String),
	Open,
	Close,
}

struct Tokens<'a> {
	chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl Iterator for Tokens<'_> {
	type Item = Token;

	fn next(&mut self) -> Option<Token> {
		loop {
			let c = self.chars.next()?;
			match c {
				c if c.is_whitespace() => {}
				'/' if self.chars.peek() == Some(&'/') => {
					for c in self.chars.by_ref() {
						if c == '\n' {
							break;
						}
					}
				}
				// Platform conditionals such as `[$WIN32]` follow a value; they're ignored.
				'[' => {
					for c in self.chars.by_ref() {
						if c == ']' {
							break;
						}
					}
				}
				'{' => return Some(Token::Open),
				'}' => return Some(Token::Close),
				'"' => {
					let mut s = String::new();
					while let Some(c) = self.chars.next() {
						match c {
							'"' => break,
							'\\' => match self.chars.next() {
								Some('n') => s.push('\n'),
								Some('t') => s.push('\t'),
								Some(other) => s.push(other),
								None => break,
							},
							c => s.push(c),
						}
					}
					return Some(Token::Str(s));
				}
				c => {
					let mut s = String::from(c);
					while let Some(&c) = self.chars.peek() {
						if c.is_whitespace() || c == '{' || c == '}' || c == '"' {
							break;
						}
						s.push(c);
						self.chars.next();
					}
					return Some(Token::Str(s));
				}
			}
		}
	}
}

fn parse_obj(tokens: &mut Tokens, nested: bool) -> Option<Obj> {
	let mut obj = Obj::default();
	loop {
		let key = match tokens.next() {
			Some(Token::Str(key)) => key,
			Some(Token::Close) if nested => return Some(obj),
			None if !nested => return Some(obj),
			_ => return None,
		};
		let value = match tokens.next()? {
			Token::Str(s) => Value::Str(s),
			Token::Open => Value::Obj(parse_obj(tokens, true)?),
			Token::Close => return None,
		};
		obj.0.insert(key.to_ascii_lowercase(), value);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn reads_library_folders() {
		let text = r#""libraryfolders"
{
	"0"
	{
		"path"		"/home/u/.local/share/Steam"
		"apps"
		{
			"1628350"		"123"
		}
	}
	"1" { "path" "/mnt/Lib\\rary" }
}"#;
		let root = parse(text).unwrap();
		let folders = root.obj("LibraryFolders").unwrap();
		let first = folders.obj("0").unwrap();
		assert_eq!(first.str("path"), Some("/home/u/.local/share/Steam"));
		assert!(first.obj("apps").unwrap().get("1628350").is_some());
		assert_eq!(
			folders.obj("1").unwrap().str("path"),
			Some("/mnt/Lib\\rary")
		);
	}

	#[test]
	fn skips_comments_and_conditionals() {
		let text = "\"compat_tools\"\n{\n  \"GE\" // name\n  {\n    \"install_path\" \".\" [$LINUX]\n  }\n}\n";
		let root = parse(text).unwrap();
		let ge = root.obj("compat_tools").unwrap().obj("GE").unwrap();
		assert_eq!(ge.str("install_path"), Some("."));
	}

	#[test]
	fn rejects_unbalanced() {
		assert!(parse("\"a\" { \"b\" \"c\"").is_none());
	}
}
