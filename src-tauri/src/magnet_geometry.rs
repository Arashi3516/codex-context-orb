//! Pure geometry in one OS coordinate space: macOS points or Windows physical pixels.
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn right(self) -> f64 {
        self.x + self.width
    }
    pub fn bottom(self) -> f64 {
        self.y + self.height
    }
    pub fn valid(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|n| n.is_finite() && n.abs() < 10_000_000.0)
            && self.width > 0.0
            && self.height > 0.0
    }
    pub fn contains_closed(self, x: f64, y: f64) -> bool {
        self.valid() && x >= self.x && x <= self.right() && y >= self.y && y <= self.bottom()
    }
}

/// Reconstruct the pointerdown position from the viewport-relative anchor.
/// The cursor may already have moved by the time an asynchronous IPC arrives.
pub fn drag_start(rect: Rect, anchor_x: f64, anchor_y: f64) -> Option<(f64, f64)> {
    if !rect.valid()
        || ![anchor_x, anchor_y]
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
    {
        return None;
    }
    Some((
        rect.x + anchor_x * rect.width,
        rect.y + anchor_y * rect.height,
    ))
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Screen {
    pub work_area: Rect,
    pub units_per_logical_pixel: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Screen,
    Window,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub id: u64,
    pub rect: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Latch {
    pub id: u64,
    pub source: Source,
    pub side: Side,
    pub value: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowAttachment {
    pub target_id: u64,
    /// The target's edge, not the touching side of the orb.
    pub edge: Side,
    /// Keep the current side of the target edge until it no longer fits.
    pub exterior: bool,
    /// Orb-center position along the target edge. Corner contacts can extend
    /// slightly beyond 0..1; preserving them avoids a jump on the first follow.
    pub fraction: f64,
}
impl WindowAttachment {
    /// Record an actual placement after a required switch between inside and
    /// outside. The target edge and tangential position remain unchanged.
    pub fn with_placement(mut self, orb: Rect, target: Target) -> Self {
        if target.id == self.target_id && orb.valid() && target.rect.valid() {
            self.exterior = match self.edge {
                Side::Left => orb.x + orb.width / 2.0 < target.rect.x,
                Side::Right => orb.x + orb.width / 2.0 > target.rect.right(),
                Side::Top => orb.y + orb.height / 2.0 < target.rect.y,
                Side::Bottom => orb.y + orb.height / 2.0 > target.rect.bottom(),
            };
        }
        self
    }
}

pub fn nearest_screen(screens: &[Screen], x: f64, y: f64) -> Option<Screen> {
    screens
        .iter()
        .copied()
        .filter(|s| {
            s.work_area.valid()
                && s.units_per_logical_pixel.is_finite()
                && s.units_per_logical_pixel > 0.0
        })
        .min_by(|a, b| {
            let distance = |r: Rect| {
                (x - x.clamp(r.x, r.right())).powi(2) + (y - y.clamp(r.y, r.bottom())).powi(2)
            };
            distance(a.work_area).total_cmp(&distance(b.work_area))
        })
}

pub fn clamp(mut rect: Rect, area: Rect) -> Rect {
    rect.width = rect.width.min(area.width);
    rect.height = rect.height.min(area.height);
    rect.x = rect.x.clamp(area.x, area.right() - rect.width);
    rect.y = rect.y.clamp(area.y, area.bottom() - rect.height);
    rect
}

/// The 92 logical-pixel orb occupies one corner of the expanded widget.
pub fn orb_box(widget: Rect, size: f64, anchor_left: bool, anchor_top: bool) -> Rect {
    let width = size.min(widget.width);
    let height = size.min(widget.height);
    Rect {
        x: if anchor_left {
            widget.x
        } else {
            widget.right() - width
        },
        y: if anchor_top {
            widget.y
        } else {
            widget.bottom() - height
        },
        width,
        height,
    }
}

/// Compose a widget around a stationary orb. None selects the roomier direction;
/// Some keeps the layout stable during a drag, preference change or resize.
pub fn place_widget(
    orb: Rect,
    area: Rect,
    requested_width: f64,
    requested_height: f64,
    anchor: Option<(bool, bool)>,
) -> (Rect, bool, bool) {
    let orb = clamp(orb, area);
    let (left, top) = anchor.unwrap_or((
        area.right() - orb.x >= orb.right() - area.x,
        area.bottom() - orb.y >= orb.bottom() - area.y,
    ));
    let available_width = if left {
        area.right() - orb.x
    } else {
        orb.right() - area.x
    };
    let available_height = if top {
        area.bottom() - orb.y
    } else {
        orb.bottom() - area.y
    };
    let width = requested_width.max(orb.width).min(available_width);
    let height = requested_height.max(orb.height).min(available_height);
    (
        Rect {
            x: if left { orb.x } else { orb.right() - width },
            y: if top { orb.y } else { orb.bottom() - height },
            width,
            height,
        },
        left,
        top,
    )
}

pub(crate) fn nearest_side(rect: Rect, x: f64, y: f64) -> Side {
    let dx = x - x.clamp(rect.x, rect.right());
    let dy = y - y.clamp(rect.y, rect.bottom());
    // The explicit index gives equal-distance corners a stable tie break.
    [
        (Side::Left, (x - rect.x).powi(2) + dy.powi(2)),
        (Side::Right, (x - rect.right()).powi(2) + dy.powi(2)),
        (Side::Top, (y - rect.y).powi(2) + dx.powi(2)),
        (Side::Bottom, (y - rect.bottom()).powi(2) + dx.powi(2)),
    ]
    .into_iter()
    .enumerate()
    .min_by(|a, b| a.1 .1.total_cmp(&b.1 .1).then(a.0.cmp(&b.0)))
    .unwrap()
    .1
     .0
}

fn dock_to_edge(
    mut orb: Rect,
    area: Rect,
    target: Target,
    edge: Side,
    exterior: bool,
) -> Option<(Rect, Option<Latch>, Option<Latch>)> {
    if !orb.valid() || !area.valid() || !target.rect.valid() {
        return None;
    }
    orb = clamp(orb, area);
    let r = target.rect;
    let vertical = matches!(edge, Side::Left | Side::Right);
    let (outer, inner) = match edge {
        Side::Left => ((r.x - orb.width, Side::Right, 2), (r.x, Side::Left, 0)),
        Side::Right => (
            (r.right(), Side::Left, 3),
            (r.right() - orb.width, Side::Right, 1),
        ),
        Side::Top => ((r.y - orb.height, Side::Bottom, 2), (r.y, Side::Top, 0)),
        Side::Bottom => (
            (r.bottom(), Side::Top, 3),
            (r.bottom() - orb.height, Side::Bottom, 1),
        ),
    };
    let fits = |value| {
        if vertical {
            value >= area.x && value + orb.width <= area.right()
        } else {
            value >= area.y && value + orb.height <= area.bottom()
        }
    };
    // Keep a positive contact segment, including an anchor at an exact corner.
    let (position, length, minimum, maximum, edge_min, edge_max) = if vertical {
        (orb.y, orb.height, area.y, area.bottom(), r.y, r.bottom())
    } else {
        (orb.x, orb.width, area.x, area.right(), r.x, r.right())
    };
    let overlap = (edge_max - edge_min).min(length).min(1.0);
    let lo = minimum.max(edge_min - length + overlap);
    let hi = (maximum - length).min(edge_max - overlap);
    let candidates = if exterior {
        [outer, inner]
    } else {
        [inner, outer]
    };
    let (value, side, index) = candidates.into_iter().find(|c| fits(c.0))?;
    if lo > hi {
        return None;
    }
    let latch = Latch {
        id: target.id.wrapping_mul(4).wrapping_add(index),
        source: Source::Window,
        side,
        value,
    };
    if vertical {
        orb.x = value;
        orb.y = position.clamp(lo, hi);
        Some((orb, Some(latch), None))
    } else {
        orb.y = value;
        orb.x = position.clamp(lo, hi);
        Some((orb, None, Some(latch)))
    }
}

/// Bind the actual released orb to the selected target edge.
pub fn from_release(orb: Rect, target: Target, x: f64, y: f64) -> Option<WindowAttachment> {
    if !orb.valid() || !target.rect.valid() || !x.is_finite() || !y.is_finite() {
        return None;
    }
    let edge = nearest_side(target.rect, x, y);
    let fraction = match edge {
        Side::Left | Side::Right => (orb.y + orb.height / 2.0 - target.rect.y) / target.rect.height,
        Side::Top | Side::Bottom => (orb.x + orb.width / 2.0 - target.rect.x) / target.rect.width,
    };
    Some(
        WindowAttachment {
            target_id: target.id,
            edge,
            exterior: true,
            fraction,
        }
        .with_placement(orb, target),
    )
}

/// Follow one target without changing its bound edge or chosen side unless that
/// placement cannot fit. A missing/off-screen edge returns None.
pub fn follow_orb(
    attachment: WindowAttachment,
    target: Target,
    orb_size: f64,
    area: Rect,
) -> Option<(Rect, Option<Latch>, Option<Latch>)> {
    if target.id != attachment.target_id
        || !target.rect.valid()
        || !area.valid()
        || !attachment.fraction.is_finite()
        || !orb_size.is_finite()
        || orb_size <= 0.0
    {
        return None;
    }
    let mut orb = Rect {
        x: target.rect.x,
        y: target.rect.y,
        width: orb_size.min(area.width),
        height: orb_size.min(area.height),
    };
    match attachment.edge {
        Side::Left | Side::Right => {
            orb.y = target.rect.y + attachment.fraction * target.rect.height - orb.height / 2.0;
        }
        Side::Top | Side::Bottom => {
            orb.x = target.rect.x + attachment.fraction * target.rect.width - orb.width / 2.0;
        }
    }
    dock_to_edge(orb, area, target, attachment.edge, attachment.exterior)
}

/// Enforce one nearest edge after a moved gesture. The caller has already
/// selected a visible, eligible target using the pointer or the orb footprint.
pub fn release_dock(
    raw: Rect,
    area: Rect,
    target: Option<Target>,
    cursor_x: f64,
    cursor_y: f64,
) -> Option<(Rect, Option<Latch>, Option<Latch>)> {
    if !raw.valid() || !area.valid() || !cursor_x.is_finite() || !cursor_y.is_finite() {
        return None;
    }
    let mut orb = clamp(raw, area);
    if let Some(target) = target.filter(|target| target.rect.valid()) {
        if let Some(docked) = dock_to_edge(
            orb,
            area,
            target,
            nearest_side(target.rect, cursor_x, cursor_y),
            true,
        ) {
            return Some(docked);
        }
        // An off-screen edge with no legal contact falls back to the screen.
    }
    let side = nearest_side(area, cursor_x, cursor_y);
    let (id, value) = match side {
        Side::Left => (0, area.x),
        Side::Right => (1, area.right() - orb.width),
        Side::Top => (2, area.y),
        Side::Bottom => (3, area.bottom() - orb.height),
    };
    let latch = Latch {
        id,
        source: Source::Screen,
        side,
        value,
    };
    if matches!(side, Side::Left | Side::Right) {
        orb.x = value;
        Some((orb, Some(latch), None))
    } else {
        orb.y = value;
        Some((orb, None, Some(latch)))
    }
}

/// Observe actual contacts without moving the orb or carrying an old latch.
pub fn attachments(raw: Rect, area: Rect, targets: &[Target]) -> (Option<Latch>, Option<Latch>) {
    if !raw.valid() || !area.valid() {
        return (None, None);
    }
    let mut xs = Vec::with_capacity(targets.len() * 4 + 2);
    let mut ys = Vec::with_capacity(targets.len() * 4 + 2);
    xs.extend([
        Latch {
            id: 0,
            source: Source::Screen,
            side: Side::Left,
            value: area.x,
        },
        Latch {
            id: 1,
            source: Source::Screen,
            side: Side::Right,
            value: area.right() - raw.width,
        },
    ]);
    ys.extend([
        Latch {
            id: 2,
            source: Source::Screen,
            side: Side::Top,
            value: area.y,
        },
        Latch {
            id: 3,
            source: Source::Screen,
            side: Side::Bottom,
            value: area.bottom() - raw.height,
        },
    ]);
    for target in targets.iter().filter(|t| t.rect.valid()) {
        let r = target.rect;
        // Require an overlapping edge segment; a distant corner is not a magnet.
        if raw.bottom().min(r.bottom()) - raw.y.max(r.y) > 0.0 {
            for (i, side, value) in [
                (0, Side::Left, r.x),
                (1, Side::Right, r.right() - raw.width),
                (2, Side::Right, r.x - raw.width),
                (3, Side::Left, r.right()),
            ] {
                if value >= area.x && value + raw.width <= area.right() {
                    xs.push(Latch {
                        id: target.id.wrapping_mul(4).wrapping_add(i),
                        source: Source::Window,
                        side,
                        value,
                    });
                }
            }
        }
        if raw.right().min(r.right()) - raw.x.max(r.x) > 0.0 {
            for (i, side, value) in [
                (0, Side::Top, r.y),
                (1, Side::Bottom, r.bottom() - raw.height),
                (2, Side::Bottom, r.y - raw.height),
                (3, Side::Top, r.bottom()),
            ] {
                if value >= area.y && value + raw.height <= area.bottom() {
                    ys.push(Latch {
                        id: target.id.wrapping_mul(4).wrapping_add(i),
                        source: Source::Window,
                        side,
                        value,
                    });
                }
            }
        }
    }
    let contact = |position: f64, candidates: Vec<Latch>| {
        candidates
            .into_iter()
            .filter(|c| (c.value - position).abs() < 0.1)
            .min_by(|a, b| {
                (a.value - position)
                    .abs()
                    .total_cmp(&(b.value - position).abs())
            })
    };
    (contact(raw.x, xs), contact(raw.y, ys))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect {
            x,
            y,
            width: w,
            height: h,
        }
    }
    #[test]
    fn negative_monitor_and_gap_choose_nearest() {
        let screens = [
            Screen {
                work_area: r(-1920., -200., 1920., 1080.),
                units_per_logical_pixel: 1.,
            },
            Screen {
                work_area: r(0., 0., 2560., 1400.),
                units_per_logical_pixel: 2.,
            },
        ];
        assert_eq!(
            nearest_screen(&screens, -1200., 30.).unwrap().work_area.x,
            -1920.
        );
        assert_eq!(
            nearest_screen(&screens, 200., -100.)
                .unwrap()
                .units_per_logical_pixel,
            2.
        );
    }
    #[test]
    fn delayed_begin_preserves_pointerdown_anchor_and_short_drag_distance() {
        let old = r(100., 200., 92., 92.);
        let (start_x, start_y) = drag_start(old, 0.5, 0.5).unwrap();
        let released_cursor = (124., 246.);
        assert_eq!((start_x, start_y), (146., 246.));
        assert_eq!(start_x - released_cursor.0, 22.);
        let moved = r(
            released_cursor.0 - old.width * 0.5,
            released_cursor.1 - old.height * 0.5,
            old.width,
            old.height,
        );
        assert_eq!(moved, r(78., 200., 92., 92.));
    }
    #[test]
    fn pointer_anchor_is_bounded_and_works_on_negative_high_dpi_screen() {
        let old = r(-1920., -100., 184., 184.);
        assert_eq!(drag_start(old, 0.5, 0.5), Some((-1828., -8.)));
        assert_eq!(drag_start(old, 0., 1.), Some((-1920., 84.)));
        for invalid in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
            assert!(drag_start(old, invalid, 0.5).is_none());
            assert!(drag_start(old, 0.5, invalid).is_none());
        }
        assert!(drag_start(r(0., 0., 0., 92.), 0.5, 0.5).is_none());
    }
    #[test]
    fn windows_require_overlapping_edges_and_disappear_cleanly() {
        let a = r(-1000., 0., 2000., 800.);
        let targets = [Target {
            id: 8,
            rect: r(200., 100., 300., 300.),
        }];
        let (x, _) = attachments(r(108., 150., 92., 92.), a, &targets);
        assert_eq!(x.unwrap().source, Source::Window);
        let (far, _) = attachments(r(108., 600., 92., 92.), a, &targets);
        assert!(far.is_none());
        let (none, _) = attachments(r(108., 150., 92., 92.), a, &[]);
        assert!(none.is_none());
    }
    #[test]
    fn offscreen_restoration_shrinks_oversize_and_never_overflows() {
        assert_eq!(
            clamp(r(10000., -9000., 2000., 3000.), r(-800., 30., 800., 500.)),
            r(-800., 30., 800., 500.)
        );
        assert!(nearest_screen(&[], 0., 0.).is_none());
    }
    #[test]
    fn physical_pixel_orb_scales_without_scaling_negative_global_origins() {
        let a = r(-2560., 0., 2560., 1440.);
        let (orb, x, _) = release_dock(r(-2300., 500., 184., 184.), a, None, -2208., 592.).unwrap();
        assert_eq!(orb.x, -2560.);
        assert_eq!(x.unwrap().source, Source::Screen);
        let (widget, left, top) = place_widget(orb, a, 764., 1380., None);
        assert_eq!(orb_box(widget, 184., left, top), orb);
        assert_eq!(clamp(widget, a), widget);
    }

    #[test]
    fn release_uses_cursor_distance_without_a_proximity_threshold() {
        let area = r(0., 0., 1000., 800.);
        for raw in [r(330., 300., 92., 92.), r(100., 100., 184., 184.)] {
            let (docked, x, y) = release_dock(raw, area, None, 400., 300.).unwrap();
            assert_eq!(docked.y, 0.);
            assert!(x.is_none());
            assert_eq!(y.unwrap().side, Side::Top);
        }
        let (_, x, y) = release_dock(r(200., 200., 92., 92.), area, None, 200., 200.).unwrap();
        assert_eq!(x.unwrap().side, Side::Left);
        assert!(y.is_none());
        assert!(release_dock(r(0., 0., 92., 92.), area, None, f64::NAN, 0.).is_none());
    }

    #[test]
    fn window_release_prefers_outer_contact_on_each_nearest_edge() {
        let area = r(0., 0., 1000., 800.);
        let target = Target {
            id: 9,
            rect: r(200., 200., 400., 300.),
        };
        for (cursor, expected, side) in [
            ((210., 350.), r(108., 304., 92., 92.), Side::Right),
            ((590., 350.), r(600., 304., 92., 92.), Side::Left),
            ((400., 210.), r(354., 108., 92., 92.), Side::Bottom),
            ((400., 490.), r(354., 500., 92., 92.), Side::Top),
        ] {
            let raw = r(cursor.0 - 46., cursor.1 - 46., 92., 92.);
            let (docked, x, y) = release_dock(raw, area, Some(target), cursor.0, cursor.1).unwrap();
            assert_eq!(docked, expected);
            let latch = x.or(y).unwrap();
            assert_eq!(latch.source, Source::Window);
            assert_eq!(latch.side, side);
            let (actual_x, actual_y) = attachments(docked, area, &[target]);
            assert_eq!(actual_x.or(actual_y).unwrap(), latch);
        }
    }

    #[test]
    fn window_release_uses_inner_contact_when_outer_does_not_fit() {
        let area = r(0., 0., 1000., 800.);
        let target = Target {
            id: 1,
            rect: r(50., 100., 500., 500.),
        };
        let (docked, x, _) =
            release_dock(r(10., 254., 92., 92.), area, Some(target), 56., 300.).unwrap();
        assert_eq!(docked.x, 50.);
        assert_eq!(x.unwrap().side, Side::Left);
        assert_eq!(x.unwrap().source, Source::Window);
    }

    #[test]
    fn unavailable_cross_screen_window_edge_falls_back_to_screen() {
        let area = r(0., 0., 1000., 800.);
        let target = Target {
            id: 1,
            rect: r(-200., 100., 1400., 600.),
        };
        let (docked, x, _) =
            release_dock(r(-41., 354., 92., 92.), area, Some(target), 5., 400.).unwrap();
        assert_eq!(docked.x, 0.);
        assert_eq!(x.unwrap().source, Source::Screen);
    }

    #[test]
    fn selected_window_accepts_all_four_boundaries_and_touching_orb_footprints() {
        let area = r(0., 0., 1000., 800.);
        let target = Target {
            id: 9,
            rect: r(200., 200., 400., 300.),
        };
        for offset in [0., 1., 46.] {
            for (x, y, edge) in [
                (200. - offset, 350., Side::Left),
                (600. + offset, 350., Side::Right),
                (400., 200. - offset, Side::Top),
                (400., 500. + offset, Side::Bottom),
            ] {
                assert_eq!(target.rect.contains_closed(x, y), offset == 0.);
                let (orb, lx, ly) =
                    release_dock(r(x - 46., y - 46., 92., 92.), area, Some(target), x, y).unwrap();
                assert_eq!(lx.or(ly).unwrap().source, Source::Window);
                let attachment = from_release(orb, target, x, y).unwrap();
                assert_eq!(attachment.edge, edge);
                assert_eq!(
                    follow_orb(attachment, target, 92., area).unwrap(),
                    (orb, lx, ly)
                );
            }
        }
    }

    #[test]
    fn following_translates_all_edges_and_preserves_fraction_on_resize() {
        let area = r(0., 0., 1400., 1000.);
        let target = Target {
            id: 9,
            rect: r(250., 250., 400., 300.),
        };
        for (x, y, edge) in [
            (250., 325., Side::Left),
            (650., 325., Side::Right),
            (350., 250., Side::Top),
            (350., 550., Side::Bottom),
        ] {
            let (orb, _, _) =
                release_dock(r(x - 46., y - 46., 92., 92.), area, Some(target), x, y).unwrap();
            let attachment = from_release(orb, target, x, y).unwrap();
            assert_eq!(attachment.fraction, 0.25);
            let moved = Target {
                rect: r(350., 300., 400., 300.),
                ..target
            };
            let (followed, lx, ly) = follow_orb(attachment, moved, 92., area).unwrap();
            assert_eq!((followed.x - orb.x, followed.y - orb.y), (100., 50.));
            assert_eq!(attachments(followed, area, &[moved]), (lx, ly));
            let resized = Target {
                rect: r(350., 300., 600., 400.),
                ..target
            };
            let (followed, _, _) = follow_orb(attachment, resized, 92., area).unwrap();
            let center = if matches!(edge, Side::Left | Side::Right) {
                followed.y + 46.
            } else {
                followed.x + 46.
            };
            let expected = if matches!(edge, Side::Left | Side::Right) {
                400.
            } else {
                500.
            };
            assert_eq!(center, expected);
            assert_eq!(clamp(followed, area), followed);
        }
    }

    #[test]
    fn follow_keeps_target_edge_when_switching_inside_or_losing_the_edge() {
        let area = r(0., 0., 1000., 800.);
        let target = Target {
            id: 7,
            rect: r(200., 200., 400., 300.),
        };
        for (edge, near, hidden) in [
            (
                Side::Left,
                r(20., 200., 400., 300.),
                r(-20., 200., 400., 300.),
            ),
            (
                Side::Right,
                r(580., 200., 400., 300.),
                r(620., 200., 400., 300.),
            ),
            (
                Side::Top,
                r(200., 20., 400., 300.),
                r(200., -20., 400., 300.),
            ),
            (
                Side::Bottom,
                r(200., 480., 400., 300.),
                r(200., 520., 400., 300.),
            ),
        ] {
            let attachment = WindowAttachment {
                target_id: 7,
                edge,
                exterior: true,
                fraction: 0.5,
            };
            let current = Target {
                rect: near,
                ..target
            };
            let (inside, lx, ly) = follow_orb(attachment, current, 92., area).unwrap();
            assert_eq!(lx.or(ly).unwrap().side, edge);
            assert_eq!(attachments(inside, area, &[current]), (lx, ly));
            assert_eq!(clamp(inside, area), inside);
            assert!(follow_orb(
                attachment,
                Target {
                    rect: hidden,
                    ..target
                },
                92.,
                area
            )
            .is_none());
            assert!(follow_orb(attachment, Target { id: 8, ..target }, 92., area).is_none());
        }
    }

    #[test]
    fn an_inside_attachment_stays_inside_when_the_window_moves_away_from_screen() {
        let area = r(0., 0., 1000., 800.);
        let moved = Target {
            id: 7,
            rect: r(300., 250., 400., 300.),
        };
        for (near, x, y, edge) in [
            (r(20., 250., 400., 300.), 20., 400., Side::Left),
            (r(580., 250., 400., 300.), 980., 400., Side::Right),
            (r(300., 20., 400., 300.), 500., 20., Side::Top),
            (r(300., 480., 400., 300.), 500., 780., Side::Bottom),
        ] {
            let target = Target { id: 7, rect: near };
            let (orb, _, _) =
                release_dock(r(x - 46., y - 46., 92., 92.), area, Some(target), x, y).unwrap();
            let attachment = from_release(orb, target, x, y).unwrap();
            assert_eq!(attachment.edge, edge);
            assert!(!attachment.exterior);
            let (followed, lx, ly) = follow_orb(attachment, moved, 92., area).unwrap();
            assert_eq!(lx.or(ly).unwrap().side, edge);
            assert_eq!(
                (followed.x - orb.x, followed.y - orb.y),
                (moved.rect.x - near.x, moved.rect.y - near.y)
            );
            assert!(!attachment.with_placement(followed, moved).exterior);
            assert_eq!(attachments(followed, area, &[moved]), (lx, ly));
        }
    }

    #[test]
    fn an_outside_attachment_switches_only_when_needed_and_keeps_the_new_side() {
        let area = r(0., 0., 1000., 800.);
        let original = Target {
            id: 7,
            rect: r(300., 250., 400., 300.),
        };
        for (x, y, edge, threshold, closer) in [
            (
                300.,
                400.,
                Side::Left,
                r(92., 250., 400., 300.),
                r(91., 250., 400., 300.),
            ),
            (
                700.,
                400.,
                Side::Right,
                r(508., 250., 400., 300.),
                r(509., 250., 400., 300.),
            ),
            (
                500.,
                250.,
                Side::Top,
                r(300., 92., 400., 300.),
                r(300., 91., 400., 300.),
            ),
            (
                500.,
                550.,
                Side::Bottom,
                r(300., 408., 400., 300.),
                r(300., 409., 400., 300.),
            ),
        ] {
            let (orb, _, _) =
                release_dock(r(x - 46., y - 46., 92., 92.), area, Some(original), x, y).unwrap();
            let attachment = from_release(orb, original, x, y).unwrap();
            assert_eq!(attachment.edge, edge);
            assert!(attachment.exterior);
            let last_outside = Target {
                rect: threshold,
                ..original
            };
            let (orb, _, _) = follow_orb(attachment, last_outside, 92., area).unwrap();
            assert!(attachment.with_placement(orb, last_outside).exterior);
            let near = Target {
                rect: closer,
                ..original
            };
            let (inside, lx, ly) = follow_orb(attachment, near, 92., area).unwrap();
            assert_eq!(lx.or(ly).unwrap().side, edge);
            let updated = attachment.with_placement(inside, near);
            assert!(!updated.exterior);
            assert_eq!(
                (updated.edge, updated.fraction),
                (attachment.edge, attachment.fraction)
            );
            let (returned, lx, ly) = follow_orb(updated, original, 92., area).unwrap();
            assert_eq!(lx.or(ly).unwrap().side, edge);
            assert!(!updated.with_placement(returned, original).exterior);
            assert_eq!(attachments(returned, area, &[original]), (lx, ly));
            assert_eq!(
                attachment.with_placement(inside, Target { id: 8, ..near }),
                attachment
            );
        }
    }

    #[test]
    fn following_preserves_corner_contact_and_handles_negative_scaled_displays() {
        let area = r(-2560., -200., 2560., 1400.);
        let target = Target {
            id: 1,
            rect: r(-2000., 200., 800., 600.),
        };
        for (x, y) in [
            (-2000., 200.),
            (-1200., 200.),
            (-2000., 800.),
            (-1200., 800.),
        ] {
            let (orb, lx, ly) =
                release_dock(r(x - 184., y - 184., 184., 184.), area, Some(target), x, y).unwrap();
            let attachment = from_release(orb, target, x, y).unwrap();
            assert_eq!(
                follow_orb(attachment, target, 184., area).unwrap(),
                (orb, lx, ly)
            );
        }
        let invalid = WindowAttachment {
            target_id: 1,
            edge: Side::Left,
            exterior: true,
            fraction: f64::NAN,
        };
        assert!(follow_orb(invalid, target, 184., area).is_none());
        assert!(from_release(r(0., 0., 92., 92.), target, f64::NAN, 0.).is_none());
    }

    #[test]
    fn small_windows_and_exact_corners_keep_a_real_contact_segment() {
        let area = r(0., 0., 1000., 800.);
        for target in [
            Target {
                id: 1,
                rect: r(200., 200., 30., 30.),
            },
            Target {
                id: 2,
                rect: r(200., 200., 400., 300.),
            },
        ] {
            let (docked, x, _) =
                release_dock(r(108., 108., 92., 92.), area, Some(target), 200., 200.).unwrap();
            assert_eq!(docked.x, 108.);
            assert!(docked.bottom() > target.rect.y);
            assert_eq!(attachments(docked, area, &[target]).0, x);
        }
    }

    #[test]
    fn composing_and_resizing_preserve_the_orb_at_all_four_screen_corners() {
        let area = r(-1000., -200., 1000., 800.);
        for left in [false, true] {
            for top in [false, true] {
                let orb = r(
                    if left { area.x } else { area.right() - 92. },
                    if top { area.y } else { area.bottom() - 92. },
                    92.,
                    92.,
                );
                let (widget, actual_left, actual_top) = place_widget(orb, area, 382., 690., None);
                assert_eq!((actual_left, actual_top), (left, top));
                assert_eq!(orb_box(widget, 92., left, top), orb);
                assert_eq!(clamp(widget, area), widget);
                let (collapsed, _, _) = place_widget(orb, area, 92., 92., Some((left, top)));
                assert_eq!(collapsed, orb);
            }
        }
    }

    #[test]
    fn expanded_release_docks_the_orb_instead_of_the_panel_boundary() {
        let area = r(0., 0., 1000., 800.);
        let raw_orb = r(454., 84., 92., 92.);
        let (orb, _, y) = release_dock(raw_orb, area, None, 500., 130.).unwrap();
        let (widget, left, top) = place_widget(orb, area, 382., 690., None);
        assert!(top);
        assert_eq!(orb_box(widget, 92., left, top).y, 0.);
        assert_eq!(orb_box(widget, 92., left, top), orb);
        assert_eq!(y.unwrap().side, Side::Top);
        assert_eq!(clamp(widget, area), widget);
    }

    #[test]
    fn short_screens_and_fixed_drag_layout_fit_without_moving_the_orb() {
        let area = r(-1200., -40., 700., 400.);
        let orb = r(-1200., -40., 92., 92.);
        let (widget, left, top) = place_widget(orb, area, 382., 690., None);
        assert_eq!(widget.height, 400.);
        assert_eq!(orb_box(widget, 92., left, top), orb);
        let (fixed, left, top) = place_widget(orb, area, 382., 690., Some((false, false)));
        assert_eq!((left, top), (false, false));
        assert_eq!(fixed, orb);
    }

    #[test]
    fn attachment_measurement_does_not_keep_moved_or_missing_window_latches() {
        let area = r(0., 0., 1000., 800.);
        let orb = r(408., 300., 92., 92.);
        let target = Target {
            id: 3,
            rect: r(500., 100., 300., 500.),
        };
        assert_eq!(
            attachments(orb, area, &[target]).0.unwrap().source,
            Source::Window
        );
        let (widget, left, top) = place_widget(orb, area, 382., 690., Some((true, true)));
        let orb = orb_box(widget, 92., left, top);
        assert_eq!(attachments(orb, area, &[]), (None, None));
        let moved = Target {
            rect: r(600., 100., 300., 500.),
            ..target
        };
        assert_eq!(attachments(orb, area, &[moved]), (None, None));
    }
}
