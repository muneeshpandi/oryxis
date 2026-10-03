//! Theme traits as the user sees them (issue #230): the localized label
//! of each measured trait, the tag line a theme card shows beside its
//! name, the ONE filter rule every theme list applies, and the
//! All / Dark / Light chips.
//!
//! The traits themselves are measured in `oryxis_terminal::colors`
//! (`palette_traits`), never declared: a custom or imported palette
//! earns exactly the same tags as a built-in. This module only puts
//! words on them.

use iced::Widget as _;
use iced::border::Radius;
use iced::widget::{button, container, text};
use iced::{Background, Border, Color, Element, Length, Padding};
use oryxis_terminal::{ThemeTone, ThemeTrait};

use crate::app::Message;
use crate::i18n::t;
use crate::theme::OryxisColors;

/// Localized label of a trait.
pub(crate) fn trait_label(tr: ThemeTrait) -> &'static str {
    t(match tr {
        ThemeTrait::Dark => "theme_tag_dark",
        ThemeTrait::Light => "theme_tag_light",
        ThemeTrait::HighContrast => "theme_tag_high_contrast",
        ThemeTrait::LowContrast => "theme_tag_low_contrast",
        ThemeTrait::Warm => "theme_tag_warm",
        ThemeTrait::Cool => "theme_tag_cool",
        ThemeTrait::Vivid => "theme_tag_vivid",
        ThemeTrait::Muted => "theme_tag_muted",
    })
}

/// The tags a card shows beside its name: every trait except the tone,
/// which the swatch already states. Empty for a plain neutral palette.
pub(crate) fn card_tag_line(traits: &[ThemeTrait]) -> String {
    traits
        .iter()
        .filter(|t| t.tone().is_none())
        .map(|t| trait_label(*t))
        .collect::<Vec<_>>()
        .join(" \u{00b7} ")
}

/// Traits of a chrome (UI) theme: the same measurement the terminal
/// palettes get, over the chrome's base pair and its accent plus the
/// three semantic colours (what its card shows as dots).
pub(crate) fn ui_theme_traits(colors: &crate::theme::ThemeColors) -> Vec<ThemeTrait> {
    oryxis_terminal::palette_traits(
        colors.bg_primary,
        colors.text_primary,
        &[colors.accent, colors.success, colors.warning, colors.error],
    )
}

/// The filter rule shared by every theme list. `filter` is the typed
/// line, already trimmed and lower-cased; `tone` the active chip.
///
/// - The chip is a hard gate on a card that HAS a tone (a palette);
///   action cards ("+ New custom theme", Import, Community) carry no
///   palette and stay visible under any chip, so the way to create or
///   import never disappears behind a filter.
/// - The line matches the card's visible label, or any trait's
///   localized label, or its English keyword, so "浅色" and "light" both
///   find the light themes in a Chinese UI, and "warm" finds Gruvbox in
///   any UI language. Substring, case-insensitive, no special cases.
pub(crate) fn theme_matches(
    filter: &str,
    tone: Option<ThemeTone>,
    label: &str,
    traits: Option<&[ThemeTrait]>,
) -> bool {
    if let (Some(want), Some(traits)) = (tone, traits)
        && !traits.iter().any(|t| t.tone() == Some(want))
    {
        return false;
    }
    if filter.is_empty() || label.to_lowercase().contains(filter) {
        return true;
    }
    traits.is_some_and(|traits| {
        traits.iter().any(|t| {
            t.keyword().contains(filter) || trait_label(*t).to_lowercase().contains(filter)
        })
    })
}

/// The three chip choices, in display order, with their label keys.
pub(crate) const TONE_CHOICES: [(Option<ThemeTone>, &str); 3] = [
    (None, "theme_filter_all"),
    (Some(ThemeTone::Dark), "theme_tag_dark"),
    (Some(ThemeTone::Light), "theme_tag_light"),
];

/// One All / Dark / Light chip. Hover and press feedback on the fill,
/// the accent tint while it is the active choice.
pub(crate) fn tone_chip<'a>(label: &'a str, active: bool, msg: Message) -> Element<'a, Message> {
    let palette = OryxisColors::t();
    let fg = if active { palette.button_text } else { palette.text_secondary };
    button(
        container(text(label).size(12).color(fg))
            .padding(Padding { top: 4.0, right: 12.0, bottom: 4.0, left: 12.0 })
            .center_y(Length::Shrink),
    )
    .on_press(msg)
    .padding(0)
    .style(move |_, status| {
        let palette = OryxisColors::t();
        let bg = match (active, status) {
            (true, button::Status::Hovered) => palette.accent_hover,
            (true, _) => palette.accent,
            (false, button::Status::Hovered) => palette.bg_hover,
            (false, button::Status::Pressed) => palette.bg_selected,
            (false, _) => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(14.0),
                color: if active { palette.accent } else { palette.border },
                width: 1.0,
            },
            ..Default::default()
        }
    })
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chip_gates_palettes_and_spares_action_cards() {
        let dark = [ThemeTrait::Dark, ThemeTrait::Cool];
        assert!(theme_matches("", Some(ThemeTone::Dark), "Nord", Some(&dark)));
        assert!(!theme_matches("", Some(ThemeTone::Light), "Nord", Some(&dark)));
        assert!(theme_matches("", Some(ThemeTone::Light), "Import", None));
    }

    #[test]
    fn the_line_matches_label_keyword_or_localized_label() {
        let traits = [ThemeTrait::Dark, ThemeTrait::Warm];
        assert!(theme_matches("gruv", None, "Gruvbox Dark", Some(&traits)));
        assert!(theme_matches("warm", None, "Gruvbox Dark", Some(&traits)));
        assert!(theme_matches("dark", None, "Gruvbox", Some(&traits)));
        assert!(!theme_matches("cool", None, "Gruvbox Dark", Some(&traits)));
        // An action card only ever matches its own label.
        assert!(!theme_matches("warm", None, "Import", None));
        assert!(theme_matches("imp", None, "Import", None));
    }

    #[test]
    fn the_tag_line_leaves_the_tone_to_the_swatch() {
        let line = card_tag_line(&[ThemeTrait::Dark, ThemeTrait::Warm, ThemeTrait::Muted]);
        assert!(!line.to_lowercase().contains("dark"), "{line}");
        assert_eq!(line.matches('\u{00b7}').count(), 1);
        assert!(card_tag_line(&[ThemeTrait::Light]).is_empty());
    }
}
