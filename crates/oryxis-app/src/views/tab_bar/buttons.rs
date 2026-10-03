//! Tab bar: buttons. Split out of views/tab_bar/mod.rs.

use super::*;
/// Plus button that trails the last tab (browser-style), opening the
/// new-tab picker (search + recent connections) as a centered modal
/// overlay. `inline` renders it tab-strip-sized with rounded hover
/// (sitting among the tabs); the docked variant (strip overflow) keeps
/// the squared full-height look so it reads as part of the chrome
/// strip next to it. `PLUS_BUTTON_WIDTH` still feeds the layout-math
/// right-cluster budget in both placements.
///
/// Uses `lucide::plus` instead of a literal `+` text character, on
/// Windows, Segoe UI's `+` renders much chunkier than the codicon
/// `−` / `□` / `✕` glyphs right next to it, breaking visual rhythm.
pub(crate) fn new_tab_btn<'a>(inline: bool) -> Element<'a, Message> {
    let hover_color = OryxisColors::t().text_secondary;
    let height = if inline { BAR_HEIGHT - 8.0 } else { BAR_HEIGHT };
    let radius = if inline { 6.0 } else { 0.0 };
    let btn = button(
        container(iced_fonts::lucide::plus().size(15).color(hover_color))
            .center(Length::Fixed(height))
            .height(Length::Fixed(height)),
    )
    .on_press(Message::Tabs(TabsMessage::ShowNewTabPicker))
    .padding(0)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color { a: 0.2, ..hover_color },
            BtnStatus::Pressed => Color { a: 0.35, ..hover_color },
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(radius), ..Default::default() },
            ..Default::default()
        }
    });
    // Click = new tab (default). Hovering reveals the New-Tab / Split
    // popover (no-op unless a terminal tab is open, see `ShowSplitMenu`).
    MouseArea::new(btn)
        .on_enter(Message::Tabs(TabsMessage::ShowSplitMenu))
        .on_exit(Message::Tabs(TabsMessage::SplitMenuLeave))
        .boxed()
}

/// The reserved window-drag handle (`DRAG_SPACER_WIDTH`, issue #226):
/// the same three gestures the strip's empty area answers, because it
/// IS that area with a floor under it. Drag moves the window,
/// double-click toggles maximize (the native title-bar convention), and
/// a right-click opens the strip menu the way the strip's own slack does
/// (issue #186); the menu carries nothing destructive for exactly this
/// reason, these pixels are one flick away from a window drag. Not a
/// button: it has no click and therefore no hover feedback to owe.
pub(crate) fn drag_spacer<'a>() -> Element<'a, Message> {
    MouseArea::new(
        container(Space::new())
            .width(Length::Fixed(DRAG_SPACER_WIDTH))
            .height(Length::Fixed(BAR_HEIGHT)),
    )
    .on_press(Message::Tabs(TabsMessage::WindowDrag))
    .on_double_click(Message::Tabs(TabsMessage::WindowMaximizeToggle))
    .on_right_press(Message::Tabs(TabsMessage::ShowTabBarMenu))
    .boxed()
}

/// Tab-jump button, opens the Termius-style "Jump to" modal listing
/// all open tabs + Quick connect entries. Always visible regardless of
/// how many tabs are open, so the user has a discoverable escape hatch
/// from a packed tab strip.
pub(crate) fn tab_jump_btn<'a>() -> Element<'a, Message> {
    let hover_color = OryxisColors::t().text_secondary;
    button(
        container(
            text("\u{22EF}") // horizontal ellipsis ⋯
                .size(15)
                .color(hover_color),
        )
        .center(Length::Fixed(DOTS_BUTTON_WIDTH))
        .height(Length::Fixed(BAR_HEIGHT)),
    )
    .on_press(Message::Tabs(TabsMessage::ShowTabJump))
    .padding(0)
    .style(move |_, status| {
        // Match the window-chrome / new-tab buttons' subtle hover
        // and squared corners so the right cluster reads as one
        // continuous strip.
        let bg = match status {
            BtnStatus::Hovered => Color { a: 0.2, ..hover_color },
            BtnStatus::Pressed => Color { a: 0.35, ..hover_color },
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border::default(),
            ..Default::default()
        }
    })
    .boxed()
}

