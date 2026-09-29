//! UI helper widgets: inputs. Split out of widgets/mod.rs.

use iced::Widget as _;
use super::*;

/// Focus a text input by id AND land the cursor at the end of its
/// value. Since the upstream unify-text-editing refactor, `focus`
/// only flips the focus flag (a freshly mounted input keeps its
/// cursor at position 0), so every "drop the keyboard into this
/// field" site in the app goes through here: on an empty field the
/// cursor move is a no-op, on a pre-filled one (rename, path bar,
/// editor forms) it restores the type-to-append behavior the app was
/// built around. Use `iced::widget::operation::focus` directly only
/// for the `__keynav_blur__` dummy-id blur trick.
pub(crate) fn focus_input<T: Send + 'static>(id: impl Into<iced::widget::Id>) -> iced::Task<T> {
    let id = id.into();
    iced::Task::batch([
        iced::widget::operation::focus(id.clone()),
        iced::widget::operation::text_input::move_cursor_to_end(id),
    ])
}

/// Shared style closure for `text_input`. Apply via `.style(rounded_input_style)`
/// to get the app's accent-focused look with the consistent 10 px radius.
pub fn rounded_input_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let c = OryxisColors::t();
    let (border_color, border_width) = match status {
        text_input::Status::Focused { .. } => (c.accent, 1.5),
        text_input::Status::Disabled => (c.border, 1.0),
        _ => (c.border, 1.0),
    };
    text_input::Style {
        background: Background::Color(c.bg_surface),
        border: Border {
            radius: Radius::from(INPUT_RADIUS),
            width: border_width,
            color: border_color,
        },
        placeholder: c.text_muted,
        value: c.text_primary,
        selection: c.accent,
    }
}

/// Shared style for multi-line `text_editor`s, matching
/// `rounded_input_style` so textareas sit next to single-line inputs
/// with the same geometry and focus feedback.
pub fn rounded_text_editor_style(
    _theme: &Theme,
    status: iced::widget::text_editor::Status,
) -> iced::widget::text_editor::Style {
    let c = OryxisColors::t();
    let (border_color, border_width) = match status {
        iced::widget::text_editor::Status::Focused { .. } => (c.accent, 1.5),
        _ => (c.border, 1.0),
    };
    iced::widget::text_editor::Style {
        background: Background::Color(c.bg_surface),
        border: Border {
            radius: Radius::from(INPUT_RADIUS),
            width: border_width,
            color: border_color,
        },
        placeholder: c.text_muted,
        value: c.text_primary,
        selection: c.accent,
    }
}

/// Shared menu style for `combo_box` dropdowns. Matches the app's
/// surface + border palette so the native overlay reads like the rest
/// of the popovers. Apply via `.menu_style(combo_menu_style)`.
pub fn combo_menu_style(_theme: &Theme) -> iced::widget::overlay::menu::Style {
    let c = OryxisColors::t();
    iced::widget::overlay::menu::Style {
        background: Background::Color(c.bg_surface),
        border: Border {
            radius: Radius::from(8.0),
            color: c.border,
            width: 1.0,
        },
        text_color: c.text_primary,
        selected_text_color: c.text_primary,
        selected_background: Background::Color(c.bg_hover),
        shadow: iced::Shadow::default(),
    }
}

/// Shared style closure for `text_editor` (multi-line). Mirrors
/// `rounded_input_style` so single-line and multi-line fields look identical:
/// same surface, border, radius, and accent-on-focus.
pub fn rounded_editor_style(_theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let c = OryxisColors::t();
    let (border_color, border_width) = match status {
        text_editor::Status::Focused { .. } => (c.accent, 1.5),
        _ => (c.border, 1.0),
    };
    text_editor::Style {
        background: Background::Color(c.bg_surface),
        border: Border {
            radius: Radius::from(INPUT_RADIUS),
            width: border_width,
            color: border_color,
        },
        placeholder: c.text_muted,
        value: c.text_primary,
        selection: c.accent,
    }
}

/// Password text-input with a Lucide eye toggle overlaid inside the rounded
/// border. The input reserves trailing padding for the icon (right under LTR,
/// left under RTL); the button lives in a `Stack` above the input,
/// leading-edge-anchored on the trailing side. Hit-testing is constrained to
/// the button's bounding box, so clicks on the rest of the field still focus
/// the input. `inner_padding` controls vertical and leading-edge inset (12
/// for the vault hero field, 10 for inline form rows).
pub(crate) fn password_input_with_eye<'a, F>(
    placeholder: &'a str,
    value: &'a str,
    on_input: F,
    on_submit: Option<Message>,
    visible: bool,
    on_toggle: Message,
    inner_padding: f32,
) -> Element<'a, Message>
where
    F: Fn(String) -> Message + 'a,
{
    password_input_with_eye_id(
        placeholder,
        value,
        on_input,
        on_submit,
        visible,
        on_toggle,
        inner_padding,
        None,
    )
}

