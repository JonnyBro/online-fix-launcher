use rust_i18n::t;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{
	ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use onlinefix_launcher_core::settings::{Language, Theme};
use onlinefix_launcher_core::{Command, Engine, Settings};

use crate::theme::Mono;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Category {
	Appearance,
	General,
}

impl Category {
	const ALL: [Category; 2] = [Category::Appearance, Category::General];

	fn label(self) -> String {
		match self {
			Category::Appearance => t!("set.cat_appearance"),
			Category::General => t!("set.cat_general"),
		}
		.into_owned()
	}

	fn icon(self) -> Icon {
		match self {
			Category::Appearance => Icon::new(IconName::Palette),
			Category::General => Icon::new(IconName::Settings2),
		}
	}
}

/// Every setting so far applies and saves at once.
pub struct SettingsView {
	engine: Rc<Engine>,
	settings: Settings,
	category: Category,
}

impl SettingsView {
	pub fn new(engine: Rc<Engine>, settings: Settings) -> Self {
		Self {
			engine,
			settings,
			category: Category::Appearance,
		}
	}

	pub fn on_saved(&mut self, settings: Settings, cx: &mut Context<Self>) {
		self.settings = settings;
		cx.notify();
	}

	fn save_now(&mut self, change: impl Fn(&mut Settings)) {
		change(&mut self.settings);
		self.engine
			.send(Command::SaveSettings(self.settings.clone()));
	}

	fn set_theme(&mut self, theme: Theme, window: &mut Window, cx: &mut Context<Self>) {
		self.save_now(|s| s.theme = theme);
		crate::theme::apply(&self.settings, window, cx);
		cx.notify();
	}

	fn set_scheme(
		&mut self,
		dark: bool,
		name: String,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		// Picking a scheme also picks its mode; "System" stays until a scheme is clicked.
		self.save_now(|s| {
			if dark {
				s.dark_scheme = name.clone();
				s.theme = Theme::Dark;
			} else {
				s.light_scheme = name.clone();
				s.theme = Theme::Light;
			}
		});
		crate::theme::apply(&self.settings, window, cx);
		cx.notify();
	}

	/// Small previews of the bundled dark or light schemes; a click picks one.
	fn scheme_picker(&self, dark: bool, cx: &mut Context<Self>) -> AnyElement {
		let theme = cx.theme().clone();
		let (current, default) = if dark {
			(&self.settings.dark_scheme, crate::theme::DEFAULT_DARK)
		} else {
			(&self.settings.light_scheme, crate::theme::DEFAULT_LIGHT)
		};
		let current = if current.is_empty() {
			default
		} else {
			current.as_str()
		};
		let cards = crate::theme::schemes()
			.iter()
			.filter(|s| s.dark == dark)
			.enumerate()
			.map(|(ix, scheme)| {
				let selected = scheme.name == current;
				// The scheme of the other mode is remembered, shown fainter.
				let in_use = selected
					&& match self.settings.theme {
						Theme::System => true,
						Theme::Dark => dark,
						Theme::Light => !dark,
					};
				let label = if scheme.name == default {
					t!("set.scheme_default").into_owned()
				} else {
					scheme.name.clone()
				};
				let name = scheme.name.clone();
				let line = |width: f32| {
					div()
						.h(px(4.))
						.w(px(width))
						.rounded_full()
						.bg(scheme.foreground.opacity(0.6))
				};
				v_flex()
					.id((if dark { "dark-scheme" } else { "light-scheme" }, ix))
					.w(px(150.))
					.gap_1()
					.p_1()
					.rounded(theme.radius_lg)
					.border_2()
					.border_color(if in_use {
						theme.primary
					} else if selected {
						theme.muted_foreground.opacity(0.4)
					} else {
						theme.transparent
					})
					.cursor_pointer()
					.hover(|s| s.bg(theme.list_hover))
					.on_click(cx.listener(move |this, _, window, cx| {
						this.set_scheme(dark, name.clone(), window, cx)
					}))
					.child(
						h_flex()
							.h(px(64.))
							.rounded(theme.radius)
							.overflow_hidden()
							.border_1()
							.border_color(theme.border)
							.bg(scheme.background)
							.child(div().w(px(36.)).h_full().bg(scheme.surface))
							.child(
								v_flex()
									.flex_1()
									.p_2()
									.gap_1p5()
									.child(line(70.))
									.child(line(50.))
									.child(
										div()
											.mt_1()
											.h(px(12.))
											.w(px(40.))
											.rounded(px(3.))
											.bg(scheme.accent),
									),
							),
					)
					.child(div().px_1().text_sm().truncate().child(label))
			});
		h_flex()
			.gap_2()
			.flex_wrap()
			.children(cards)
			.into_any_element()
	}

	fn set_language(&mut self, language: Language, window: &mut Window, cx: &mut Context<Self>) {
		self.save_now(|s| s.language = language);
		crate::apply_language(language);
		window.refresh();
		cx.notify();
	}

	fn scale_control(&self, cx: &mut Context<Self>) -> AnyElement {
		let scale = crate::theme::scale(cx);
		let step = |id: &'static str, label: &'static str, delta: f32, enabled: bool| {
			Button::new(id)
				.label(label)
				.small()
				.outline()
				.disabled(!enabled)
				.on_click(move |_, window, cx| {
					crate::theme::set_scale(crate::theme::scale(cx) + delta, window, cx)
				})
		};
		v_flex()
			.gap_1()
			.child(
				h_flex()
					.gap_2()
					.child(step(
						"scale-down",
						"-",
						-0.1,
						scale > crate::theme::SCALE_MIN,
					))
					.child(
						div()
							.w(px(56.))
							.text_center()
							.mono(cx)
							.child(format!("{:.0}%", scale * 100.)),
					)
					.child(step("scale-up", "+", 0.1, scale < crate::theme::SCALE_MAX))
					.child(
						Button::new("scale-reset")
							.label(t!("set.scale_reset"))
							.small()
							.outline()
							.disabled((scale - 1.).abs() < f32::EPSILON)
							.on_click(|_, window, cx| crate::theme::set_scale(1., window, cx)),
					),
			)
			.into_any_element()
	}

	fn appearance(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let choice = |id: &'static str, label: String, selected: bool| {
			Button::new(id)
				.label(label)
				.small()
				.outline()
				.selected(selected)
		};
		let theme = self.settings.theme;
		let language = self.settings.language;
		v_flex()
			.gap_4()
			.child(hint(t!("set.instant_hint").into(), cx))
			.child(field(
				t!("set.language").into(),
				h_flex().gap_1().children(
					[
						(
							"lang-system",
							Language::System,
							t!("set.language_system").into_owned(),
						),
						// Language names stay in their own language.
						("lang-en", Language::English, "English".to_owned()),
						("lang-ru", Language::Russian, "Русский".to_owned()),
					]
					.into_iter()
					.map(|(id, value, label)| {
						choice(id, label, language == value).on_click(cx.listener(
							move |this, _, window, cx| this.set_language(value, window, cx),
						))
					}),
				),
				cx,
			))
			.child(field(t!("set.scale").into(), self.scale_control(cx), cx))
			.child(field(
				t!("set.theme").into(),
				h_flex().gap_1().children(
					[("theme-system", Theme::System, t!("set.theme_system"))]
						.into_iter()
						.map(|(id, value, label)| {
							choice(id, label.into_owned(), theme == value).on_click(cx.listener(
								move |this, _, window, cx| this.set_theme(value, window, cx),
							))
						}),
				),
				cx,
			))
			.child(field(
				t!("set.dark_scheme").into(),
				self.scheme_picker(true, cx),
				cx,
			))
			.child(field(
				t!("set.light_scheme").into(),
				self.scheme_picker(false, cx),
				cx,
			))
	}

	fn general(&self, cx: &mut Context<Self>) -> impl IntoElement {
		v_flex().gap_4().child(field(
			t!("set.updates").into(),
			Checkbox::new("check-updates")
				.label(t!("set.check_updates").into_owned())
				.checked(self.settings.check_app_updates)
				.on_click(cx.listener(|this, checked: &bool, _, cx| {
					let checked = *checked;
					this.save_now(|s| s.check_app_updates = checked);
					cx.notify();
				})),
			cx,
		))
	}
}

