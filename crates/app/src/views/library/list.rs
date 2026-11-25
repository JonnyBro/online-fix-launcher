use super::grid::run_label;
use super::*;

const ROW_HEIGHT: f32 = 52.;
const THUMB: (f32, f32) = (80., 45.);
const PLAY_W: f32 = 110.;
const DATE_W: f32 = 140.;
const NAME_MIN: f32 = 180.;

impl LibraryView {
	pub(super) fn list(&mut self, cx: &mut Context<Self>) -> AnyElement {
		let theme = cx.theme().clone();
		let games: Vec<Game> = self.visible(cx).into_iter().cloned().collect();
		if games.is_empty() && self.unpacking.is_empty() {
			return self.nothing_shown(cx).into_any_element();
		}
		let LibraryPrefs {
			sort: current,
			descending,
			..
		} = self.prefs;
		let column = |sort: Sort, label: String, width: Option<f32>| {
			let arrow = (current == sort).then(|| {
				Icon::new(if descending {
					IconName::ChevronDown
				} else {
					IconName::ChevronUp
				})
				.xsmall()
			});
			h_flex()
				.id(SharedString::from(format!("col-{sort:?}")))
				.gap_1()
				.whitespace_nowrap()
				.when_some(width, |d, w| d.w(px(w)).flex_shrink_0().justify_end())
				.when(width.is_none(), |d| d.flex_1().min_w(px(NAME_MIN)))
				.when(current == sort, |d| d.text_color(theme.foreground))
				.cursor_pointer()
				.hover(|s| s.text_color(theme.foreground))
				.on_click(cx.listener(move |this, _, _, cx| this.set_sort(sort, cx)))
				.child(div().min_w_0().truncate().child(label))
				.children(arrow)
		};
		let header = h_flex()
			.w_full()
			.h(px(30.))
			.flex_shrink_0()
			.px_5()
			.gap_4()
			.text_sm()
			.text_color(theme.muted_foreground)
			.border_b_1()
			.border_color(theme.border)
			.child(div().w(px(THUMB.0)).flex_shrink_0())
			.child(column(Sort::Name, t!("lib.col_game").into(), None))
			.child(column(
				Sort::PlayTime,
				t!("lib.sort_play_time").into(),
				Some(PLAY_W),
			))
			.child(column(
				Sort::LastPlayed,
				t!("lib.sort_last_played").into(),
				Some(DATE_W),
			))
			.child(column(
				Sort::Added,
				t!("lib.sort_added").into(),
				Some(DATE_W),
			));

		let row = |id: ElementId| {
			h_flex()
				.id(id)
				.w_full()
				.h(px(ROW_HEIGHT))
				.flex_shrink_0()
				.px_5()
				.gap_4()
				.border_b_1()
				.border_color(theme.border.opacity(0.6))
		};
		let numbers = |text: String, width: f32| {
			div()
				.w(px(width))
				.flex_shrink_0()
				.text_right()
				.text_sm()
				.mono(cx)
				.child(text)
		};
		let pending: Vec<AnyElement> = self
			.unpacking
			.iter()
			.enumerate()
			.map(|(ix, u)| {
				let (label, progress) = match u.stage {
					PendingStage::Unpacking(p) => (t!("lib.unpacking"), Some(p)),
					PendingStage::Installing => (t!("lib.installing"), None),
				};
				row(("unpacking-row", ix).into())
					.child(
						div()
							.w(px(THUMB.0))
							.h(px(THUMB.1))
							.flex_shrink_0()
							.rounded(theme.radius)
							.border_1()
							.border_dashed()
							.border_color(theme.border),
					)
					.child(
						v_flex()
							.flex_1()
							.min_w(px(NAME_MIN))
							.child(div().truncate().child(u.name.clone()))
							.child(
								div()
									.text_sm()
									.text_color(theme.muted_foreground)
									.child(label.into_owned()),
							),
					)
					.children(progress.map(|p| {
						h_flex()
							.gap_2()
							.w(px(PLAY_W + DATE_W))
							.flex_shrink_0()
							.child(Progress::new(("unpack-row-progress", ix)).value(p * 100.))
							.child(div().text_sm().mono(cx).child(format!("{:.0}%", p * 100.)))
					}))
					.into_any_element()
			})
			.collect();
		let rows = games.into_iter().map(|game| {
			let selected = self.selected.as_deref() == Some(&game.id);
			let run = self.runs.get(&game.id).copied();
			let id = game.id.clone();
			let play_id = game.id.clone();
			let art = self.art.get(&game.id).cloned().unwrap_or_default();
			let picture = art.hero.as_ref().or(art.grid.as_ref());
			row(SharedString::from(format!("row-{}", game.id)).into())
				.when(game.hidden, |r| r.opacity(0.5))
				.when(selected, |r| r.bg(theme.list_active))
				.hover(|s| s.bg(theme.list_hover))
				.cursor_pointer()
				.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
					if event.click_count() >= 2 {
						this.launch(&play_id);
					} else {
						let again = this.selected.as_deref() == Some(id.as_str());
						this.select((!again).then(|| id.clone()), window, cx);
					}
				}))
				.child(cover(&game, picture, art.icon.as_ref(), THUMB, &theme))
				.child(
					v_flex()
						.flex_1()
						.min_w(px(NAME_MIN))
						.child(
							h_flex()
								.gap_1p5()
								.min_w_0()
								.children(game.favorite.then(|| {
									Icon::new(IconName::StarFill)
										.xsmall()
										.text_color(theme.warning)
								}))
								.children(game.hidden.then(|| {
									Icon::new(IconName::EyeOff)
										.xsmall()
										.text_color(theme.muted_foreground)
								}))
								.child(div().min_w_0().truncate().child(game.name.clone())),
						)
						.children(run.map(|state| {
							div()
								.text_sm()
								.text_color(theme.success)
								.child(run_label(state))
						})),
				)
				.child(numbers(
					if game.play_seconds > 0 {
						format_play_time(game.play_seconds)
					} else {
						"—".into()
					},
					PLAY_W,
				))
				.child(numbers(
					game.last_played
						.map(format_date)
						.unwrap_or_else(|| "—".into()),
					DATE_W,
				))
				.child(numbers(
					if game.added > 0 {
						format_date(game.added)
					} else {
						"—".into()
					},
					DATE_W,
				))
				.context_menu(self.menu(&game, cx))
				.into_any_element()
		});
		let rows: Vec<AnyElement> = rows.collect();
		v_flex()
			.size_full()
			.child(header)
			.child(
				div()
					.id("library-list")
					.flex_1()
					.min_h_0()
					.overflow_y_scroll()
					.child(v_flex().children(pending).children(rows)),
			)
			.into_any_element()
	}
}