/// `password_input_with_eye` with a focus id on the inner input, so
/// the keyboard router's input rows (Enter-to-focus) can target the
/// field. Kept as a sibling instead of a new parameter so the many
/// non-navigable call sites stay untouched.
#[allow(clippy::too_many_arguments)]
pub(crate) fn password_input_with_eye_id<'a, F>(
    placeholder: &'a str,
    value: &'a str,
    on_input: F,
    on_submit: Option<Message>,
    visible: bool,
    on_toggle: Message,
    inner_padding: f32,
    id: Option<iced::widget::Id>,
) -> Element<'a, Message>
where
    F: Fn(String) -> Message + 'a,
{
    password_input_with_eye_nav(
        placeholder,
        value,
        on_input,
        on_submit,
        visible,
        on_toggle,
        inner_padding,
        id,
        |eye| eye,
    )
}

/// `password_input_with_eye_id` whose eye button is wrapped by the
/// caller's keynav slot (`wrap_eye`), making the toggle a stop of the
/// surface's keyboard walk right after the field itself. The closure
/// runs during construction, so call sites must record the FIELD's
/// input row before building this widget to keep recording order equal
/// to display order.
#[allow(clippy::too_many_arguments)]
pub(crate) fn password_input_with_eye_nav<'a, F, W>(
    placeholder: &'a str,
    value: &'a str,
    on_input: F,
    on_submit: Option<Message>,
    visible: bool,
    on_toggle: Message,
    inner_padding: f32,
    id: Option<iced::widget::Id>,
    wrap_eye: W,
) -> Element<'a, Message>
where
    F: Fn(String) -> Message + 'a,
    W: FnOnce(Element<'a, Message>) -> Element<'a, Message>,
{
    let rtl = crate::i18n::is_rtl_layout();
    // Reserve ~32 px on the trailing edge so the eye icon doesn't overlap
    // typed text. Leading edge keeps the requested inner padding.
    let trailing = 32.0;
    let (pad_left, pad_right) = if rtl {
        (trailing, inner_padding)
    } else {
        (inner_padding, trailing)
    };
    let mut field = text_input(placeholder, value)
        .on_input(on_input)
        .secure(!visible)
        .align_x(dir_align_x())
        .padding(Padding {
            top: inner_padding,
            right: pad_right,
            bottom: inner_padding,
            left: pad_left,
        })
        .width(Length::Fill)
        .style(rounded_input_style);
    // Inline form rows (inner_padding 10) match the 13px body text of
    // the panels around them; the vault hero field (inner_padding 12)
    // keeps its larger default so the lock screen still reads as a hero
    // input.
    if inner_padding < 12.0 {
        field = field.size(13);
    }
    if let Some(id) = id {
        field = field.id(id);
    }
    if let Some(submit) = on_submit {
        field = field.on_submit(submit);
    }

    let icon = if visible {
        iced_fonts::lucide::eye_off()
    } else {
        iced_fonts::lucide::eye()
    }
    .size(14)
    .color(OryxisColors::t().text_muted);

    // Hover/press feedback carried by the background (convention:
    // every clickable affordance branches on button::Status), plus the
    // icon-only tooltip, reusing the Privacy Mode Reveal/Hide strings.
    let toggle = button(icon)
        .on_press(on_toggle)
        .style(|_t, status| {
            let bg = match status {
                button::Status::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                button::Status::Pressed => Color::from_rgba(1.0, 1.0, 1.0, 0.12),
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: Radius::from(6.0),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .padding(4);
    let tip_key = if visible { "privacy_hide" } else { "privacy_reveal" };
    let toggle = iced::widget::tooltip(
        toggle,
        container(text(crate::i18n::t(tip_key)).size(11))
            .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(6.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            }),
        iced::widget::tooltip::Position::Bottom,
    );
    let toggle = wrap_eye(toggle.boxed());

    let (align, overlay_pad) = if rtl {
        (
            iced::alignment::Horizontal::Left,
            Padding { left: 2.0, ..Padding::ZERO },
        )
    } else {
        (
            iced::alignment::Horizontal::Right,
            Padding { right: 2.0, ..Padding::ZERO },
        )
    };

    let toggle_overlay = container::<_, iced::Theme>(toggle)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(align)
        .align_y(iced::alignment::Vertical::Center)
        .padding(overlay_pad);

    Stack::<iced::Element<'_, _>>::new()
        .push(field.boxed())
        .push(toggle_overlay.boxed())
        .width(Length::Fill)
        .boxed()
}

/// Shared style closure for `pick_list`, matches `rounded_input_style` so
/// selects and inputs sit side-by-side with the same geometry.
pub fn rounded_pick_list_style(_theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let c = OryxisColors::t();
    let border_color = match status {
        pick_list::Status::Opened { .. } => c.accent,
        // Keyboard focus reads like an open-ready state: full accent,
        // matching the focused text_input border.
        pick_list::Status::Focused { .. } => c.accent,
        pick_list::Status::Hovered => c.accent_hover,
        _ => c.border,
    };
    pick_list::Style {
        text_color: c.text_primary,
        placeholder_color: c.text_muted,
        handle_color: c.text_muted,
        background: Background::Color(c.bg_surface),
        border: Border {
            radius: Radius::from(INPUT_RADIUS),
            width: 1.0,
            color: border_color,
        },
    }
}
