use crate::{
    magnet_geometry::{
        self as geometry, Latch, Rect, Screen, Side, Source, Target, WindowAttachment,
    },
    magnet_platform::{self as platform, OtherWindow},
};
use serde::{Deserialize, Serialize};
use std::{
    sync::{mpsc, Mutex},
    time::{Duration, Instant},
};
use tauri::{Manager, WebviewWindow};

const ACTIVE_TICK_INTERVAL: Duration = Duration::from_nanos(16_666_667);
const MOVING_TARGET_INTERVAL: Duration = Duration::from_nanos(8_333_333);
const MOVING_TARGET_HOLD: Duration = Duration::from_millis(200);

struct TickSchedule {
    slot: Instant,
    interval: Option<Duration>,
}
impl TickSchedule {
    fn next_deadline(
        &mut self,
        posted: Instant,
        completed: Instant,
        interval: Duration,
    ) -> Instant {
        if self.interval != Some(interval) {
            self.slot = posted;
            self.interval = Some(interval);
        }
        let next = self.slot + interval;
        self.slot = if completed <= next {
            next
        } else {
            // Keep the frame grid, skipping expired slots in constant time.
            // Both supported intervals are below one second.
            let remainder = completed.duration_since(self.slot).as_nanos() % interval.as_nanos();
            if remainder == 0 {
                completed
            } else {
                completed + interval - Duration::from_nanos(remainder as u64)
            }
        };
        self.slot
    }
    fn wake(&mut self, now: Instant) {
        self.slot = now;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowMode {
    Codex,
    Off,
    All,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    window_mode: WindowMode,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            window_mode: WindowMode::Codex,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    drag: &'static str,
    screen_edges: &'static str,
    window_edges: &'static str,
    coordinate_space: &'static str,
    codex_gui: &'static str,
    reason: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MagnetState {
    revision: u64,
    preferences: Preferences,
    capabilities: Capabilities,
    dragging: bool,
    last_drag_moved: bool,
    snapped_x: Option<Source>,
    snapped_y: Option<Source>,
    snap_side_x: Option<Side>,
    snap_side_y: Option<Side>,
    layout_anchor_left: bool,
    layout_anchor_top: bool,
    window_targets: usize,
    geometry: Option<Rect>,
    monitor: Option<Screen>,
    last_error: Option<String>,
}

struct Drag {
    start_frame: Rect,
    start_x: f64,
    start_y: f64,
    anchor_x: f64,
    anchor_y: f64,
    logical_width: f64,
    logical_height: f64,
    started: Instant,
    last_cursor: (f64, f64),
    release_frame: Option<Rect>,
    resume_motion: Option<DockMotion>,
}

#[derive(Clone, Copy)]
struct DockMotion {
    from: Rect,
    to: Rect,
    screen: Screen,
    x: Option<Latch>,
    y: Option<Latch>,
    started: Instant,
    duration: Duration,
}
impl DockMotion {
    fn frame(&self, elapsed: Duration) -> (Rect, bool) {
        let t = (elapsed.as_secs_f64() / self.duration.as_secs_f64()).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        (
            Rect {
                x: self.from.x + (self.to.x - self.from.x) * eased,
                y: self.from.y + (self.to.y - self.from.y) * eased,
                ..self.to
            },
            t >= 1.0,
        )
    }
}
impl Drag {
    fn widget_at(&self, screen: Screen, cursor_x: f64, cursor_y: f64) -> Rect {
        let width = platform::coordinate(self.logical_width * screen.units_per_logical_pixel);
        let height = platform::coordinate(self.logical_height * screen.units_per_logical_pixel);
        Rect {
            x: cursor_x - self.anchor_x * width,
            y: cursor_y - self.anchor_y * height,
            width,
            height,
        }
    }
    fn release_point(&self, current_frame: Rect, anchor: Option<(f64, f64)>) -> (f64, f64) {
        let frame = self.release_frame.unwrap_or(current_frame);
        anchor.map_or(self.last_cursor, |(x, y)| {
            (frame.x + x * frame.width, frame.y + y * frame.height)
        })
    }
}

pub struct Engine {
    view: MagnetState,
    drag: Option<Drag>,
    x: Option<Latch>,
    y: Option<Latch>,
    windows: Vec<OtherWindow>,
    wake: Option<mpsc::SyncSender<()>>,
    requested_width: f64,
    requested_height: f64,
    configured: bool,
    windows_observed_at: Option<Instant>,
    attachment: Option<WindowAttachment>,
    attachment_owner: Option<u32>,
    motion: Option<DockMotion>,
    reduced_motion: bool,
    last_placed: Option<Rect>,
    last_follow_sample: Option<(u64, u32, Rect)>,
    fast_follow_until: Option<Instant>,
}
pub type Magnet = Mutex<Engine>;

impl Default for Engine {
    fn default() -> Self {
        let supported = platform::COORDINATES != "unsupported";
        Self {
            view: MagnetState {
                revision: 0,
                preferences: Preferences::default(),
                capabilities: Capabilities {
                    drag: if supported {
                        "unavailable"
                    } else {
                        "unsupported"
                    },
                    screen_edges: if supported {
                        "unavailable"
                    } else {
                        "unsupported"
                    },
                    window_edges: if supported {
                        "unavailable"
                    } else {
                        "unsupported"
                    },
                    coordinate_space: platform::COORDINATES,
                    codex_gui: platform::CODEX_GUI,
                    reason: Some("Window geometry has not been checked".into()),
                },
                dragging: false,
                last_drag_moved: false,
                snapped_x: None,
                snapped_y: None,
                snap_side_x: None,
                snap_side_y: None,
                layout_anchor_left: false,
                layout_anchor_top: false,
                window_targets: 0,
                geometry: None,
                monitor: None,
                last_error: None,
            },
            drag: None,
            x: None,
            y: None,
            windows: Vec::new(),
            wake: None,
            requested_width: 92.0,
            requested_height: 92.0,
            configured: false,
            windows_observed_at: None,
            attachment: None,
            attachment_owner: None,
            motion: None,
            reduced_motion: false,
            last_placed: None,
            last_follow_sample: None,
            fast_follow_until: None,
        }
    }
}

impl Engine {
    fn require_configured(&self) -> Result<(), String> {
        if self.configured {
            Ok(())
        } else {
            Err(self
                .view
                .last_error
                .clone()
                .unwrap_or_else(|| "Native window configuration is unavailable".into()))
        }
    }
    fn wake(&self) {
        if let Some(wake) = &self.wake {
            let _ = wake.try_send(());
        }
    }
    fn attachments(&mut self) {
        self.view.snapped_x = self.x.map(|x| x.source);
        self.view.snapped_y = self.y.map(|y| y.source);
        self.view.snap_side_x = self.x.map(|x| x.side);
        self.view.snap_side_y = self.y.map(|y| y.side);
    }
    fn eligible(&self, w: &OtherWindow) -> bool {
        w.dockable
            && match self.view.preferences.window_mode {
                WindowMode::Off => false,
                WindowMode::All => true,
                WindowMode::Codex => w.codex,
            }
    }
    fn release_target(&self, cursor_x: f64, cursor_y: f64, orb: Rect) -> Option<Target> {
        let target = |w: &OtherWindow| Target {
            id: w.id,
            rect: w.rect,
        };
        let mut pointer_blocked = false;
        for (index, w) in self.windows.iter().enumerate() {
            // Evaluate a foreground window's pointer and footprint together;
            // the pointer can be over a background window beside its border.
            if w.rect.contains_closed(cursor_x, cursor_y) {
                if !pointer_blocked && self.eligible(w) {
                    return Some(target(w));
                }
                // A front window blocks only later pointer hits. A separate,
                // visible target edge can still be touched by the orb footprint.
                pointer_blocked = true;
            }
            if !self.eligible(w) || !orb.valid() || !w.rect.valid() {
                continue;
            }
            let front = &self.windows[..index];
            let visible_segment = |vertical: bool, fixed: f64, lo: f64, hi: f64| {
                let (minimum, maximum) = if vertical {
                    (orb.x, orb.right())
                } else {
                    (orb.y, orb.bottom())
                };
                if fixed < minimum || fixed > maximum || lo > hi {
                    return false;
                }
                if lo == hi {
                    let point = if vertical { (fixed, lo) } else { (lo, fixed) };
                    return !front
                        .iter()
                        .any(|w| w.rect.contains_closed(point.0, point.1));
                }
                // A popup can cover the nearest projection while leaving another
                // part of the touched edge visible. Subtract all foreground spans.
                let mut covered: Vec<(f64, f64)> = front
                    .iter()
                    .filter_map(|w| {
                        let r = w.rect;
                        if !r.valid() {
                            return None;
                        }
                        let (near, far, start, end) = if vertical {
                            (r.x, r.right(), r.y, r.bottom())
                        } else {
                            (r.y, r.bottom(), r.x, r.right())
                        };
                        (fixed >= near && fixed <= far && start <= hi && end >= lo)
                            .then_some((start.max(lo), end.min(hi)))
                    })
                    .collect();
                covered.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut end = lo;
                for (start, next) in covered {
                    if start > end {
                        return true;
                    }
                    end = end.max(next);
                    if end >= hi {
                        return false;
                    }
                }
                end < hi
            };
            let r = w.rect;
            let y = (r.y.max(orb.y), r.bottom().min(orb.bottom()));
            let x = (r.x.max(orb.x), r.right().min(orb.right()));
            // Authorize the same nearest edge that release_dock will use, never
            // an unrelated visible edge around an occluded corner.
            let visible = match geometry::nearest_side(r, cursor_x, cursor_y) {
                Side::Left => visible_segment(true, r.x, y.0, y.1),
                Side::Right => visible_segment(true, r.right(), y.0, y.1),
                Side::Top => visible_segment(false, r.y, x.0, x.1),
                Side::Bottom => visible_segment(false, r.bottom(), x.0, x.1),
            };
            if visible {
                return Some(target(w));
            }
        }
        None
    }
    fn orb(&self, widget: Rect, screen: Screen) -> Rect {
        geometry::orb_box(
            widget,
            platform::coordinate(92.0 * screen.units_per_logical_pixel),
            self.view.layout_anchor_left,
            self.view.layout_anchor_top,
        )
    }
    fn measure_attachments(&mut self, widget: Rect, screen: Screen) {
        (self.x, self.y) =
            geometry::attachments(self.orb(widget, screen), screen.work_area, &self.targets());
        self.attachments();
    }
    fn place_orb(
        &mut self,
        window: &WebviewWindow,
        orb: Rect,
        screen: Screen,
        reorient: bool,
    ) -> Result<(), String> {
        let anchor = self.layout_anchor(reorient);
        let (widget, left, top) = geometry::place_widget(
            platform::positioned_rect(orb),
            screen.work_area,
            platform::coordinate(self.requested_width * screen.units_per_logical_pixel),
            platform::coordinate(self.requested_height * screen.units_per_logical_pixel),
            anchor,
        );
        self.position(window, widget, screen)?;
        self.view.layout_anchor_left = left;
        self.view.layout_anchor_top = top;
        Ok(())
    }
    fn layout_anchor(&self, reorient: bool) -> Option<(bool, bool)> {
        // Only a compact 92px window can change DOM corners without a transient
        // jump between the native frame commit and the asynchronous UI reply.
        if reorient && self.requested_width == 92.0 && self.requested_height == 92.0 {
            None
        } else {
            Some((self.view.layout_anchor_left, self.view.layout_anchor_top))
        }
    }
    fn targets(&self) -> Vec<Target> {
        self.windows
            .iter()
            .enumerate()
            .filter(|(index, w)| {
                self.eligible(w)
                    && !self.windows[..*index].iter().any(|front| {
                        front.rect.x <= w.rect.x
                            && front.rect.y <= w.rect.y
                            && front.rect.right() >= w.rect.right()
                            && front.rect.bottom() >= w.rect.bottom()
                    })
            })
            .map(|(_, w)| Target {
                id: w.id,
                rect: w.rect,
            })
            .collect()
    }
    fn refresh_windows(&mut self) {
        self.windows_observed_at = Some(Instant::now());
        match platform::windows() {
            Ok(windows) => {
                self.windows = windows;
                self.view.capabilities.window_edges = "available";
                self.view.window_targets = self.targets().len();
            }
            Err(error) => {
                self.windows.clear();
                self.view.window_targets = 0;
                self.view.capabilities.window_edges = if platform::COORDINATES == "unsupported" {
                    "unsupported"
                } else {
                    "unavailable"
                };
                self.view.capabilities.reason = Some(error);
            }
        }
    }
    fn desktop(&mut self, window: &WebviewWindow) -> Result<platform::Desktop, String> {
        let desktop = platform::desktop(window)?;
        if !desktop.rect.valid() || desktop.screens.is_empty() {
            return Err("No usable display work area is available".into());
        }
        self.view.geometry = Some(desktop.rect);
        self.view.monitor = geometry::nearest_screen(
            &desktop.screens,
            desktop.rect.x + desktop.rect.width / 2.0,
            desktop.rect.y + desktop.rect.height / 2.0,
        );
        self.view.capabilities.screen_edges = "available";
        if self.view.capabilities.window_edges == "available" {
            self.view.capabilities.reason = None;
        }
        Ok(desktop)
    }
    fn position(
        &mut self,
        window: &WebviewWindow,
        rect: Rect,
        monitor: Screen,
    ) -> Result<(), String> {
        let rect = platform::positioned_rect(rect);
        if self.view.geometry != Some(rect) {
            platform::apply(window, rect)?;
        }
        self.view.geometry = Some(rect);
        self.view.monitor = Some(monitor);
        self.last_placed = Some(rect);
        Ok(())
    }
    fn displaced_screen_dock(&self, desktop: &platform::Desktop) -> Option<(Rect, Screen)> {
        let expected = self.last_placed.filter(|frame| *frame != desktop.rect)?;
        let screen = geometry::nearest_screen(
            &desktop.screens,
            expected.x + expected.width / 2.0,
            expected.y + expected.height / 2.0,
        )?;
        Some((self.orb(expected, screen), screen))
    }
    fn resize_anchor(&self, desktop: &platform::Desktop) -> Result<(Rect, Screen), String> {
        if self.drag.is_none() && self.attachment.is_none() && self.motion.is_none() {
            if let Some(expected) = self.displaced_screen_dock(desktop) {
                return Ok(expected);
            }
        }
        let screen = geometry::nearest_screen(
            &desktop.screens,
            desktop.rect.x + desktop.rect.width / 2.0,
            desktop.rect.y + desktop.rect.height / 2.0,
        )
        .ok_or("No usable display")?;
        Ok((self.orb(desktop.rect, screen), screen))
    }
    fn sample_drag(
        &mut self,
        screen: Screen,
        cursor_x: f64,
        cursor_y: f64,
        pressed: bool,
        own_frame: Rect,
    ) -> Option<Rect> {
        let drag = self.drag.as_mut().unwrap();
        if drag.release_frame.is_some() {
            return None;
        }
        if !pressed || drag.started.elapsed() > Duration::from_secs(120) {
            // Mouse movement after pointerup belongs to another gesture. Keep this
            // frame until the explicit end command supplies its captured up point.
            drag.release_frame = Some(own_frame);
            return None;
        }
        drag.last_cursor = (cursor_x, cursor_y);
        self.view.last_drag_moved |= (cursor_x - drag.start_x).hypot(cursor_y - drag.start_y)
            >= 4.0 * screen.units_per_logical_pixel;
        self.view
            .last_drag_moved
            .then(|| drag.widget_at(screen, cursor_x, cursor_y))
    }
    fn window_refresh_due(&self) -> bool {
        [self.x, self.y]
            .iter()
            .flatten()
            .any(|l| l.source == Source::Window)
            && self
                .windows_observed_at
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(2))
    }
    fn end_movement(&mut self, drag: &Drag, reported: Option<bool>) -> Option<Rect> {
        let sampled_movement = self.view.last_drag_moved;
        // A complete DOM gesture can rule out a different physical press sampled
        // by a delayed begin. Cancel, without a report, keeps native evidence.
        self.view.last_drag_moved = reported.unwrap_or(sampled_movement);
        (sampled_movement && !self.view.last_drag_moved).then_some(drag.start_frame)
    }
    fn settle(
        &mut self,
        window: &WebviewWindow,
        from: Rect,
        to: Rect,
        screen: Screen,
        x: Option<Latch>,
        y: Option<Latch>,
    ) -> Result<(), String> {
        let distance = (to.x - from.x).hypot(to.y - from.y) / screen.units_per_logical_pixel;
        self.x = None;
        self.y = None;
        self.attachments();
        if self.reduced_motion || distance < 1.0 {
            self.motion = None;
            self.place_orb(window, to, screen, true)?;
            self.x = x;
            self.y = y;
            self.attachments();
        } else {
            // Short cubic deceleration, without bounce or an unbounded spring.
            self.motion = Some(DockMotion {
                from,
                to,
                screen,
                x,
                y,
                started: Instant::now(),
                duration: Duration::from_secs_f64((0.12 + distance * 0.0002).min(0.22)),
            });
        }
        self.wake();
        Ok(())
    }
    fn screen_fallback(
        &mut self,
        window: &WebviewWindow,
        orb: Rect,
        screen: Screen,
    ) -> Result<(), String> {
        self.attachment = None;
        self.attachment_owner = None;
        self.reset_follow_pacing();
        let (to, x, y) = geometry::release_dock(
            orb,
            screen.work_area,
            None,
            orb.x + orb.width / 2.0,
            orb.y + orb.height / 2.0,
        )
        .ok_or("Display recovery geometry is unavailable")?;
        self.settle(window, orb, to, screen, x, y)
    }
    fn reset_follow_pacing(&mut self) {
        self.last_follow_sample = None;
        self.fast_follow_until = None;
    }
    fn note_follow_motion(&mut self, w: OtherWindow, now: Instant) {
        match self.last_follow_sample {
            Some((id, pid, previous)) if id == w.id && pid == w.owner_pid => {
                if previous != w.rect {
                    self.fast_follow_until = Some(now + MOVING_TARGET_HOLD);
                }
            }
            _ => self.fast_follow_until = None,
        }
        self.last_follow_sample = Some((w.id, w.owner_pid, w.rect));
    }
    fn screen_for_target(
        screens: &[Screen],
        target: Target,
        preferred: Screen,
        x: f64,
        y: f64,
    ) -> Screen {
        let fits = |screen: &Screen| {
            let size = platform::coordinate(92.0 * screen.units_per_logical_pixel);
            geometry::target_interior(
                Rect {
                    x: 0.,
                    y: 0.,
                    width: size,
                    height: size,
                },
                screen.work_area,
                target.rect,
            )
            .is_some()
        };
        if fits(&preferred) {
            return preferred;
        }
        screens
            .iter()
            .copied()
            .filter(fits)
            .reduce(|a, b| geometry::nearest_screen(&[a, b], x, y).unwrap_or(a))
            .unwrap_or(preferred)
    }
    fn follow_placement(
        &mut self,
        w: OtherWindow,
        screen: Screen,
    ) -> Option<(Rect, Option<Latch>, Option<Latch>)> {
        let attachment = self.attachment?;
        if !self.eligible(&w) || self.attachment_owner != Some(w.owner_pid) {
            return None;
        }
        let target = Target {
            id: w.id,
            rect: w.rect,
        };
        let (orb, x, y) = geometry::follow_orb(
            attachment,
            target,
            platform::coordinate(92.0 * screen.units_per_logical_pixel),
            screen.work_area,
        )?;
        // A temporarily clipped edge has no contact latch, but keeps its target
        // and preferred position so moving back restores the same attachment.
        self.note_follow_motion(w, Instant::now());
        if let Some(cached) = self.windows.iter_mut().find(|old| old.id == w.id) {
            *cached = w;
        }
        Some((orb, x, y))
    }
    fn follow(
        &mut self,
        window: &WebviewWindow,
        desktop: &platform::Desktop,
    ) -> Result<(), String> {
        let Some(attachment) = self.attachment else {
            return Ok(());
        };
        let target = match platform::window(attachment.target_id) {
            Ok(target) => target,
            Err(error) => {
                self.view.capabilities.window_edges = "unavailable";
                self.view.capabilities.reason = Some(error);
                None
            }
        };
        if let Some(w) = target {
            let point = match attachment.edge {
                Side::Left => (w.rect.x, w.rect.y + attachment.fraction * w.rect.height),
                Side::Right => (
                    w.rect.right(),
                    w.rect.y + attachment.fraction * w.rect.height,
                ),
                Side::Top => (w.rect.x + attachment.fraction * w.rect.width, w.rect.y),
                Side::Bottom => (
                    w.rect.x + attachment.fraction * w.rect.width,
                    w.rect.bottom(),
                ),
            };
            let screen = geometry::nearest_screen(&desktop.screens, point.0, point.1)
                .ok_or("No usable display")?;
            let screen = Self::screen_for_target(
                &desktop.screens,
                Target {
                    id: w.id,
                    rect: w.rect,
                },
                screen,
                point.0,
                point.1,
            );
            if let Some((orb, x, y)) = self.follow_placement(w, screen) {
                if let Some(motion) = self.motion.as_mut() {
                    motion.to = orb;
                    motion.screen = screen;
                    motion.x = x;
                    motion.y = y;
                } else {
                    self.place_orb(window, orb, screen, true)?;
                    self.x = x;
                    self.y = y;
                    self.attachments();
                }
                return Ok(());
            }
        }
        self.windows.retain(|w| w.id != attachment.target_id);
        let screen = self.view.monitor.ok_or("No usable display")?;
        self.screen_fallback(window, self.orb(desktop.rect, screen), screen)
    }
    fn animate(&mut self, window: &WebviewWindow) -> Result<(), String> {
        let Some(motion) = self.motion else {
            return Ok(());
        };
        let (frame, done) = motion.frame(motion.started.elapsed());
        self.place_orb(window, frame, motion.screen, done)?;
        if done {
            self.motion = None;
            self.x = motion.x;
            self.y = motion.y;
            self.attachments();
        }
        Ok(())
    }
    fn tick_interval(&self) -> Duration {
        self.tick_interval_at(Instant::now())
    }
    fn tick_interval_at(&self, now: Instant) -> Duration {
        // The target read can run faster than the display while it moves, to
        // reduce polling phase delay. Stop the burst shortly after it rests.
        if self.drag.is_none()
            && self.motion.is_none()
            && self.attachment.is_some()
            && self.fast_follow_until.is_some_and(|until| now < until)
        {
            return MOVING_TARGET_INTERVAL;
        }
        if self.motion.is_some()
            || self.attachment.is_some()
            || self
                .drag
                .as_ref()
                .is_some_and(|d| d.release_frame.is_none())
        {
            ACTIVE_TICK_INTERVAL
        } else {
            Duration::from_millis(500)
        }
    }
    fn tick(&mut self, window: &WebviewWindow) -> Result<(), String> {
        let desktop = self.desktop(window)?;
        if self.drag.is_some() {
            if self.drag.as_ref().unwrap().release_frame.is_some() {
                self.view.dragging = false;
                return Ok(());
            }
            let (cursor_x, cursor_y, pressed) = platform::pointer()?;
            self.view.capabilities.drag = "available";
            let screen = geometry::nearest_screen(&desktop.screens, cursor_x, cursor_y)
                .ok_or("No usable display")?;
            if let Some(raw) = self.sample_drag(screen, cursor_x, cursor_y, pressed, desktop.rect) {
                let orb = self.orb(raw, screen);
                // Keep the orb under the pointer and the layout direction fixed.
                // The requested panel size is restored when release chooses space.
                self.place_orb(window, orb, screen, false)?;
                self.measure_attachments(self.view.geometry.unwrap(), screen);
            }
            self.view.dragging =
                self.drag.as_ref().unwrap().release_frame.is_none() && self.view.last_drag_moved;
        } else {
            if self.attachment.is_some() {
                self.follow(window, &desktop)?;
                self.animate(window)?;
                return Ok(());
            }
            if self.motion.is_some() {
                self.animate(window)?;
                return Ok(());
            }
            // External OS placement is not a new user drag. Keep the last dock
            // after a window manager recenters us; display removal still clamps
            // it to an available work area. Explicit drags take the branch above.
            if let Some((expected, screen)) = self.displaced_screen_dock(&desktop) {
                let (orb, x, y) = geometry::release_dock(
                    expected,
                    screen.work_area,
                    None,
                    expected.x + expected.width / 2.0,
                    expected.y + expected.height / 2.0,
                )
                .ok_or("Display recovery geometry is unavailable")?;
                self.place_orb(window, orb, screen, true)?;
                self.x = x;
                self.y = y;
                self.attachments();
                return Ok(());
            }
            // Recover after display removal/work-area changes without reading the mouse.
            if self.window_refresh_due() {
                self.refresh_windows();
            }
            let screen = self.view.monitor.ok_or("No usable display")?;
            let orb = self.orb(desktop.rect, screen);
            let fitted = geometry::clamp(orb, screen.work_area);
            if fitted != orb {
                let (orb, x, y) = geometry::release_dock(
                    fitted,
                    screen.work_area,
                    None,
                    fitted.x + fitted.width / 2.0,
                    fitted.y + fitted.height / 2.0,
                )
                .ok_or("Display recovery geometry is unavailable")?;
                self.place_orb(window, orb, screen, true)?;
                self.x = x;
                self.y = y;
                self.attachments();
            } else {
                self.place_orb(window, orb, screen, false)?;
                self.measure_attachments(self.view.geometry.unwrap(), screen);
            }
        }
        Ok(())
    }
    fn fail(&mut self, error: String) {
        self.drag = None;
        self.motion = None;
        self.attachment = None;
        self.attachment_owner = None;
        self.reset_follow_pacing();
        self.last_placed = None;
        self.view.dragging = false;
        self.view.last_error = Some(error.clone());
        self.view.capabilities.reason = Some(error);
        self.view.capabilities.drag = if platform::COORDINATES == "unsupported" {
            "unsupported"
        } else {
            "unavailable"
        };
        self.view.capabilities.screen_edges = if platform::COORDINATES == "unsupported" {
            "unsupported"
        } else {
            "unavailable"
        };
    }
}

fn dispatch(
    window: WebviewWindow,
    action: impl FnOnce(&WebviewWindow, &mut Engine) -> Result<(), String> + Send + 'static,
) -> Result<MagnetState, String> {
    if window.label() != "orb" {
        return Err("Magnet commands apply only to the orb window".into());
    }
    let (send, recv) = mpsc::sync_channel(1);
    let own = window.clone();
    window
        .run_on_main_thread(move || {
            let state = own.state::<Magnet>();
            let result = match state.lock() {
                Ok(mut engine) => match action(&own, &mut engine) {
                    Ok(()) => {
                        engine.view.revision = engine.view.revision.saturating_add(1);
                        Ok(engine.view.clone())
                    }
                    Err(error) => {
                        engine.fail(error.clone());
                        Err(error)
                    }
                },
                Err(_) => Err("Magnet state is unavailable".into()),
            };
            let _ = send.send(result);
        })
        .map_err(|_| "Window event loop is unavailable")?;
    recv.recv_timeout(Duration::from_secs(3))
        .map_err(|_| "Window geometry request timed out")?
}

pub fn start(window: WebviewWindow) {
    let (wake, wake_receiver) = mpsc::sync_channel(1);
    {
        let state = window.state::<Magnet>();
        if let Ok(mut engine) = state.lock() {
            engine.wake = Some(wake);
            if let Err(error) = platform::configure(&window) {
                eprintln!("Orb window configuration failed: {error}");
                engine.fail(error);
                return;
            }
            engine.configured = true;
            engine.refresh_windows();
            match engine.desktop(&window) {
                Ok(desktop) => {
                    if let Some(screen) = engine.view.monitor {
                        let orb = engine.orb(desktop.rect, screen);
                        let preferred = geometry::clamp(
                            Rect {
                                x: screen.work_area.right()
                                    - orb.width
                                    - 24.0 * screen.units_per_logical_pixel,
                                y: screen.work_area.bottom()
                                    - orb.height
                                    - 24.0 * screen.units_per_logical_pixel,
                                ..orb
                            },
                            screen.work_area,
                        );
                        if let Some((orb, x, y)) = geometry::release_dock(
                            preferred,
                            screen.work_area,
                            None,
                            preferred.x + preferred.width / 2.0,
                            preferred.y + preferred.height / 2.0,
                        ) {
                            if let Err(error) = engine.place_orb(&window, orb, screen, true) {
                                engine.fail(error);
                            } else {
                                engine.x = x;
                                engine.y = y;
                                engine.attachments();
                            }
                        }
                    }
                }
                Err(error) => engine.fail(error),
            }
            match platform::pointer() {
                Ok(_) => engine.view.capabilities.drag = "available",
                Err(error) => engine.view.capabilities.reason = Some(error),
            }
        };
    }
    // Exactly one acknowledged callback at a time; no queued frame backlog and no
    // lock is held while sleeping. Follow reads only the one attached window.
    std::thread::spawn(move || {
        let mut schedule = TickSchedule {
            slot: Instant::now(),
            interval: None,
        };
        loop {
            let (send, recv) = mpsc::sync_channel(1);
            let own = window.clone();
            let posted = Instant::now();
            if window
                .run_on_main_thread(move || {
                    let state = own.state::<Magnet>();
                    let Ok(mut engine) = state.lock() else {
                        let _ = send.send(Duration::from_millis(500));
                        return;
                    };
                    if let Err(error) = engine.tick(&own) {
                        engine.fail(error);
                    }
                    engine.view.revision = engine.view.revision.saturating_add(1);
                    let _ = send.send(engine.tick_interval());
                })
                .is_err()
            {
                break;
            }
            let Ok(interval) = recv.recv() else {
                break;
            };
            let completed = Instant::now();
            let deadline = schedule.next_deadline(posted, completed, interval);
            match wake_receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(()) => schedule.wake(Instant::now()),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

#[tauri::command]
pub async fn get_magnet_state(window: WebviewWindow) -> Result<MagnetState, String> {
    dispatch(window, |_, _| Ok(()))
}

#[tauri::command]
pub async fn set_magnet_preferences(
    window: WebviewWindow,
    preferences: Preferences,
) -> Result<MagnetState, String> {
    dispatch(window, move |window, engine| {
        engine.require_configured()?;
        engine.view.preferences = preferences;
        if engine.view.preferences.window_mode != WindowMode::Off {
            engine.refresh_windows();
        } else {
            engine.view.window_targets = 0;
        }
        // Layout belongs to the orb, not the current window eligibility or latch.
        let desktop = engine.desktop(window)?;
        if engine.attachment.is_some() {
            engine.follow(window, &desktop)?;
        } else {
            engine.measure_attachments(
                desktop.rect,
                engine.view.monitor.ok_or("No usable display")?,
            );
        }
        engine.view.last_error = None;
        engine.wake();
        Ok(())
    })
}

#[tauri::command]
pub async fn begin_magnetic_drag(
    window: WebviewWindow,
    anchor_x: f64,
    anchor_y: f64,
) -> Result<MagnetState, String> {
    if ![anchor_x, anchor_y]
        .iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
    {
        return Err("Pointerdown anchors must be finite values between 0 and 1".into());
    }
    dispatch(window, move |window, engine| {
        engine.require_configured()?;
        engine.view.last_drag_moved = false;
        engine.view.last_error = None;
        let desktop = engine.desktop(window)?;
        let (x, y) = geometry::drag_start(desktop.rect, anchor_x, anchor_y)
            .ok_or("Pointerdown anchor is unavailable")?;
        let scale = engine
            .view
            .monitor
            .ok_or("No usable display")?
            .units_per_logical_pixel;
        engine.reset_follow_pacing();
        engine.drag = Some(Drag {
            start_frame: desktop.rect,
            start_x: x,
            start_y: y,
            anchor_x,
            anchor_y,
            logical_width: desktop.rect.width / scale,
            logical_height: desktop.rect.height / scale,
            started: Instant::now(),
            last_cursor: (x, y),
            release_frame: None,
            resume_motion: engine.motion.take(),
        });
        // A short gesture may already be released. Freeze its unchanged frame and
        // let pointerup supply the captured final point; never sample post-up motion.
        engine.tick(window)?;
        engine.wake();
        Ok(())
    })
}

#[tauri::command]
pub async fn end_magnetic_drag(
    window: WebviewWindow,
    anchor_x: Option<f64>,
    anchor_y: Option<f64>,
    moved: Option<bool>,
    reduced_motion: Option<bool>,
) -> Result<MagnetState, String> {
    if anchor_x.is_some() != anchor_y.is_some()
        || ![anchor_x, anchor_y]
            .into_iter()
            .flatten()
            .all(f64::is_finite)
    {
        return Err("Pointerup anchors must be a finite pair when supplied".into());
    }
    dispatch(window, move |window, engine| {
        engine.require_configured()?;
        engine.reduced_motion = reduced_motion.unwrap_or(engine.reduced_motion);
        engine.view.dragging = false;
        if engine.drag.is_none() {
            return Ok(());
        }
        let desktop = engine.desktop(window)?;
        let drag = engine.drag.take().unwrap();
        if let Some(frame) = engine.end_movement(&drag, moved) {
            let screen = geometry::nearest_screen(
                &desktop.screens,
                frame.x + frame.width / 2.0,
                frame.y + frame.height / 2.0,
            )
            .ok_or("No usable display")?;
            engine.position(window, frame, screen)?;
            engine.measure_attachments(frame, screen);
        }
        if !engine.view.last_drag_moved {
            if let Some(motion) = drag.resume_motion {
                let screen = engine.view.monitor.ok_or("No usable display")?;
                engine.settle(
                    window,
                    engine.orb(engine.view.geometry.unwrap(), screen),
                    motion.to,
                    motion.screen,
                    motion.x,
                    motion.y,
                )?;
            }
            return Ok(());
        }
        let (cursor_x, cursor_y) = drag.release_point(desktop.rect, anchor_x.zip(anchor_y));
        if !cursor_x.is_finite() || !cursor_y.is_finite() {
            return Err("Pointerup position is unavailable".into());
        }
        let mut screen = geometry::nearest_screen(&desktop.screens, cursor_x, cursor_y)
            .ok_or("No usable display")?;
        let mut orb = engine.orb(drag.widget_at(screen, cursor_x, cursor_y), screen);
        if engine.view.preferences.window_mode != WindowMode::Off {
            engine.refresh_windows();
        }
        let target = engine.release_target(cursor_x, cursor_y, orb);
        if let Some(target) = target {
            screen =
                Engine::screen_for_target(&desktop.screens, target, screen, cursor_x, cursor_y);
            orb = engine.orb(drag.widget_at(screen, cursor_x, cursor_y), screen);
        }
        let size = platform::coordinate(92.0 * screen.units_per_logical_pixel);
        let target = target.filter(|_| orb.width == size && orb.height == size);
        let (docked, x, y) =
            geometry::release_dock(orb, screen.work_area, target, cursor_x, cursor_y)
                .ok_or("Release geometry is unavailable")?;
        engine.reset_follow_pacing();
        engine.attachment = target
            .filter(|_| docked.width == orb.width && docked.height == orb.height)
            .and_then(|target| geometry::from_release(docked, target, cursor_x, cursor_y));
        engine.attachment_owner = engine.attachment.and_then(|a| {
            engine
                .windows
                .iter()
                .find(|w| w.id == a.target_id)
                .map(|w| w.owner_pid)
        });
        // The first animation frame is the displayed release frame, avoiding a
        // teleport when pointerup arrives between native pointer samples.
        let from = engine.orb(
            desktop.rect,
            engine.view.monitor.ok_or("No usable display")?,
        );
        engine.settle(window, from, docked, screen, x, y)?;
        Ok(())
    })
}

#[tauri::command]
pub async fn resize_orb_window(
    window: WebviewWindow,
    width: f64,
    height: f64,
) -> Result<MagnetState, String> {
    if !width.is_finite()
        || !height.is_finite()
        || !(92.0..=2000.0).contains(&width)
        || !(92.0..=2000.0).contains(&height)
    {
        return Err("Orb dimensions must be between 92 and 2000 logical pixels".into());
    }
    dispatch(window, move |window, engine| {
        engine.require_configured()?;
        let desktop = engine.desktop(window)?;
        let (orb, screen) = engine.resize_anchor(&desktop)?;
        engine.drag = None;
        engine.view.dragging = false;
        engine.requested_width = width;
        engine.requested_height = height;
        engine.place_orb(window, orb, screen, true)?;
        if engine.view.preferences.window_mode != WindowMode::Off {
            engine.refresh_windows();
        }
        engine.measure_attachments(engine.view.geometry.unwrap(), screen);
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tick_deadline_accounts_for_main_queue_and_callback_time() {
        let start = Instant::now();
        let mut schedule = TickSchedule {
            slot: start,
            interval: None,
        };
        let first = schedule.next_deadline(
            start,
            start + Duration::from_millis(8),
            ACTIVE_TICK_INTERVAL,
        );
        assert_eq!(first, start + ACTIVE_TICK_INTERVAL);
        let second = schedule.next_deadline(
            first + Duration::from_millis(2),
            first + Duration::from_millis(10),
            ACTIVE_TICK_INTERVAL,
        );
        assert_eq!(second, start + ACTIVE_TICK_INTERVAL * 2);
    }
    #[test]
    fn late_ticks_skip_expired_slots_without_a_catchup_queue() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let interval = Duration::from_millis(10);
        let mut schedule = TickSchedule {
            slot: start,
            interval: None,
        };
        assert_eq!(schedule.next_deadline(at(0), at(47), interval), at(50));
        assert_eq!(schedule.next_deadline(at(50), at(80), interval), at(80));
        assert_eq!(schedule.next_deadline(at(80), at(83), interval), at(90));
        let days_later = 2 * 24 * 60 * 60 * 1000;
        assert_eq!(
            schedule.next_deadline(at(90), at(days_later + 7), interval),
            at(days_later + 10)
        );
    }
    #[test]
    fn idle_transitions_and_wakes_reset_the_schedule_phase() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let active = Duration::from_millis(10);
        let idle = Duration::from_millis(500);
        let mut schedule = TickSchedule {
            slot: start,
            interval: None,
        };
        assert_eq!(schedule.next_deadline(at(0), at(3), active), at(10));
        assert_eq!(schedule.next_deadline(at(10), at(13), idle), at(510));
        schedule.wake(at(25));
        assert_eq!(schedule.next_deadline(at(25), at(28), active), at(35));
        // A queued wake starts a new phase even when the interval is unchanged.
        schedule.wake(at(29));
        assert_eq!(schedule.next_deadline(at(29), at(31), active), at(39));
    }
    #[test]
    fn target_motion_bursts_expire_renew_and_do_not_cross_bindings() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let target = |x| OtherWindow {
            id: 7,
            owner_pid: 10,
            codex: true,
            dockable: true,
            rect: Rect {
                x,
                y: 100.,
                width: 400.,
                height: 400.,
            },
        };
        let mut e = Engine::default();
        e.attachment = Some(WindowAttachment {
            target_id: 7,
            edge: Side::Right,
            fraction: 0.5,
        });
        e.note_follow_motion(target(100.), at(0));
        assert_eq!(e.tick_interval_at(at(0)), ACTIVE_TICK_INTERVAL);
        e.note_follow_motion(target(110.), at(10));
        assert_eq!(e.tick_interval_at(at(10)), MOVING_TARGET_INTERVAL);
        e.note_follow_motion(target(110.), at(150));
        assert_eq!(e.fast_follow_until, Some(at(210)));
        e.note_follow_motion(target(120.), at(180));
        assert_eq!(e.tick_interval_at(at(379)), MOVING_TARGET_INTERVAL);
        assert_eq!(e.tick_interval_at(at(380)), ACTIVE_TICK_INTERVAL);
        e.note_follow_motion(target(130.), at(400));
        e.note_follow_motion(
            OtherWindow {
                id: 8,
                ..target(500.)
            },
            at(410),
        );
        assert_eq!(e.tick_interval_at(at(410)), ACTIVE_TICK_INTERVAL);
        e.note_follow_motion(
            OtherWindow {
                id: 8,
                ..target(510.)
            },
            at(420),
        );
        e.note_follow_motion(
            OtherWindow {
                id: 8,
                owner_pid: 20,
                ..target(520.)
            },
            at(430),
        );
        assert_eq!(e.tick_interval_at(at(430)), ACTIVE_TICK_INTERVAL);
        e.note_follow_motion(
            OtherWindow {
                id: 8,
                owner_pid: 20,
                ..target(530.)
            },
            at(440),
        );
        e.reset_follow_pacing();
        assert_eq!(e.last_follow_sample, None);
        assert_eq!(e.fast_follow_until, None);
        e.note_follow_motion(target(900.), at(450));
        assert_eq!(e.tick_interval_at(at(450)), ACTIVE_TICK_INTERVAL);
        e.note_follow_motion(target(910.), at(460));
        e.fail("Fixture target became unavailable".into());
        assert_eq!(e.last_follow_sample, None);
        assert_eq!(e.fast_follow_until, None);
        assert_eq!(e.tick_interval_at(at(460)), Duration::from_millis(500));
    }
    fn hit(e: &Engine, x: f64, y: f64) -> Option<Target> {
        e.release_target(
            x,
            y,
            Rect {
                x: x - 46.,
                y: y - 46.,
                width: 92.,
                height: 92.,
            },
        )
    }
    #[test]
    fn a_background_pointer_hit_cannot_steal_the_foreground_codex_right_border() {
        let mut e = Engine::default();
        e.windows = vec![
            OtherWindow {
                id: 1,
                owner_pid: 10,
                codex: true,
                dockable: true,
                rect: Rect {
                    x: 61.,
                    y: 51.,
                    width: 1311.,
                    height: 790.,
                },
            },
            OtherWindow {
                id: 2,
                owner_pid: 20,
                codex: false,
                dockable: true,
                rect: Rect {
                    x: 471.,
                    y: 82.,
                    width: 946.,
                    height: 674.,
                },
            },
        ];
        // The pointer is over background Chrome, while the 92px orb touches
        // the foreground Codex right edge at x=1372.
        assert_eq!(hit(&e, 1386., 462.).unwrap().id, 1);
        e.view.preferences.window_mode = WindowMode::All;
        assert_eq!(hit(&e, 1386., 462.).unwrap().id, 1);
        e.windows.swap(0, 1);
        e.view.preferences.window_mode = WindowMode::Codex;
        assert!(hit(&e, 1386., 462.).is_none());
        e.view.preferences.window_mode = WindowMode::All;
        assert_eq!(hit(&e, 1386., 462.).unwrap().id, 2);
        e.view.preferences.window_mode = WindowMode::Off;
        assert!(hit(&e, 1386., 462.).is_none());
    }
    #[test]
    fn the_touched_edge_can_remain_visible_beside_a_small_foreground_popup() {
        let mut e = Engine::default();
        let popup = OtherWindow {
            id: 3,
            owner_pid: 30,
            codex: false,
            dockable: false,
            rect: Rect {
                x: 1360.,
                y: 453.,
                width: 24.,
                height: 18.,
            },
        };
        e.windows = vec![
            popup,
            OtherWindow {
                id: 1,
                owner_pid: 10,
                codex: true,
                dockable: true,
                rect: Rect {
                    x: 61.,
                    y: 51.,
                    width: 1311.,
                    height: 790.,
                },
            },
            OtherWindow {
                id: 2,
                owner_pid: 20,
                codex: false,
                dockable: true,
                rect: Rect {
                    x: 471.,
                    y: 82.,
                    width: 946.,
                    height: 674.,
                },
            },
        ];
        // The projection (1372,462) is covered, but the touched edge spans
        // y=416..508 and has visible portions above and below the popup.
        assert_eq!(hit(&e, 1386., 462.).unwrap().id, 1);
        // Two foreground spans together cover the entire touched edge.
        e.windows[0].rect.y = 416.;
        e.windows[0].rect.height = 50.;
        e.windows.insert(
            1,
            OtherWindow {
                id: 4,
                rect: Rect {
                    x: 1360.,
                    y: 460.,
                    width: 24.,
                    height: 48.,
                },
                ..popup
            },
        );
        assert!(hit(&e, 1386., 462.).is_none());
        e.windows[0].rect.height = 34.;
        e.windows[1].rect.y = 470.;
        e.windows[1].rect.height = 38.;
        assert_eq!(hit(&e, 1386., 462.).unwrap().id, 1);
        // Covering the pointer does not hide the separate touched edge.
        e.windows[0].rect = Rect {
            x: 1380.,
            y: 450.,
            width: 20.,
            height: 24.,
        };
        assert_eq!(hit(&e, 1386., 462.).unwrap().id, 1);
    }
    #[test]
    fn an_adjacent_foreground_pointer_blocker_does_not_hide_a_visible_codex_edge() {
        let mut e = Engine::default();
        e.windows = vec![
            OtherWindow {
                id: 2,
                owner_pid: 20,
                codex: false,
                dockable: false,
                rect: Rect {
                    x: 1358.,
                    y: 688.,
                    width: 126.,
                    height: 126.,
                },
            },
            OtherWindow {
                id: 1,
                owner_pid: 10,
                codex: true,
                dockable: true,
                rect: Rect {
                    x: 46.,
                    y: 109.,
                    width: 1311.,
                    height: 790.,
                },
            },
        ];
        // Real snapshot geometry, with generic owners: a front popup begins
        // one pixel beyond the Codex edge, covering the pointer but not the edge.
        assert_eq!(hit(&e, 1386., 751.).unwrap().id, 1);
        e.windows[0].dockable = true;
        assert_eq!(hit(&e, 1386., 751.).unwrap().id, 1);
        e.view.preferences.window_mode = WindowMode::All;
        assert_eq!(hit(&e, 1386., 751.).unwrap().id, 2);
        e.view.preferences.window_mode = WindowMode::Codex;
        e.windows[0].rect.x = 1356.;
        // Moving that same front window over the touched edge must block it.
        assert!(hit(&e, 1386., 751.).is_none());
        e.windows[0].rect.x = 1358.;
        e.view.preferences.window_mode = WindowMode::Off;
        assert!(hit(&e, 1386., 751.).is_none());
    }
    #[test]
    fn a_covered_pointer_cannot_select_a_background_window_without_an_edge_hit() {
        let mut e = Engine::default();
        e.windows = vec![
            OtherWindow {
                id: 2,
                owner_pid: 20,
                codex: false,
                dockable: false,
                rect: Rect {
                    x: 380.,
                    y: 280.,
                    width: 40.,
                    height: 40.,
                },
            },
            OtherWindow {
                id: 1,
                owner_pid: 10,
                codex: true,
                dockable: true,
                rect: Rect {
                    x: 100.,
                    y: 100.,
                    width: 600.,
                    height: 500.,
                },
            },
        ];
        assert!(hit(&e, 400., 300.).is_none());
        e.view.preferences.window_mode = WindowMode::All;
        assert!(hit(&e, 400., 300.).is_none());
    }
    #[test]
    fn a_neighbor_display_is_used_only_when_the_preferred_target_area_cannot_fit() {
        let a = Screen {
            work_area: Rect {
                x: 0.,
                y: 0.,
                width: 1000.,
                height: 800.,
            },
            units_per_logical_pixel: 1.,
        };
        let b = Screen {
            work_area: Rect {
                x: 1000.,
                ..a.work_area
            },
            ..a
        };
        let target = Target {
            id: 7,
            rect: Rect {
                x: 691.,
                y: 100.,
                width: 400.,
                height: 400.,
            },
        };
        // The nearest edge is on B, but its visible strip is only 91px wide.
        let selected = Engine::screen_for_target(&[a, b], target, b, 1091., 300.);
        assert_eq!(selected.work_area, a.work_area);
        let (orb, x, y) = geometry::release_dock(
            Rect {
                x: 1044.,
                y: 254.,
                width: 92.,
                height: 92.,
            },
            selected.work_area,
            Some(target),
            1090.,
            300.,
        )
        .unwrap();
        assert!(target.rect.contains_rect(orb));
        assert!(selected.work_area.contains_rect(orb));
        assert_eq!((x, y), (None, None));
        let binding = geometry::from_release(orb, target, 1090., 300.).unwrap();
        assert_eq!(
            geometry::follow_orb(binding, target, 92., selected.work_area)
                .unwrap()
                .0,
            orb
        );

        let just_fits = Target {
            rect: Rect {
                x: 692.,
                ..target.rect
            },
            ..target
        };
        assert_eq!(
            Engine::screen_for_target(&[a, b], just_fits, b, 1092., 300.).work_area,
            b.work_area
        );
        let negative = Screen {
            work_area: Rect {
                x: -1000.,
                ..a.work_area
            },
            ..a
        };
        let scaled = Screen {
            units_per_logical_pixel: 2.,
            ..a
        };
        let wide = Target {
            rect: Rect {
                x: -50.,
                ..target.rect
            },
            ..target
        };
        assert_eq!(
            Engine::screen_for_target(&[negative, scaled], wide, negative, -50., 300.)
                .units_per_logical_pixel,
            2.
        );
        let narrow = Target {
            rect: Rect {
                width: 200.,
                ..wide.rect
            },
            ..wide
        };
        assert_eq!(
            Engine::screen_for_target(&[negative, scaled], narrow, negative, -50., 300.).work_area,
            negative.work_area
        );
    }
    #[test]
    fn following_a_temporarily_clipped_or_small_target_keeps_its_binding() {
        let screen = Screen {
            work_area: Rect {
                x: 0.,
                y: 0.,
                width: 1000.,
                height: 800.,
            },
            units_per_logical_pixel: 1.,
        };
        let target = |x, width| OtherWindow {
            id: 7,
            owner_pid: 10,
            codex: true,
            dockable: true,
            rect: Rect {
                x,
                y: 100.,
                width,
                height: 400.,
            },
        };
        let mut e = Engine::default();
        let binding = WindowAttachment {
            target_id: 7,
            edge: Side::Right,
            fraction: 0.5,
        };
        e.attachment = Some(binding);
        e.attachment_owner = Some(10);
        e.windows = vec![target(500., 400.)];
        for (current, orb_x, contact, contained) in [
            (target(500., 400.), 808., true, true),
            (target(600., 400.), 908., true, true),
            (target(601., 400.), 908., false, true),
            (target(909., 400.), 908., false, false),
            (target(908., 400.), 908., false, true),
            (target(500., 91.), 499., false, false),
            (target(500., 400.), 808., true, true),
        ] {
            let (orb, x, y) = e.follow_placement(current, screen).unwrap();
            assert_eq!(
                orb,
                Rect {
                    x: orb_x,
                    y: 254.,
                    width: 92.,
                    height: 92.
                }
            );
            assert_eq!(x.or(y).is_some(), contact);
            assert_eq!(current.rect.contains_rect(orb), contained);
            assert!(screen.work_area.contains_rect(orb));
            assert_eq!(e.attachment, Some(binding));
            assert_eq!(e.attachment_owner, Some(10));
            assert_eq!(e.windows[0].rect, current.rect);
        }
        // Reused IDs and policy changes still reject this follow path.
        for rejected in [
            OtherWindow {
                owner_pid: 20,
                ..target(500., 400.)
            },
            OtherWindow {
                id: 8,
                ..target(500., 400.)
            },
            OtherWindow {
                dockable: false,
                ..target(500., 400.)
            },
        ] {
            assert!(e.follow_placement(rejected, screen).is_none());
        }
        e.view.preferences.window_mode = WindowMode::Off;
        assert!(e.follow_placement(target(500., 400.), screen).is_none());
        e.view.preferences.window_mode = WindowMode::Codex;
        assert!(e
            .follow_placement(
                OtherWindow {
                    codex: false,
                    ..target(500., 400.)
                },
                screen
            )
            .is_none());
        e.view.preferences.window_mode = WindowMode::All;
        assert!(e
            .follow_placement(
                OtherWindow {
                    codex: false,
                    ..target(500., 400.)
                },
                screen
            )
            .is_some());
    }
    #[test]
    fn a_visible_corner_edge_does_not_authorize_the_occluded_nearest_edge() {
        let mut e = Engine::default();
        e.windows = vec![OtherWindow {
            id: 1,
            owner_pid: 10,
            codex: true,
            dockable: true,
            rect: Rect {
                x: 100.,
                y: 100.,
                width: 500.,
                height: 400.,
            },
        }];
        assert_eq!(hit(&e, 94., 94.).unwrap().id, 1);
        assert_eq!(
            geometry::nearest_side(e.windows[0].rect, 94., 94.),
            Side::Left
        );
        e.windows.insert(
            0,
            OtherWindow {
                id: 2,
                owner_pid: 20,
                codex: false,
                dockable: false,
                rect: Rect {
                    x: 90.,
                    y: 100.,
                    width: 20.,
                    height: 40.,
                },
            },
        );
        // The nearest left edge is fully covered; the visible top edge is not
        // the edge that release_dock will choose at this equal-distance corner.
        assert!(hit(&e, 94., 94.).is_none());
    }
    #[test]
    fn all_four_visible_borders_accept_the_ball_footprint_but_never_reach_beyond_it() {
        let mut e = Engine::default();
        e.windows.push(OtherWindow {
            id: 2,
            owner_pid: 3,
            codex: true,
            dockable: true,
            rect: Rect {
                x: 100.,
                y: 100.,
                width: 500.,
                height: 400.,
            },
        });
        for (x, y) in [
            (99., 300.),
            (601., 300.),
            (350., 99.),
            (350., 501.),
            (600., 300.),
            (350., 500.),
        ] {
            assert_eq!(hit(&e, x, y).unwrap().id, 2, "border {x},{y}");
        }
        for (x, y) in [(53., 300.), (647., 300.), (350., 53.), (350., 547.)] {
            assert!(hit(&e, x, y).is_none(), "outside footprint {x},{y}");
        }
        // The pointer is in empty space, but another application's popup covers
        // the portion of the Codex edge touched by the ball.
        e.windows.insert(
            0,
            OtherWindow {
                id: 1,
                owner_pid: 4,
                codex: false,
                dockable: false,
                rect: Rect {
                    x: 100.,
                    y: 200.,
                    width: 80.,
                    height: 200.,
                },
            },
        );
        assert!(hit(&e, 99., 300.).is_none());
        assert_eq!(hit(&e, 601., 300.).unwrap().id, 2);
    }
    #[test]
    fn dock_motion_decelerates_without_overshoot_and_has_a_finite_end() {
        let from = Rect {
            x: -200.,
            y: 50.,
            width: 92.,
            height: 92.,
        };
        let to = Rect {
            x: 800.,
            y: 750.,
            ..from
        };
        let motion = DockMotion {
            from,
            to,
            screen: Screen {
                work_area: Rect {
                    x: -500.,
                    y: 0.,
                    width: 1500.,
                    height: 1000.,
                },
                units_per_logical_pixel: 1.,
            },
            x: None,
            y: None,
            started: Instant::now(),
            duration: Duration::from_millis(200),
        };
        let mut previous = from;
        let mut previous_step = f64::INFINITY;
        for ms in (0..=200).step_by(20) {
            let (frame, done) = motion.frame(Duration::from_millis(ms));
            assert!(frame.x >= previous.x && frame.x <= to.x);
            assert!(frame.y >= previous.y && frame.y <= to.y);
            if ms > 0 {
                let step = (frame.x - previous.x).hypot(frame.y - previous.y);
                assert!(step <= previous_step + 0.000001);
                previous_step = step;
            }
            assert_eq!(done, ms == 200);
            previous = frame;
        }
        assert_eq!(motion.frame(Duration::from_secs(2)), (to, true));
        let mut e = Engine::default();
        assert_eq!(e.tick_interval(), Duration::from_millis(500));
        e.attachment = Some(WindowAttachment {
            target_id: 1,
            edge: Side::Left,
            fraction: 0.5,
        });
        assert_eq!(e.tick_interval(), ACTIVE_TICK_INTERVAL);
        e.motion = Some(motion);
        e.fast_follow_until = Some(motion.started + MOVING_TARGET_HOLD);
        assert_eq!(e.tick_interval_at(motion.started), ACTIVE_TICK_INTERVAL);
    }
    #[test]
    fn animated_reorientation_waits_until_the_dom_corners_coincide() {
        let mut e = Engine::default();
        e.requested_width = 382.;
        e.requested_height = 690.;
        e.view.layout_anchor_left = false;
        e.view.layout_anchor_top = false;
        let orb = Rect {
            x: 0.,
            y: 200.,
            width: 92.,
            height: 92.,
        };
        let area = Rect {
            x: 0.,
            y: 0.,
            width: 1000.,
            height: 1000.,
        };
        assert_eq!(e.layout_anchor(true), Some((false, false)));
        let (frame, left, top) =
            geometry::place_widget(orb, area, 382., 690., e.layout_anchor(true));
        assert_eq!((left, top), (false, false));
        assert_eq!(geometry::orb_box(frame, 92., false, false), orb);
        e.requested_width = 92.;
        e.requested_height = 92.;
        let (frame, left, top) = geometry::place_widget(orb, area, 92., 92., e.layout_anchor(true));
        assert_eq!((left, top), (true, true));
        // Before the new native state arrives, both old and new CSS anchors put
        // the 80px button at the same 6px inset in the compact window.
        assert_eq!(
            geometry::orb_box(frame, 92., false, false),
            geometry::orb_box(frame, 92., left, top)
        );
    }

    #[test]
    fn external_recentering_does_not_replace_a_valid_screen_dock() {
        let mut e = Engine::default();
        let parked = Rect {
            x: 908.,
            y: 400.,
            width: 92.,
            height: 92.,
        };
        let screen = Screen {
            work_area: Rect {
                x: 0.,
                y: 20.,
                width: 1000.,
                height: 800.,
            },
            units_per_logical_pixel: 1.,
        };
        let mut desktop = platform::Desktop {
            rect: parked,
            screens: vec![screen],
        };
        e.last_placed = Some(parked);
        assert!(e.displaced_screen_dock(&desktop).is_none());
        desktop.rect.x = 454.;
        desktop.rect.y = 374.;
        let (expected, chosen) = e.displaced_screen_dock(&desktop).unwrap();
        assert_eq!(expected, parked);
        let (restored, x, _) = geometry::release_dock(
            expected,
            chosen.work_area,
            None,
            expected.x + 46.,
            expected.y + 46.,
        )
        .unwrap();
        assert_eq!(restored, parked);
        assert_eq!(x.unwrap().source, Source::Screen);
        // An expansion queued before the recovery tick must not trust the
        // system's centered frame as the new resting position.
        let (anchor, chosen) = e.resize_anchor(&desktop).unwrap();
        assert_eq!(anchor, parked);
        let (expanded, left, top) =
            geometry::place_widget(anchor, chosen.work_area, 382., 690., Some((false, false)));
        e.last_placed = Some(expanded);
        desktop.rect = expanded;
        assert!(e.displaced_screen_dock(&desktop).is_none());
        assert_eq!(geometry::orb_box(desktop.rect, 92., left, top), parked);
        desktop.rect.x = 454.;
        // If that display disappears, recovery selects a current work area.
        desktop.screens[0].work_area = Rect {
            x: -800.,
            y: 0.,
            width: 800.,
            height: 700.,
        };
        let (expected, chosen) = e.displaced_screen_dock(&desktop).unwrap();
        let (restored, _, _) = geometry::release_dock(
            expected,
            chosen.work_area,
            None,
            expected.x + 46.,
            expected.y + 46.,
        )
        .unwrap();
        assert!(restored.x >= -800. && restored.right() <= 0.);
    }
    #[test]
    fn default_is_codex_only_and_unknown_terminal_is_not_eligible() {
        let mut e = Engine::default();
        let mut w = OtherWindow {
            id: 1,
            owner_pid: 3,
            rect: Rect {
                x: 0.,
                y: 0.,
                width: 500.,
                height: 400.,
            },
            codex: false,
            dockable: true,
        };
        assert!(!e.eligible(&w));
        w.codex = true;
        assert!(e.eligible(&w));
        e.view.preferences.window_mode = WindowMode::Off;
        assert!(!e.eligible(&w));
        e.view.preferences.window_mode = WindowMode::All;
        w.codex = false;
        assert!(e.eligible(&w));
    }
    #[test]
    fn preferences_accept_only_window_mode() {
        for mode in ["codex", "off", "all"] {
            assert!(serde_json::from_value::<Preferences>(
                serde_json::json!({ "windowMode": mode })
            )
            .is_ok());
        }
        for json in [
            r#"{"windowMode":"terminal"}"#,
            r#"{"windowMode":"codex","screenEdges":false}"#,
            r#"{"windowMode":"codex","snapDistance":14}"#,
        ] {
            assert!(serde_json::from_str::<Preferences>(json).is_err());
        }
    }
    #[test]
    fn covered_codex_window_does_not_attract_through_another_app() {
        let mut e = Engine::default();
        let rect = Rect {
            x: 0.,
            y: 0.,
            width: 500.,
            height: 400.,
        };
        e.windows = vec![
            OtherWindow {
                id: 1,
                owner_pid: 3,
                rect,
                codex: false,
                dockable: true,
            },
            OtherWindow {
                id: 2,
                owner_pid: 3,
                rect,
                codex: true,
                dockable: true,
            },
        ];
        assert!(e.targets().is_empty());
        e.windows.remove(0);
        assert_eq!(e.targets().len(), 1);
        e.view.preferences.window_mode = WindowMode::Off;
        assert!(e.targets().is_empty());
    }
    #[test]
    fn release_hit_test_respects_partial_foreground_occlusion_and_window_mode() {
        let mut e = Engine::default();
        e.windows = vec![
            OtherWindow {
                id: 1,
                owner_pid: 3,
                rect: Rect {
                    x: 200.,
                    y: 100.,
                    width: 200.,
                    height: 200.,
                },
                codex: false,
                dockable: true,
            },
            OtherWindow {
                id: 2,
                owner_pid: 3,
                rect: Rect {
                    x: 100.,
                    y: 100.,
                    width: 500.,
                    height: 400.,
                },
                codex: true,
                dockable: true,
            },
        ];
        assert!(hit(&e, 250., 150.).is_none());
        assert_eq!(hit(&e, 150., 150.).unwrap().id, 2);
        e.view.preferences.window_mode = WindowMode::All;
        assert_eq!(hit(&e, 250., 150.).unwrap().id, 1);
        e.view.preferences.window_mode = WindowMode::Off;
        assert!(hit(&e, 150., 150.).is_none());
        assert!(hit(&e, 900., 600.).is_none());
        // A small/floating foreground window is an occluder, not a dock target.
        e.view.preferences.window_mode = WindowMode::All;
        e.windows[0].dockable = false;
        e.windows[0].codex = true;
        assert!(hit(&e, 250., 150.).is_none());
        assert_eq!(hit(&e, 150., 150.).unwrap().id, 2);
    }
    #[test]
    fn preference_and_contact_updates_do_not_change_layout_anchor() {
        let mut e = Engine::default();
        e.view.layout_anchor_left = true;
        e.view.layout_anchor_top = true;
        e.x = Some(Latch {
            id: 0,
            source: Source::Screen,
            side: Side::Left,
            value: 0.,
        });
        e.y = Some(Latch {
            id: 2,
            source: Source::Screen,
            side: Side::Top,
            value: 0.,
        });
        e.attachments();
        e.view.preferences.window_mode = WindowMode::Off;
        e.x = None;
        e.y = None;
        e.attachments();
        assert!(e.view.layout_anchor_left && e.view.layout_anchor_top);
        assert_eq!((e.view.snap_side_x, e.view.snap_side_y), (None, None));
    }
    #[test]
    fn click_does_not_release_dock_but_movement_stays_sticky_when_cursor_returns() {
        let mut e = Engine::default();
        let screen = Screen {
            work_area: Rect {
                x: 0.,
                y: 0.,
                width: 1000.,
                height: 800.,
            },
            units_per_logical_pixel: 1.,
        };
        let frame = Rect {
            x: 400.,
            y: 300.,
            width: 92.,
            height: 92.,
        };
        let new_drag = || Drag {
            start_frame: frame,
            start_x: 446.,
            start_y: 346.,
            anchor_x: 0.5,
            anchor_y: 0.5,
            logical_width: 92.,
            logical_height: 92.,
            started: Instant::now(),
            last_cursor: (446., 346.),
            release_frame: None,
            resume_motion: None,
        };
        e.drag = Some(new_drag());
        let now = Instant::now();
        e.attachment = Some(WindowAttachment {
            target_id: 7,
            edge: Side::Right,
            fraction: 0.5,
        });
        e.fast_follow_until = Some(now + MOVING_TARGET_HOLD);
        assert_eq!(e.tick_interval_at(now), ACTIVE_TICK_INTERVAL);
        // Mouse has moved 20px after the actual click was released.
        assert!(e.sample_drag(screen, 466., 346., false, frame).is_none());
        assert!(!e.view.last_drag_moved);
        assert_eq!(
            e.drag
                .as_ref()
                .unwrap()
                .release_point(frame, Some((0.5, 0.5))),
            (446., 346.)
        );
        assert!(e.sample_drag(screen, 900., 700., true, frame).is_none());
        assert!(!e.view.last_drag_moved); // The frozen gesture does not consume a new press.

        e.drag = Some(new_drag());
        assert!(e.sample_drag(screen, 468., 346., true, frame).is_some());
        assert!(e.view.last_drag_moved);
        let returned = e.sample_drag(screen, 446., 346., true, frame).unwrap();
        assert!(e.sample_drag(screen, 900., 700., false, frame).is_none());
        assert!(e.view.last_drag_moved);
        assert_eq!(
            e.drag.as_ref().unwrap().release_point(frame, None),
            (446., 346.)
        );
        let (docked, _, y) =
            geometry::release_dock(returned, screen.work_area, hit(&e, 446., 346.), 446., 346.)
                .unwrap();
        assert_eq!(docked.y, 0.);
        assert_eq!(y.unwrap().source, Source::Screen);
    }
    #[test]
    fn late_short_gesture_uses_captured_up_point_and_frozen_frame() {
        let frame = Rect {
            x: 400.,
            y: 300.,
            width: 92.,
            height: 92.,
        };
        let drag = Drag {
            start_frame: frame,
            start_x: 446.,
            start_y: 346.,
            anchor_x: 0.5,
            anchor_y: 0.5,
            logical_width: 92.,
            logical_height: 92.,
            started: Instant::now(),
            last_cursor: (446., 346.),
            release_frame: Some(frame),
            resume_motion: None,
        };
        let other_frame = Rect {
            x: 0.,
            y: 0.,
            width: 382.,
            height: 690.,
        };
        assert_eq!(
            drag.release_point(other_frame, Some((68. / 92., 0.5))),
            (468., 346.)
        );
        assert_eq!(
            drag.release_point(other_frame, Some((-0.5, 1.5))),
            (354., 438.)
        );
        assert_eq!(drag.release_point(other_frame, None), (446., 346.));
    }
    #[test]
    fn explicit_click_report_undoes_a_different_press_consumed_by_late_begin() {
        let mut e = Engine::default();
        let frame = Rect {
            x: 400.,
            y: 300.,
            width: 92.,
            height: 92.,
        };
        let screen = Screen {
            work_area: Rect {
                x: 0.,
                y: 0.,
                width: 1000.,
                height: 800.,
            },
            units_per_logical_pixel: 1.,
        };
        e.drag = Some(Drag {
            start_frame: frame,
            start_x: 446.,
            start_y: 346.,
            anchor_x: 0.5,
            anchor_y: 0.5,
            logical_width: 92.,
            logical_height: 92.,
            started: Instant::now(),
            last_cursor: (446., 346.),
            release_frame: None,
            resume_motion: None,
        });
        // The entire up interval was missed; this is already another mouse press.
        assert!(e.sample_drag(screen, 466., 346., true, frame).is_some());
        let drag = e.drag.take().unwrap();
        assert!(e.view.last_drag_moved);
        assert_eq!(e.end_movement(&drag, Some(false)), Some(frame));
        assert!(!e.view.last_drag_moved);
        assert_eq!(e.end_movement(&drag, Some(true)), None);
        assert!(e.view.last_drag_moved);
        assert_eq!(e.end_movement(&drag, None), None);
        assert!(e.view.last_drag_moved); // Cancel preserves the last pressed evidence.
    }
    #[test]
    fn window_contacts_have_a_bounded_refresh_and_missing_targets_clear() {
        let mut e = Engine::default();
        let widget = Rect {
            x: 500.,
            y: 300.,
            width: 92.,
            height: 92.,
        };
        let screen = Screen {
            work_area: Rect {
                x: 0.,
                y: 0.,
                width: 1000.,
                height: 800.,
            },
            units_per_logical_pixel: 1.,
        };
        e.windows.push(OtherWindow {
            id: 5,
            owner_pid: 3,
            rect: Rect {
                x: 500.,
                y: 100.,
                width: 300.,
                height: 500.,
            },
            codex: true,
            dockable: true,
        });
        e.windows_observed_at = Some(Instant::now());
        e.measure_attachments(widget, screen);
        assert_eq!(e.x.unwrap().source, Source::Window);
        assert!(!e.window_refresh_due());
        e.windows_observed_at = Some(Instant::now() - Duration::from_secs(3));
        assert!(e.window_refresh_due());
        e.windows.clear(); // The next successful/failed snapshot cannot reuse old bounds.
        e.windows_observed_at = Some(Instant::now());
        e.measure_attachments(widget, screen);
        assert!(e.x.is_none() && e.y.is_none());
        assert!(!e.window_refresh_due());
    }
    #[test]
    fn configuration_failure_is_preserved_and_blocks_window_actions() {
        let mut e = Engine::default();
        e.fail("Own window policy could not be configured".into());
        assert_eq!(
            e.require_configured().unwrap_err(),
            "Own window policy could not be configured"
        );
        assert_eq!(
            e.view.last_error.as_deref(),
            Some("Own window policy could not be configured")
        );
        assert_eq!(e.view.capabilities.reason, e.view.last_error);
        assert!(!e.configured);
    }
    #[test]
    fn startup_preferred_position_is_really_docked_and_sets_a_stable_layout() {
        let area = Rect {
            x: 0.,
            y: 25.,
            width: 1440.,
            height: 850.,
        };
        let preferred = Rect {
            x: area.right() - 116.,
            y: area.bottom() - 116.,
            width: 92.,
            height: 92.,
        };
        let (orb, x, y) =
            geometry::release_dock(preferred, area, None, preferred.x + 46., preferred.y + 46.)
                .unwrap();
        assert!(x.is_some() || y.is_some());
        assert!(orb.right() == area.right() || orb.bottom() == area.bottom());
        let (widget, left, top) = geometry::place_widget(orb, area, 92., 92., None);
        assert_eq!(geometry::orb_box(widget, 92., left, top), orb);
        assert_eq!((left, top), (false, false));
    }
    #[test]
    fn state_exposes_no_process_or_window_identity() {
        let s = serde_json::to_value(Engine::default().view).unwrap();
        assert_eq!(s["preferences"]["windowMode"], "codex");
        assert_eq!(s["lastDragMoved"], false);
        assert_eq!(s["layoutAnchorLeft"], false);
        assert_eq!(s["layoutAnchorTop"], false);
        assert!(s["preferences"].get("enabled").is_none());
        assert!(s["preferences"].get("screenEdges").is_none());
        for field in ["pid", "title", "windowId", "owner", "windows"] {
            assert!(s.get(field).is_none());
        }
    }
}
