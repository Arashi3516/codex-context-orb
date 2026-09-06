use crate::{
    magnet_geometry::{self as geometry, Latch, Rect, Screen, Side, Source, Target},
    magnet_platform::{self as platform, OtherWindow},
};
use serde::{Deserialize, Serialize};
use std::{
    sync::{mpsc, Mutex},
    time::{Duration, Instant},
};
use tauri::{Manager, WebviewWindow};

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
}
pub type Magnet = Mutex<Engine>;

impl Default for Engine {
    fn default() -> Self {
        let supported = platform::COORDINATES != "unsupported";
        Self {
            view: MagnetState {
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
    fn release_target(&self, cursor_x: f64, cursor_y: f64) -> Option<Target> {
        // Hit-test before eligibility: a foreground non-Codex window can cover
        // only part of a Codex window and must not be clicked through.
        self.windows
            .iter()
            .find(|w| w.rect.contains(cursor_x, cursor_y))
            .filter(|w| self.eligible(w))
            .map(|w| Target {
                id: w.id,
                rect: w.rect,
            })
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
        let anchor = if reorient {
            None
        } else {
            Some((self.view.layout_anchor_left, self.view.layout_anchor_top))
        };
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
        Ok(())
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
                    Ok(()) => Ok(engine.view.clone()),
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
    // lock is held while sleeping. Idle checks read only own geometry/display work areas.
    std::thread::spawn(move || loop {
        let (send, recv) = mpsc::sync_channel(1);
        let own = window.clone();
        if window
            .run_on_main_thread(move || {
                let state = own.state::<Magnet>();
                let Ok(mut engine) = state.lock() else {
                    let _ = send.send(false);
                    return;
                };
                if let Err(error) = engine.tick(&own) {
                    engine.fail(error);
                }
                let _ = send.send(
                    engine
                        .drag
                        .as_ref()
                        .is_some_and(|d| d.release_frame.is_none()),
                );
            })
            .is_err()
        {
            break;
        }
        let Ok(active) = recv.recv() else {
            break;
        };
        let _ = wake_receiver.recv_timeout(Duration::from_millis(if active { 16 } else { 500 }));
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
        engine.measure_attachments(
            desktop.rect,
            engine.view.monitor.ok_or("No usable display")?,
        );
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
            return Ok(());
        }
        let (cursor_x, cursor_y) = drag.release_point(desktop.rect, anchor_x.zip(anchor_y));
        if !cursor_x.is_finite() || !cursor_y.is_finite() {
            return Err("Pointerup position is unavailable".into());
        }
        let screen = geometry::nearest_screen(&desktop.screens, cursor_x, cursor_y)
            .ok_or("No usable display")?;
        let orb = engine.orb(drag.widget_at(screen, cursor_x, cursor_y), screen);
        if engine.view.preferences.window_mode != WindowMode::Off {
            engine.refresh_windows();
        }
        let (orb, x, y) = geometry::release_dock(
            orb,
            screen.work_area,
            engine.release_target(cursor_x, cursor_y),
            cursor_x,
            cursor_y,
        )
        .ok_or("Release geometry is unavailable")?;
        engine.place_orb(window, orb, screen, true)?;
        engine.x = x;
        engine.y = y;
        engine.attachments();
        engine.wake();
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
        engine.drag = None;
        engine.view.dragging = false;
        let desktop = engine.desktop(window)?;
        let screen = engine.view.monitor.ok_or("No usable display")?;
        let orb = engine.orb(desktop.rect, screen);
        engine.requested_width = width;
        engine.requested_height = height;
        engine.place_orb(window, orb, screen, false)?;
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
    fn default_is_codex_only_and_unknown_terminal_is_not_eligible() {
        let mut e = Engine::default();
        let mut w = OtherWindow {
            id: 1,
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
                rect,
                codex: false,
                dockable: true,
            },
            OtherWindow {
                id: 2,
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
        assert!(e.release_target(250., 150.).is_none());
        assert_eq!(e.release_target(150., 150.).unwrap().id, 2);
        e.view.preferences.window_mode = WindowMode::All;
        assert_eq!(e.release_target(250., 150.).unwrap().id, 1);
        e.view.preferences.window_mode = WindowMode::Off;
        assert!(e.release_target(150., 150.).is_none());
        assert!(e.release_target(900., 600.).is_none());
        // A small/floating foreground window is an occluder, not a dock target.
        e.view.preferences.window_mode = WindowMode::All;
        e.windows[0].dockable = false;
        e.windows[0].codex = true;
        assert!(e.release_target(250., 150.).is_none());
        assert_eq!(e.release_target(150., 150.).unwrap().id, 2);
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
        };
        e.drag = Some(new_drag());
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
        let (docked, _, y) = geometry::release_dock(
            returned,
            screen.work_area,
            e.release_target(446., 346.),
            446.,
            346.,
        )
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
            x: 408.,
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