/// Terminal side-panel toggle, one per sidebar region (issue #102).
/// Sits right of the `+ new tab` button. The glyph names the PHYSICAL
/// region it drives (panel_left / panel_right), which is also why it
/// never flips under RTL: the region doesn't either.
pub(crate) fn sidebar_btn<'a>(
    side: crate::state::SidebarSide,
    cell_w: f32,
    cell_h: f32,
) -> Element<'a, Message> {
    let hover_color = OryxisColors::t().text_secondary;
    let (glyph, tip) = match side {
        crate::state::SidebarSide::Left => {
            (iced_fonts::lucide::panel_left(), crate::i18n::t("sidebar_toggle_left"))
        }
        crate::state::SidebarSide::Right => {
            (iced_fonts::lucide::panel_right(), crate::i18n::t("sidebar_toggle_right"))
        }
    };
    let btn = button(
        container(glyph.size(15).color(hover_color))
            .center(Length::Fixed(cell_w))
            .height(Length::Fixed(cell_h)),
    )
    .on_press(Message::Ai(AiMessage::ToggleSidebarRegion(side)))
    .padding(0)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color { a: 0.2, ..hover_color },
            BtnStatus::Pressed => Color { a: 0.35, ..hover_color },
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border::default(),
            ..Default::default()
        }
    });
    // Two near-identical glyphs can sit side by side now, so each
    // names its region (icon-only controls get a tooltip).
    crate::views::terminal::icon_tooltip(btn.boxed(), tip)
}

/// Burger menu trigger at the leading edge of the tab bar. When the
/// menu is open the button paints with the accent hover state so the
/// click affordance reads as "active control" instead of a stray glyph.
pub(crate) fn burger_menu_btn<'a>(is_open: bool) -> Element<'a, Message> {
    let hover_color = OryxisColors::t().text_secondary;
    let resting_bg = if is_open {
        Color { a: 0.2, ..hover_color }
    } else {
        Color::TRANSPARENT
    };
    // Symmetric padding around the glyph (no fixed-width `center`,
    // which left an empty right gap that read as a margin), so the
    // burger is all padding and no margin.
    button(
        container(
            iced_fonts::lucide::menu().size(15).color(hover_color),
        )
        .center_y(Length::Fixed(BAR_HEIGHT))
        .padding(Padding { top: 0.0, right: 11.0, bottom: 0.0, left: 11.0 }),
    )
    .on_press(Message::Tabs(TabsMessage::ToggleBurgerMenu))
    .padding(0)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color { a: 0.2, ..hover_color },
            BtnStatus::Pressed => Color { a: 0.35, ..hover_color },
            _ => resting_bg,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border::default(),
            ..Default::default()
        }
    })
    .boxed()
}

/// Minimize / maximize / close glyph button for the window chrome.
/// Fills the full cell height (no padding) so hover backgrounds reach
/// the very top and bottom edges, same behaviour as Windows / VS Code.
/// The bars use the standard 46 x BAR_HEIGHT cell; the side strip's
/// header (hidden top bar) passes a compact cell.
pub(crate) fn window_btn<'a>(
    icon: iced::widget::Text<'a>,
    msg: Message,
    hover_color: Color,
    cell_w: f32,
    cell_h: f32,
) -> Element<'a, Message> {
    button(
        container(icon.size(15).color(OryxisColors::t().text_secondary))
            .center(Length::Fixed(cell_w))
            .height(Length::Fixed(cell_h)),
    )
    .on_press(msg)
    .padding(0)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color { a: 0.2, ..hover_color },
            BtnStatus::Pressed => Color { a: 0.35, ..hover_color },
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border::default(),
            ..Default::default()
        }
    })
    .boxed()
}
