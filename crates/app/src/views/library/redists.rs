use onlinefix_launcher_core::redist::{Kind, Redist};

use super::*;

impl LibraryView {
	pub fn set_redists(&mut self, id: String, found: Vec<Redist>, cx: &mut Context<Self>) {
		self.redists.insert(id, found);
		cx.notify();
	}

	pub fn set_redist_step(
		&mut self,
		id: String,
		step: Option<(usize, usize)>,
		cx: &mut Context<Self>,
	) {
		match step {
			Some(step) => {
				self.redist_step.insert(id, step);
			}
			None => {
				self.redist_step.remove(&id);
				self.redist_pick.clear();
			}
		}
		cx.notify();
	}

	fn redist_ticked(&self, game: &Game, redist: &Redist) -> bool {
		self.redist_pick
			.get(&redist.path)
			.copied()
			.unwrap_or_else(|| !game.redists_done.contains(&redist.key))
	}

	/// Only when the game came with some.
	pub(super) fn redists_section(
		&self,
		game: &Game,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let found = self.redists.get(&game.id).filter(|f| !f.is_empty())?;
		let theme = cx.theme().clone();
		let running = self.runs.contains_key(&game.id);
		let step = self.redist_step.get(&game.id).copied();
		let missing = found
			.iter()
			.filter(|r| !game.redists_done.contains(&r.key))
			.count();
		let picked: Vec<PathBuf> = found
			.iter()
			.filter(|r| self.redist_ticked(game, r))
			.map(|r| r.path.clone())
			.collect();
		let rows = found.iter().enumerate().map(|(ix, redist)| {
			let done = game.redists_done.contains(&redist.key);
			let path = redist.path.clone();
			h_flex()
				.gap_2()
				.child(
					Checkbox::new(("redist", ix))
						.label(redist_label(redist))
						.checked(self.redist_ticked(game, redist))
						.disabled(step.is_some())
						.tooltip(redist.path.display().to_string())
						.on_click(cx.listener(move |this, checked: &bool, _, cx| {
							this.redist_pick.insert(path.clone(), *checked);
							cx.notify();
						})),
				)
				.child(div().flex_1())
				.when(done, |row| {
					row.child(
						div()
							.text_sm()
							.text_color(theme.success)
							.child(t!("redist.done").into_owned()),
					)
				})
		});
		let id = game.id.clone();
		let count = picked.len();
		let button = Button::new("install-redists")
			.small()
			.outline()
			.disabled(running || step.is_some() || count == 0)
			.when_some(step, |b, (done, total)| {
				b.loading(true)
					.label(t!("redist.installing", n = done + 1, total = total))
			})
			.when(step.is_none(), |b| {
				b.label(t!("redist.install", n = count))
					.icon(Icon::new(IconName::ArrowDown))
			})
			.when(running, |b| b.tooltip(t!("redist.close_game")))
			.on_click(cx.listener(move |this, _, _, _| {
				this.engine.send(Command::InstallRedists {
					id: id.clone(),
					paths: picked.clone(),
				});
			}));
		Some(
			v_flex()
				.gap_2()
				.pt_2()
				.border_t_1()
				.border_color(theme.border)
				.child(div().pt_2().child(t!("redist.title").into_owned()))
				.child(
					div()
						.text_sm()
						.text_color(if missing > 0 {
							theme.warning
						} else {
							theme.muted_foreground
						})
						.child(
							if missing > 0 {
								t!("redist.hint")
							} else {
								t!("redist.all_done")
							}
							.into_owned(),
						),
				)
				.child(v_flex().gap_1().children(rows))
				.child(h_flex().child(button))
				.into_any_element(),
		)
	}
}

fn redist_label(redist: &Redist) -> String {
	let name = match redist.kind {
		Kind::VisualCpp => "Visual C++".to_owned(),
		Kind::DirectX => "DirectX".to_owned(),
		Kind::DotNet => ".NET Framework".to_owned(),
		Kind::Xna => "XNA Framework".to_owned(),
		Kind::PhysX => "PhysX".to_owned(),
		Kind::UnrealPrereqs => t!("redist.unreal").into_owned(),
		Kind::OpenAl => "OpenAL".to_owned(),
		Kind::Other => redist
			.path
			.file_name()
			.map(|n| n.to_string_lossy().into_owned())
			.unwrap_or_default(),
	};
	if redist.detail.is_empty() {
		name
	} else {
		format!("{name} {}", redist.detail)
	}
}
