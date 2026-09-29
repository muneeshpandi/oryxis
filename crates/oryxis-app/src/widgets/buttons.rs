//! UI helper widgets: buttons. Split out of widgets/mod.rs.

use iced::Widget as _;
use super::*;
/// Wide call-to-action button, Semibold label, theme-defined
/// `button_bg` / `button_text` pair, fixed 380-wide / 8 px radius.
/// Used for empty-state primary actions on Keys / Snippets and
/// anywhere else we want the same prominent affordance.
pub(crate) fn cta_button<'a>(label: String, msg: Message) -> Element<'a, Message> {
    let fg = OryxisColors::t().button_text;
    button(
        container(
            text(label)
                .size(14)
                .font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                })
                .color(fg),
        )
        .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
        .width(380)
        .center_x(380),
    )
    .on_press(msg)
    .width(380)
    .style(|_, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => OryxisColors::t().button_bg_hover,
            _ => OryxisColors::t().button_bg,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        }
    })
    .boxed()
}

/// Primary styled button, bold Inter, compact vertical padding, wide
/// horizontal padding. Used for Connect / Save / Cancel / destructive actions.
/// On hover the background lightens; keeps consistent language with split
/// buttons elsewhere (+ HOST, + ADD).
pub(crate) fn styled_button(label: &str, msg: Message, color: Color) -> Element<'_, Message> {
    styled_button_opt(label, Some(msg), color)
}

/// Like [`styled_button`] but the action is optional. `None` renders a
/// muted, non-interactive button (no `on_press`), so actions that only
/// apply when some state exists (e.g. "Reset hints" with no dismissed
/// hints) can communicate "nothing to do" instead of silently no-oping.
pub(crate) fn styled_button_opt(
    label: &str,
    msg: Option<Message>,
    color: Color,
) -> Element<'_, Message> {
    styled_button_owned(label.to_owned(), msg, color)
}

/// [`styled_button_opt`] for a label built at render time (a `format!`
/// with a host name in it). The borrowed variants above tie the
/// element's lifetime to the label they were given even though the
/// text is copied; this one owns it, so a dialog can return it.
pub(crate) fn styled_button_owned<'a>(
    label: String,
    msg: Option<Message>,
    color: Color,
) -> Element<'a, Message> {
    let enabled = msg.is_some();
    // Accent-colored CTAs share the per-theme `button_text` pairing so
    // every primary button (here, `+ HOST`, `+ ADD`, `New Snippet`,
    // etc.) renders in the same text color across the app. Non-accent
    // call sites (Cancel on bg_hover, Destroy on error, …) still
    // auto-pick via the luminance heuristic.
    let fg = if !enabled {
        OryxisColors::t().text_muted
    } else if color == OryxisColors::t().accent {
        OryxisColors::t().button_text
    } else {
        crate::theme::contrast_text_for(color)
    };
    let disabled_bg = OryxisColors::t().bg_selected;
    let mut b = button(
        container(
            text(label).size(12).font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            }).color(fg),
        )
        .padding(Padding { top: 5.0, right: 18.0, bottom: 5.0, left: 18.0 }),
    )
    .style(move |_, status| {
        let bg = if !enabled {
            disabled_bg
        } else {
            match status {
                iced::widget::button::Status::Hovered => Color {
                    a: 1.0,
                    r: (color.r + 0.05).min(1.0),
                    g: (color.g + 0.05).min(1.0),
                    b: (color.b + 0.05).min(1.0),
                },
                _ => color,
            }
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    });
    if let Some(msg) = msg {
        b = b.on_press(msg);
    }
    b.boxed()
}

/// Like [`styled_button`] but with a leading icon glyph. The caller colors
/// the icon (accent CTAs pair with `button_text`, like the label); the
/// background carries the hover feedback, matching [`styled_button`], and
/// `dir_row` keeps the icon on the leading edge under RTL.
pub(crate) fn styled_icon_button<'a>(
    icon: Element<'a, Message>,
    label: &'a str,
    msg: Message,
    color: Color,
) -> Element<'a, Message> {
    let fg = if color == OryxisColors::t().accent {
        OryxisColors::t().button_text
    } else {
        crate::theme::contrast_text_for(color)
    };
    button(
        container(
            dir_row(vec![
                icon,
                Space::new().width(8).boxed(),
                text(label.to_owned())
                    .size(12)
                    .font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                    })
                    .color(fg)
                    .boxed(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 6.0, right: 18.0, bottom: 6.0, left: 18.0 }),
    )
    .on_press(msg)
    .style(move |_, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => Color {
                a: 1.0,
                r: (color.r + 0.05).min(1.0),
                g: (color.g + 0.05).min(1.0),
                b: (color.b + 0.05).min(1.0),
            },
            _ => color,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .boxed()
}

/// The standard panel/modal header dismiss button: the muted `×` that
/// closes a form or side panel from its top-right corner. One size, one
/// padding, one hover treatment for every header across the app, so the
/// close affordance reads the same whether it sits on the Edit Host
/// tray, a Settings sub-panel, or a Keys dialog.
///
/// This exists because these buttons had drifted to 13 / 14 / 16 / 20 px
/// with no hover feedback, each hand-built at its call site. Route new
/// header closers through here rather than re-typing `button(text("×"))`.
pub(crate) fn panel_close_button<'a>(msg: Message) -> Element<'a, Message> {
    button(
        text("\u{00D7}")
            .size(18)
            .color(OryxisColors::t().text_muted),
    )
    .on_press(msg)
    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
    .style(|_, status| {
        // Transparent until hovered/pressed, then a faint wash so the
        // corner answers the pointer like the other icon affordances.
        let bg = match status {
            iced::widget::button::Status::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
            iced::widget::button::Status::Pressed => Color::from_rgba(1.0, 1.0, 1.0, 0.12),
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .boxed()
}
