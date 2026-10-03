//! Resolves what the terminal's backdrop looks like right now: the
//! translucency and the background picture, per-host override first and
//! the global settings underneath.
//!
//! One module because the two travel together and interact: a picture
//! sits on top of the translucent layer and hides it completely, so a
//! host with both configured would be paying for a see-through window
//! that shows nothing. The resolver settles that once (picture wins,
//! opacity is reported as opaque while one is set) instead of leaving
//! every call site to guess.
//!
//! Resolution starts from the tab's label, the same route
//! `resolve_terminal_palette_for_label` takes, so a tab whose host was
//! renamed or which has no host at all (local shell, WSL) falls through
//! to the global settings rather than failing.

use crate::app::Oryxis;
use oryxis_terminal::{BackgroundImage, BgFit};

/// The backdrop for one tab, already reduced to what the widget needs.
#[derive(Debug, Clone, Default)]
pub(crate) struct ResolvedAppearance {
    /// Alpha for the terminal's backdrop, or `None` when it is opaque.
    /// Already gated on the window really being transparent.
    pub(crate) alpha: Option<f32>,
    /// Picture to lay behind the grid, if any resolves to a usable path.
    pub(crate) image: Option<BackgroundImage>,
}

impl Oryxis {
    /// Per-host appearance overrides for a tab label, if that label
    /// still matches a saved host.
    fn host_appearance(
        &self,
        label: &str,
    ) -> Option<&oryxis_core::models::TerminalAppearance> {
        let base = label.trim_end_matches(" (disconnected)");
        self.connections
            .iter()
            .find(|c| c.label == base)
            .and_then(|c| c.terminal_appearance.as_ref())
    }

    /// The effective backdrop for a tab label. Every field resolves
    /// independently: a host that only overrides the picture still
    /// follows the global opacity, which is what `None` means on each
    /// field.
    pub(crate) fn resolve_terminal_appearance(&self, label: &str) -> ResolvedAppearance {
        let host = self.host_appearance(label);

        // Empty path is a real value on the host side: it is how a host
        // says "no picture here" against a global one. `None` inherits.
        let path = match host.and_then(|a| a.image.as_deref()) {
            Some(p) => p.trim(),
            None => self.prefs.terminal_bg_image.trim(),
        };
        let image = (!path.is_empty()).then(|| {
            let fit = host
                .and_then(|a| a.fit.as_deref())
                .unwrap_or(&self.prefs.terminal_bg_fit);
            let dim = host
                .and_then(|a| a.dim)
                .unwrap_or(self.prefs.terminal_bg_dim);
            BackgroundImage {
                handle: iced::advanced::image::Handle::from_path(path),
                fit: BgFit::from_str_or_default(fit),
                dim: f32::from(dim.min(100)) / 100.0,
            }
        });

        // A picture covers the whole pane, so the see-through backdrop
        // underneath it would never be visible. Report opaque instead of
        // asking the renderer to composite a layer nobody can see.
        let alpha = if image.is_some() {
            None
        } else {
            let percent = host
                .and_then(|a| a.opacity)
                .unwrap_or_else(crate::theme::terminal_opacity);
            crate::theme::alpha_for_opacity(percent)
        };

        ResolvedAppearance { alpha, image }
    }

    /// The active tab's backdrop. The window-level layers (the root
    /// container and the terminal container) read this; per-pane draws
    /// read the same value, because appearance is resolved from the
    /// tab's origin host rather than per pane.
    pub(crate) fn active_terminal_appearance(&self) -> ResolvedAppearance {
        if !self.terminal_surface_visible() || self.sftp_surface_visible() {
            return ResolvedAppearance::default();
        }
        match self.cur_active_tab().and_then(|idx| self.tabs.get(idx)) {
            Some(tab) => self.resolve_terminal_appearance(&tab.label),
            None => ResolvedAppearance::default(),
        }
    }
}

/// i18n key for a fit mode's label. Lives here rather than in
/// `oryxis-terminal` so the terminal crate stays free of the app's
/// translation table.
pub(crate) fn bg_fit_label_key(fit: BgFit) -> &'static str {
    match fit {
        BgFit::Cover => "bg_fit_cover",
        BgFit::Contain => "bg_fit_contain",
        BgFit::Stretch => "bg_fit_stretch",
        BgFit::Center => "bg_fit_center",
        BgFit::Tile => "bg_fit_tile",
    }
}

/// The percentages the dim picker offers. Coarser than the opacity
/// steps: the difference between 55% and 60% of veil over a photograph
/// is not something anyone picks deliberately.
pub(crate) const DIM_STEPS: [u8; 11] = [0, 10, 20, 30, 40, 50, 55, 60, 70, 80, 90];

#[cfg(test)]
mod tests {
    use oryxis_core::models::TerminalAppearance;
    use oryxis_terminal::BgFit;

    /// Mirror of the field-by-field fallback in
    /// `resolve_terminal_appearance`, exercised without an `Oryxis`
    /// (which needs a vault and a live iced runtime to build). What is
    /// worth pinning here is the INHERITANCE, not the widget plumbing:
    /// a host overriding one field must keep following the global on
    /// every other, which is the whole contract of `None`.
    fn resolve(
        host: Option<&TerminalAppearance>,
        global: (&str, &str, u8),
    ) -> (String, BgFit, u8) {
        let (g_image, g_fit, g_dim) = global;
        let path = match host.and_then(|a| a.image.as_deref()) {
            Some(p) => p.trim(),
            None => g_image.trim(),
        };
        let fit = host.and_then(|a| a.fit.as_deref()).unwrap_or(g_fit);
        let dim = host.and_then(|a| a.dim).unwrap_or(g_dim);
        (
            path.to_string(),
            BgFit::from_str_or_default(fit),
            dim,
        )
    }

    const GLOBAL: (&str, &str, u8) = ("/pics/global.png", "cover", 55);

    #[test]
    fn a_host_without_overrides_follows_every_global() {
        assert_eq!(
            resolve(None, GLOBAL),
            ("/pics/global.png".to_string(), BgFit::Cover, 55),
        );
    }

    #[test]
    fn one_override_does_not_drag_the_others_along() {
        let only_fit = TerminalAppearance {
            fit: Some("tile".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve(Some(&only_fit), GLOBAL),
            // Picture and fade still come from the global.
            ("/pics/global.png".to_string(), BgFit::Tile, 55),
        );
    }

    #[test]
    fn an_empty_path_is_this_host_opting_out_not_inheriting() {
        // The distinction that makes the picker's third state real:
        // with a global picture set, "none on this host" has to be
        // expressible, and it is NOT the same value as "inherit".
        let none_here = TerminalAppearance {
            image: Some(String::new()),
            ..Default::default()
        };
        assert_eq!(resolve(Some(&none_here), GLOBAL).0, "");
        assert_eq!(resolve(None, GLOBAL).0, "/pics/global.png");
    }

    #[test]
    fn an_unknown_stored_fit_keeps_the_picture() {
        // A payload from a newer build must not cost the user their
        // background; it falls back to the default fit instead.
        let future = TerminalAppearance {
            fit: Some("parallax".into()),
            ..Default::default()
        };
        assert_eq!(resolve(Some(&future), GLOBAL).1, BgFit::Cover);
    }
}
