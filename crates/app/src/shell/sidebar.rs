use super::*;

impl Shell {
	fn nav_item(&self, tab: Tab, cx: &mut Context<Self>) -> SidebarMenuItem {
		SidebarMenuItem::new(tab.label())
			.icon(Icon::from(tab.icon()))
			.active(self.tab == tab)
			.on_click(cx.listener(move |this, _, _, cx| this.set_tab(tab, cx)))
	}

	fn header(&self, cx: &mut Context<Self>) -> SidebarHeader {
		let theme = cx.theme();
		SidebarHeader::new().w_full().child(
			h_flex()
				.w_full()
				.gap_2()
				.child(img(crate::icons::APP_ICON).size_8().flex_shrink_0())
				.child(
					v_flex()
						.flex_1()
						.min_w_0()
						.child(div().truncate().child(crate::APP_NAME))
						.child(
							div()
								.text_sm()
								.text_color(theme.muted_foreground)
								.truncate()
								.child(t!("app.tagline").into_owned()),
						),
				),
		)
	}

	pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let main = SidebarMenu::new().children([
			self.nav_item(Tab::Library, cx),
			self.nav_item(Tab::Proton, cx),
		]);
		Sidebar::new("sidebar")
			.collapsible(SidebarCollapsible::None)
			.w(px(240.))
			.header(self.header(cx))
			.child(SidebarGroup::new(t!("nav.main")).child(main))
			.footer(
				SidebarFooter::new().p_0().child(
					Button::new("nav-settings")
						.icon(Icon::from(AppIcon::Settings))
						.label(Tab::Settings.label())
						.ghost()
						.w_full()
						.justify_start()
						.selected(self.tab == Tab::Settings)
						.on_click(cx.listener(|this, _, _, cx| this.set_tab(Tab::Settings, cx))),
				),
			)
	}
}
