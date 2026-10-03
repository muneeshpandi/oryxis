//! UI helper widgets: layout. Split out of widgets/mod.rs.

use iced::Widget as _;
use super::*;
/// Build a `Row` from elements written in left-to-right *reading order*,
/// reversing them when the active layout direction is RTL. Use anywhere the
/// physical placement of children should mirror with the layout setting
/// e.g. sidebar vs. content, leading/trailing icon pairs.
///
/// The `iced::widget::row!` macro takes positional children and can't be
/// reversed after construction, so callers that need direction-awareness
/// should switch to this helper instead.
pub fn dir_row<'a, M: 'a>(items: Vec<Element<'a, M>>) -> Row<iced::Element<'a, M>> {
    if crate::i18n::is_rtl_layout() {
        Row::with_children(items.into_iter().rev().collect::<Vec<_>>())
    } else {
        Row::with_children(items)
    }
}

/// Horizontal alignment for content that should hug the *leading* edge
/// `Left` under LTR, `Right` under RTL. Use on `Column::align_x`,
/// `Container::align_x`, or `text(...).align_x(...)` inside `Length::Fill`
/// regions where children would otherwise glue to the physical left edge.
pub fn dir_align_x() -> iced::alignment::Horizontal {
    if crate::i18n::is_rtl_layout() {
        iced::alignment::Horizontal::Right
    } else {
        iced::alignment::Horizontal::Left
    }
}

/// Split the 1000-unit progress track into (filled, remaining) weights,
/// clamping anything out of range (NaN included, which compares false and
/// falls through `clamp` to the low end). A zero on either side means
/// "omit that segment", never "render it weightless": see
/// [`progress_track`] for why the difference matters.
fn track_portions(ratio: f32) -> (u16, u16) {
    let filled = (ratio.clamp(0.0, 1.0) * 1000.0) as u16;
    (filled, 1000u16.saturating_sub(filled))
}

/// A horizontal progress track: a filled leading segment over a muted
/// track, sized by `ratio` (0.0..=1.0) and rounded to a pill.
///
/// Use this rather than hand-rolling a two-`FillPortion` row. A zero
/// portion is NOT a zero-width child in iced: `Limits::resolve_width`
/// matches `Length::FillPortion(_)` with a wildcard (zero included) and
/// answers with the MAXIMUM width, while the flex pass reads a zero fill
/// factor as a static child and hands it the whole remaining space as its
/// limit. A `FillPortion(0)` segment therefore spans the entire track
/// instead of vanishing, which inverts both ends of every bar built that
/// way: empty reads as full, and full reads as empty (issue #107, where
/// 358 KB of a 4.4 GB transfer showed as complete). This helper omits a
/// weightless segment instead of rendering it.
///
/// The fill grows from the *leading* edge, so it mirrors under RTL.
pub fn progress_track<'a, M: 'a>(
    ratio: f32,
    height: f32,
    fill: Color,
    track: Color,
) -> Element<'a, M> {
    let radius = Radius::from(height / 2.0);
    let (filled, remaining) = track_portions(ratio);
    let mut segments: Vec<Element<'a, M>> = Vec::with_capacity(2);
    if filled > 0 {
        segments.push(
            container(Space::new())
                .width(Length::FillPortion(filled))
                .height(Length::Fixed(height))
                .style(move |_| container::Style {
                    background: Some(Background::Color(fill)),
                    border: Border { radius, ..Default::default() },
                    ..Default::default()
                })
                .boxed(),
        );
    }
    if remaining > 0 {
        segments.push(
            container(Space::new())
                .width(Length::FillPortion(remaining))
                .height(Length::Fixed(height))
                .boxed(),
        );
    }
    container(dir_row(segments).width(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .style(move |_| container::Style {
            background: Some(Background::Color(track)),
            border: Border { radius, ..Default::default() },
            ..Default::default()
        })
        .boxed()
}

/// Pick a column count for a card grid given the available content width.
/// Floor-divides slack by `min_card_width + h_gap`, clamped to `>= 1`.
/// Callers compute `available_width` from `window_size` minus the visible
/// chrome (left sidebar, optional right panel, padding).
pub fn card_grid_columns(available_width: f32, min_card_width: f32, h_gap: f32) -> usize {
    if available_width <= 0.0 || min_card_width <= 0.0 {
        return 1;
    }
    let n = ((available_width + h_gap) / (min_card_width + h_gap)).floor() as usize;
    n.max(1)
}

/// Distribute pre-built cards into rows of `cols` cards each. Cards must be
/// built with `Length::Fill` width so the row evenly divides the slack;
/// partial last rows are padded with invisible fillers so the trailing
/// card keeps the same per-card width as the full rows above.
///
/// Honours the active layout direction via `dir_row`, under RTL each
/// row's children are reversed, but the row order (top-to-bottom) stays
/// the same.
pub fn distribute_card_grid<'a, M: 'a>(
    cards: Vec<Element<'a, M>>,
    cols: usize,
    h_gap: f32,
    v_gap: f32,
) -> Element<'a, M> {
    use iced::widget::column;

    if cards.is_empty() {
        return Space::new().boxed();
    }
    let cols = cols.max(1);
    let mut grid_rows: Vec<Element<'a, M>> = Vec::new();
    let mut row_buf: Vec<Element<'a, M>> = Vec::with_capacity(cols);
    let total = cards.len();

    for (i, card) in cards.into_iter().enumerate() {
        row_buf.push(card);
        if row_buf.len() == cols {
            grid_rows.push(dir_row(std::mem::take(&mut row_buf)).spacing(h_gap).boxed());
            if i + 1 < total {
                grid_rows.push(Space::new().height(v_gap).boxed());
            }
        }
    }
    if !row_buf.is_empty() {
        while row_buf.len() < cols {
            row_buf.push(Space::new().width(Length::Fill).boxed());
        }
        grid_rows.push(dir_row(row_buf).spacing(h_gap).boxed());
    }
    column(grid_rows).width(Length::Fill).boxed()
}

#[cfg(test)]
mod tests {
    use super::track_portions;
    use iced::advanced::layout::Limits;
    use iced::{Length, Size};

    /// The iced behavior `progress_track` is designed around: a zero fill
    /// portion is NOT a zero-width child. `resolve_width` matches
    /// `FillPortion(_)` with a wildcard, zero included, and answers with the
    /// maximum width. Rendering a weightless segment therefore paints it
    /// across the entire track, which is what made a 358 KB / 4.4 GB
    /// transfer read as complete (issue #107). If this ever changes
    /// upstream, omitting the weightless side becomes optional rather than
    /// load-bearing.
    #[test]
    fn zero_fill_portion_resolves_to_the_full_width_in_iced() {
        let limits = Limits::new(Size::ZERO, Size::new(1000.0, 4.0));
        assert_eq!(
            limits.resolve_width(Length::FillPortion(0), 0.0),
            1000.0,
            "a zero portion takes the whole track, so it must never be rendered"
        );
    }

    /// A transfer that has barely started weighs the filled side at zero,
    /// which the helper turns into an omitted segment rather than a full bar.
    #[test]
    fn a_barely_started_ratio_has_no_filled_segment() {
        // The reported case: 358.5 KB of 4.4 GB.
        let (filled, remaining) = track_portions(358_500.0 / 4_724_464_025.0);
        assert_eq!(filled, 0, "under 0.1% nothing may be drawn as filled");
        assert_eq!(remaining, 1000);
    }

    /// The symmetric end: at 100% the remainder weighs zero, so it is the
    /// remainder that must be omitted. Rendering it would hand the track to
    /// a transparent segment and read as an empty bar at completion.
    #[test]
    fn a_finished_ratio_has_no_remaining_segment() {
        assert_eq!(track_portions(1.0), (1000, 0));
    }

    /// Ordinary progress keeps both sides, and they always divide the same
    /// 1000-unit track, so the ratio drawn is the ratio computed.
    #[test]
    fn mid_range_splits_the_track_proportionally() {
        for (ratio, want_filled) in [(0.25_f32, 250u16), (0.5, 500), (0.999, 999)] {
            let (filled, remaining) = track_portions(ratio);
            assert_eq!(filled, want_filled, "ratio {ratio}");
            assert_eq!(filled + remaining, 1000, "ratio {ratio}");
        }
    }

    /// Out-of-range input cannot misdraw the bar: a negative, >1 or NaN
    /// ratio is clamped before it reaches the track.
    #[test]
    fn out_of_range_ratios_are_clamped() {
        assert_eq!(track_portions(-1.0), (0, 1000));
        assert_eq!(track_portions(5.0), (1000, 0));
        assert_eq!(track_portions(f32::NAN), (0, 1000), "NaN must not fill the bar");
    }
}