impl Render for SettingsView {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.theme().clone();
		let nav = v_flex()
			.w(px(220.))
			.flex_shrink_0()
			.h_full()
			.p_3()
			.gap_1()
			.border_r_1()
			.border_color(theme.border)
			.children(Category::ALL.into_iter().enumerate().map(|(ix, category)| {
				Button::new(("category", ix))
					.icon(category.icon())
					.label(category.label())
					.ghost()
					.w_full()
					.justify_start()
					.selected(self.category == category)
					.on_click(cx.listener(move |this, _, _, cx| {
						this.category = category;
						cx.notify();
					}))
			}));
		let body = match self.category {
			Category::Appearance => self.appearance(cx).into_any_element(),
			Category::General => self.general(cx).into_any_element(),
		};
		h_flex().size_full().child(nav).child(
			div()
				.id("settings-body")
				.flex_1()
				.min_w_0()
				.h_full()
				.overflow_y_scroll()
				.child(
					v_flex()
						.max_w(px(820.))
						.px_6()
						.py_5()
						.gap_5()
						.child(div().text_xl().child(self.category.label()))
						.child(body),
				),
		)
	}
}

fn field(label: String, control: impl IntoElement, cx: &App) -> impl IntoElement {
	h_flex()
		.gap_4()
		.items_start()
		.child(
			div()
				.w(px(200.))
				.pt_1()
				.flex_shrink_0()
				.text_color(cx.theme().muted_foreground)
				.child(label),
		)
		.child(div().flex_1().min_w_0().child(control))
}

fn hint(text: String, cx: &App) -> impl IntoElement {
	div()
		.text_sm()
		.text_color(cx.theme().muted_foreground)
		.child(text)
}
