//! Standalone window chrome, drag region + minimize / maximize / close.
//!
//! Used by screens that don't render the full tab bar (vault setup / unlock /
//! error). The main app layout embeds the controls inside `view_tab_bar`.
//!
//! Also the one authority for WHO draws the window frame
//! ([`NATIVE_FRAME`]) and the predicates every bar asks about it.

use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, container, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length};
use iced_fonts::codicon as cd;

use crate::app::{Message, TabsMessage};
use crate::theme::OryxisColors;

/// Whether the OS draws the window frame instead of us (issue #238).
///
/// macOS: AppKit keeps its decorations with a transparent title bar laid
/// over our content (`main.rs`), so the traffic lights, their native
/// fullscreen, and the resize edges are the platform's. Our own
/// minimize / maximize / close trio, the edge resize handles and the 1 px
/// frame all stand down there, and our bars reserve the corner the traffic
/// lights float in ([`TRAFFIC_LIGHT_INSET`]). A Mac user reads buttons on
/// the right as a Windows port, and a green button that only zooms as a
/// fullscreen that does not work.
///
/// Everywhere else the window is undecorated and the chrome is ours.
pub(crate) const NATIVE_FRAME: bool = cfg!(target_os = "macos");

/// Width reserved at the PHYSICAL left of a bar for the traffic lights
/// (three 14 pt buttons from x = 7 with 6 pt gaps is ~61 pt, plus the
/// breathing room AppKit's own toolbars leave before the first item).
/// Physical because AppKit never moves them for a right-to-left layout.
pub(crate) const TRAFFIC_LIGHT_INSET: f32 = 78.0;

/// Height of a standard AppKit title bar. With the content running
/// underneath it, its vertical centre is where the traffic lights sit.
pub(crate) const MACOS_TITLE_BAR_HEIGHT: f32 = 28.0;

/// The traffic-light corner of a bar: an empty drag area, since the
/// buttons AppKit draws over it take their own clicks and the rest of the
/// corner is title bar like any other empty stretch of it.
pub(crate) fn traffic_light_gutter<'a>(width: f32, height: f32) -> Element<'a, Message> {
    MouseArea::new(
        container(Space::new())
            .width(Length::Fixed(width))
            .height(Length::Fixed(height)),
    )
    .on_press(Message::Tabs(TabsMessage::WindowDrag))
    .on_double_click(Message::Tabs(TabsMessage::WindowMaximizeToggle))
    .into()
}

impl crate::app::Oryxis {
    /// Fullscreen that HIDES our chrome (tab bar, status bar, the
    /// "press F11" hint and the hover X): the mode F11 enters, on every
    /// platform. The one fullscreen that is not immersive is macOS's
    /// green button, a Space of its own that keeps the app's tabs like
    /// every Mac app does (`fullscreen_immersive` tells the two apart).
    pub(crate) fn immersive_fullscreen(&self) -> bool {
        self.cur_fullscreen() && self.cur_immersive()
    }

    /// Width of the traffic-light corner the top-most bar must leave
    /// empty right now. Zero off macOS, and zero in native fullscreen,
    /// where AppKit hides the buttons until the pointer reaches the top
    /// edge and then shows them in a title bar of their own above ours.
    pub(crate) fn traffic_light_inset(&self) -> f32 {
        if NATIVE_FRAME && !self.cur_fullscreen() {
            TRAFFIC_LIGHT_INSET
        } else {
            0.0
        }
    }

    /// Prefix `bar` with the traffic-light corner when one is due. A
    /// plain `row`, never `dir_row`: the corner is physical.
    pub(crate) fn with_traffic_light_gutter<'a>(
        &self,
        bar: Element<'a, Message>,
        height: f32,
    ) -> Element<'a, Message> {
        let inset = self.traffic_light_inset();
        if inset <= 0.0 {
            return bar;
        }
        iced::widget::row![traffic_light_gutter(inset, height), bar]
            .align_y(iced::Alignment::Center)
            .into()
    }

    /// Whether the side dock's "hide the top bar" is in effect: the
    /// preference, on a side dock, and on macOS only for the LEFT dock.
    /// The traffic lights live in the window's top-left corner whatever
    /// we draw, so with the strip on the right they would sit on the
    /// content itself; there the slim top bar stays to carry them. One
    /// predicate for every reader (layout, strip header, menus anchors,
    /// the drag hit bands), so none of them can disagree about where the
    /// burger is.
    pub(crate) fn top_bar_hidden(&self) -> bool {
        let pos = crate::views::tab_bar::tab_bar_pos();
        pos.is_side()
            && self.prefs.side_hide_top_bar
            && !(NATIVE_FRAME && pos == crate::views::tab_bar::TabBarPos::Right)
    }
}

/// Chrome bar height, must match the main view's `BAR_HEIGHT` so the lock
/// screen's chrome doesn't visually pop when transitioning to the main app
/// after unlocking.
const CHROME_HEIGHT: f32 = 40.0;

/// Top bar with drag region + window controls. Renders at a fixed 28 px height
/// in the sidebar background tone so it blends with the tab bar on the main
/// screen. Uses VS Code's codicon glyphs to match the native Windows chrome
/// look the user expects (and stays identical cross-platform).
pub(crate) fn window_chrome_bar<'a>() -> Element<'a, Message> {
    // macOS: the whole bar is the drag area the traffic lights float over.
    // No gutter needed, nothing of ours sits in their corner.
    let drag_region: Element<'_, Message> = MouseArea::new(
        container(Space::new().width(Length::Fill).height(Length::Fixed(CHROME_HEIGHT)))
            .width(Length::Fill)
            .height(Length::Fixed(CHROME_HEIGHT)),
    )
    .on_press(Message::Tabs(TabsMessage::WindowDrag))
    .into();

    // Lock screen doesn't know whether the window is currently maximized; the
    // toggle works either way, so we always show the maximize glyph here.
    // `dir_row` flips the trio under RTL so close ends up on the leading edge.
    let buttons = if NATIVE_FRAME {
        Vec::new()
    } else {
        vec![
            chrome_btn(cd::chrome_minimize(), Message::Tabs(TabsMessage::WindowMinimize), OryxisColors::t().text_secondary),
            chrome_btn(cd::chrome_maximize(), Message::Tabs(TabsMessage::WindowMaximizeToggle), OryxisColors::t().text_secondary),
            chrome_btn(cd::chrome_close(), Message::Tabs(TabsMessage::WindowClose), OryxisColors::t().error),
        ]
    };
    let controls: Element<'_, Message> = crate::widgets::dir_row(buttons)
    .align_y(iced::Alignment::Center)
    .into();

    container(
        crate::widgets::dir_row(vec![drag_region, controls])
            .align_y(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
        ..Default::default()
    })
    .into()
}

fn chrome_btn<'a>(
    icon: iced::widget::Text<'a>,
    msg: Message,
    hover_color: Color,
) -> Element<'a, Message> {
    button(
        container(icon.size(15).color(OryxisColors::t().text_secondary))
            .center(Length::Fixed(46.0))
            .height(Length::Fixed(CHROME_HEIGHT)),
    )
    // Same fix as the main tab bar, `button` defaults to 5 px padding on
    // top/bottom, which prevented the hover background from filling the
    // whole chrome strip.
    .padding(0)
    .on_press(msg)
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
    .into()
}
