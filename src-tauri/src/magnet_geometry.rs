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
    pub fn contains(self, x: f64, y: f64) -> bool {
        self.valid() && x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
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

fn nearest_side(rect: Rect, x: f64, y: f64) -> Side {
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

/// Enforce one nearest edge after a moved gesture. The caller selects the
/// frontmost window at the release point before checking its eligibility.
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
    if let Some(target) = target.filter(|t| t.rect.contains(cursor_x, cursor_y)) {
        let r = target.rect;
        let edge = nearest_side(r, cursor_x, cursor_y);
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
        if let Some((value, side, index)) = [outer, inner].into_iter().find(|c| fits(c.0)) {
            if lo <= hi {
                let latch = Latch {
                    id: target.id.wrapping_mul(4).wrapping_add(index),
                    source: Source::Window,
                    side,
                    value,
                };
                if vertical {
                    orb.x = value;
                    orb.y = position.clamp(lo, hi);
                    return Some((orb, Some(latch), None));
                }
                orb.y = value;
                orb.x = position.clamp(lo, hi);
                return Some((orb, None, Some(latch)));
            }
        }
        // An off-screen window edge may have no legal outer or inner placement.
        // In that case the screen edge is the real attachment, never a fake latch.
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
        // A provided target that does not contain the release point is not eligible.
        let (_, x, _) = release_dock(
            r(900., 700., 92., 92.),
            area,
            Some(Target {
                id: 2,
                rect: r(100., 100., 200., 200.),
            }),
            950.,
            746.,
        )
        .unwrap();
        assert_eq!(x.unwrap().source, Source::Screen);
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
