//! Only own-window writes. External-window reads are bounds, visibility and owner identity.
//! No titles, screenshots, accessibility trees, terminal contents or filesystem reads.
use crate::magnet_geometry::{Rect, Screen};
use tauri::WebviewWindow;

#[derive(Clone, Copy, Debug)]
pub struct OtherWindow {
    pub id: u64,
    pub rect: Rect,
    pub codex: bool,
    pub dockable: bool,
}
pub struct Desktop {
    pub rect: Rect,
    pub screens: Vec<Screen>,
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn configure(_: &WebviewWindow) -> Result<(), String> {
    Ok(())
}

pub fn coordinate(value: f64) -> f64 {
    if cfg!(target_os = "windows") {
        value.round()
    } else {
        value
    }
}

pub fn positioned_rect(rect: Rect) -> Rect {
    Rect {
        x: coordinate(rect.x),
        y: coordinate(rect.y),
        width: coordinate(rect.width),
        height: coordinate(rect.height),
    }
}

#[cfg(target_os = "macos")]
mod os {
    use super::*;
    use core_foundation::{
        base::{CFType, TCFType},
        dictionary::CFDictionary,
        number::CFNumber,
        string::{CFString, CFStringRef},
    };
    use core_graphics::{geometry::CGRect, window::*};
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSEvent, NSRunningApplication, NSScreen, NSWindow, NSWindowCollectionBehavior,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    pub const COORDINATES: &str = "logical_points";
    pub const CODEX_GUI: &str = "bundle_id";
    fn marker() -> Result<MainThreadMarker, String> {
        MainThreadMarker::new().ok_or_else(|| "Window geometry requires the main thread".into())
    }
    fn top() -> Result<f64, String> {
        let screens = NSScreen::screens(marker()?);
        let first = screens.firstObject().ok_or("No displays are available")?;
        let frame = first.frame();
        Ok(frame.origin.y + frame.size.height)
    }
    fn rect(frame: NSRect, top: f64) -> Rect {
        Rect {
            x: frame.origin.x,
            y: top - frame.origin.y - frame.size.height,
            width: frame.size.width,
            height: frame.size.height,
        }
    }
    pub fn configure(window: &WebviewWindow) -> Result<(), String> {
        marker()?;
        let own: &NSWindow = unsafe {
            &*window
                .ns_window()
                .map_err(|_| "Own window is unavailable")?
                .cast()
        };
        // Disable the OS title/background drag path. Programmatic setFrame remains
        // available; display rearrangement is handled by our own geometry recovery.
        own.setMovable(false);
        own.setMovableByWindowBackground(false);
        let mut behavior = own.collectionBehavior();
        behavior.remove(NSWindowCollectionBehavior::FullScreenAllowsTiling);
        behavior.insert(NSWindowCollectionBehavior::FullScreenDisallowsTiling);
        own.setCollectionBehavior(behavior);
        Ok(())
    }
    pub fn desktop(window: &WebviewWindow) -> Result<Desktop, String> {
        let top = top()?;
        // The pointer belongs to this Tauri window and is used only on the main thread.
        let own: &NSWindow = unsafe {
            &*window
                .ns_window()
                .map_err(|_| "Own window is unavailable")?
                .cast()
        };
        let screens = NSScreen::screens(marker()?)
            .iter()
            .map(|s| Screen {
                work_area: rect(s.visibleFrame(), top),
                units_per_logical_pixel: 1.0,
            })
            .filter(|s| s.work_area.valid())
            .collect();
        Ok(Desktop {
            rect: rect(own.frame(), top),
            screens,
        })
    }
    pub fn apply(window: &WebviewWindow, r: Rect) -> Result<(), String> {
        marker()?;
        let own: &NSWindow = unsafe {
            &*window
                .ns_window()
                .map_err(|_| "Own window is unavailable")?
                .cast()
        };
        own.setFrame_display(
            NSRect::new(
                NSPoint::new(r.x, top()? - r.y - r.height),
                NSSize::new(r.width, r.height),
            ),
            true,
        );
        Ok(())
    }
    pub fn pointer() -> Result<(f64, f64, bool), String> {
        marker()?;
        let p = NSEvent::mouseLocation();
        Ok((p.x, top()? - p.y, NSEvent::pressedMouseButtons() & 1 != 0))
    }
    fn value(dict: &CFDictionary<CFString, CFType>, key: CFStringRef) -> Option<CFType> {
        let key = unsafe { CFString::wrap_under_get_rule(key) };
        dict.find(&key).map(|v| (*v).clone())
    }
    pub fn windows() -> Result<Vec<OtherWindow>, String> {
        marker()?;
        let array = copy_window_info(
            kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
            kCGNullWindowID,
        )
        .ok_or("Window geometry is unavailable in this desktop session")?;
        let mut output = Vec::new();
        for raw in array.iter().take(2048) {
            // Read only this explicit whitelist. The system dictionary is never serialized.
            let dict: CFDictionary<CFString, CFType> =
                unsafe { CFDictionary::wrap_under_get_rule(*raw as _) };
            let number = |key| value(&dict, key)?.downcast::<CFNumber>()?.to_i64();
            let (Some(pid), Some(id), Some(layer)) = (
                number(unsafe { kCGWindowOwnerPID }),
                number(unsafe { kCGWindowNumber }),
                number(unsafe { kCGWindowLayer }),
            ) else {
                continue;
            };
            if pid == std::process::id() as i64 {
                continue;
            }
            if value(&dict, unsafe { kCGWindowAlpha })
                .and_then(|v| v.downcast::<CFNumber>())
                .and_then(|v| v.to_f64())
                .is_some_and(|alpha| alpha <= 0.0)
            {
                continue;
            }
            let Some(bounds) = value(&dict, unsafe { kCGWindowBounds })
                .and_then(|v| v.downcast::<CFDictionary>())
                .and_then(|d| CGRect::from_dict_representation(&d))
            else {
                continue;
            };
            let r = Rect {
                x: bounds.origin.x,
                y: bounds.origin.y,
                width: bounds.size.width,
                height: bounds.size.height,
            };
            if !r.valid() {
                continue;
            }
            let codex = NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32)
                .and_then(|app| app.bundleIdentifier())
                .is_some_and(|id| id.to_string() == "com.openai.codex");
            output.push(OtherWindow {
                id: id as u64,
                rect: r,
                codex,
                dockable: layer == 0 && r.width >= 40.0 && r.height >= 40.0,
            });
            if output.len() == 128 {
                break;
            }
        }
        Ok(output)
    }
}

#[cfg(target_os = "windows")]
mod os {
    use super::*;
    use windows_sys::Win32::{
        Foundation::{GetLastError, SetLastError, HWND, LPARAM, POINT, RECT},
        Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS},
        UI::{
            HiDpi::{
                GetAwarenessFromDpiAwarenessContext, GetWindowDpiAwarenessContext,
                DPI_AWARENESS_PER_MONITOR_AWARE,
            },
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON},
            WindowsAndMessaging::*,
        },
    };
    pub const COORDINATES: &str = "physical_pixels";
    // No executable-name/title guesses. Store identity must be verified separately.
    pub const CODEX_GUI: &str = "unavailable";
    fn hwnd(window: &WebviewWindow) -> Result<HWND, String> {
        window
            .hwnd()
            .map(|h| h.0 as HWND)
            .map_err(|_| "Own window is unavailable".into())
    }
    fn rect(r: RECT) -> Rect {
        Rect {
            x: r.left as f64,
            y: r.top as f64,
            width: (r.right - r.left) as f64,
            height: (r.bottom - r.top) as f64,
        }
    }
    pub fn configure(window: &WebviewWindow) -> Result<(), String> {
        let handle = hwnd(window)?;
        unsafe {
            SetLastError(0);
            let previous = GetWindowLongPtrW(handle, GWL_STYLE);
            let error = GetLastError();
            if previous == 0 && error != 0 {
                return Err(format!("Own window style could not be read ({error})"));
            }
            let desired = (previous as u32) & !(WS_THICKFRAME | WS_MAXIMIZEBOX);
            if desired == previous as u32 {
                return Ok(());
            }
            SetLastError(0);
            let result = SetWindowLongPtrW(handle, GWL_STYLE, desired as _);
            let error = GetLastError();
            if result == 0 && error != 0 {
                return Err(format!("Own window style could not be changed ({error})"));
            }
            if SetWindowPos(
                handle,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED
                    | SWP_NOMOVE
                    | SWP_NOSIZE
                    | SWP_NOZORDER
                    | SWP_NOOWNERZORDER
                    | SWP_NOACTIVATE,
            ) == 0
            {
                return Err(format!(
                    "Own window frame could not be refreshed ({})",
                    GetLastError()
                ));
            }
        }
        Ok(())
    }
    pub fn desktop(window: &WebviewWindow) -> Result<Desktop, String> {
        let handle = hwnd(window)?;
        unsafe {
            if GetAwarenessFromDpiAwarenessContext(GetWindowDpiAwarenessContext(handle))
                != DPI_AWARENESS_PER_MONITOR_AWARE
            {
                return Err(
                    "Per-monitor DPI awareness is unavailable; magnetic geometry is disabled"
                        .into(),
                );
            }
            let mut own: RECT = std::mem::zeroed();
            if GetWindowRect(handle, &mut own) == 0 {
                return Err("Own window geometry is unavailable".into());
            }
            let screens = window
                .available_monitors()
                .map_err(|_| "Display work areas are unavailable")?
                .iter()
                .map(|s| {
                    let r = s.work_area();
                    Screen {
                        work_area: Rect {
                            x: r.position.x as f64,
                            y: r.position.y as f64,
                            width: r.size.width as f64,
                            height: r.size.height as f64,
                        },
                        units_per_logical_pixel: s.scale_factor(),
                    }
                })
                .filter(|s| s.work_area.valid())
                .collect();
            Ok(Desktop {
                rect: rect(own),
                screens,
            })
        }
    }
    pub fn apply(window: &WebviewWindow, r: Rect) -> Result<(), String> {
        // SetWindowPos only receives the caller's own Tauri HWND, never enumerated HWNDs.
        if unsafe {
            SetWindowPos(
                hwnd(window)?,
                std::ptr::null_mut(),
                r.x.round() as i32,
                r.y.round() as i32,
                r.width.round() as i32,
                r.height.round() as i32,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER,
            )
        } == 0
        {
            return Err("Own window could not be positioned".into());
        }
        Ok(())
    }
    pub fn pointer() -> Result<(f64, f64, bool), String> {
        unsafe {
            let mut p: POINT = std::mem::zeroed();
            if GetPhysicalCursorPos(&mut p) == 0 {
                return Err("Mouse position is unavailable".into());
            }
            let key = if GetSystemMetrics(SM_SWAPBUTTON) != 0 {
                VK_RBUTTON
            } else {
                VK_LBUTTON
            };
            Ok((p.x as f64, p.y as f64, GetAsyncKeyState(key as i32) < 0))
        }
    }
    unsafe extern "system" fn collect(handle: HWND, data: LPARAM) -> i32 {
        let output = &mut *(data as *mut Vec<OtherWindow>);
        if output.len() >= 128 {
            return 1;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(handle, &mut pid);
        if pid == std::process::id() || IsWindowVisible(handle) == 0 || IsIconic(handle) != 0 {
            return 1;
        }
        let mut cloaked: u32 = 0;
        let cloak_status = DwmGetWindowAttribute(
            handle,
            DWMWA_CLOAKED as u32,
            (&mut cloaked as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
        );
        if cloak_status >= 0 && cloaked != 0 {
            return 1;
        }
        let mut bounds: RECT = std::mem::zeroed();
        if DwmGetWindowAttribute(
            handle,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&mut bounds as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        ) < 0
        {
            // Small popups may not have a DWM extended frame but still occlude
            // another window at the release point.
            if GetWindowRect(handle, &mut bounds) == 0 {
                return 1;
            }
        }
        let r = rect(bounds);
        if r.valid() {
            SetLastError(0);
            let style = GetWindowLongPtrW(handle, GWL_EXSTYLE);
            let style_known = style != 0 || GetLastError() == 0;
            output.push(OtherWindow {
                id: handle as usize as u64,
                rect: r,
                codex: false,
                dockable: cloak_status >= 0
                    && style_known
                    && (style as u32 & WS_EX_TOOLWINDOW) == 0
                    && r.width >= 40.0
                    && r.height >= 40.0,
            });
        }
        1
    }
    pub fn windows() -> Result<Vec<OtherWindow>, String> {
        let mut output = Vec::new();
        if unsafe {
            EnumWindows(
                Some(collect),
                (&mut output as *mut Vec<OtherWindow>) as LPARAM,
            )
        } == 0
        {
            return Err("Window geometry is unavailable in this desktop session".into());
        }
        Ok(output)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod os {
    use super::*;
    pub const COORDINATES: &str = "unsupported";
    pub const CODEX_GUI: &str = "unavailable";
    pub fn desktop(_: &WebviewWindow) -> Result<Desktop, String> {
        Err("Magnetic geometry is supported on macOS and Windows only".into())
    }
    pub fn apply(_: &WebviewWindow, _: Rect) -> Result<(), String> {
        Err("Unsupported platform".into())
    }
    pub fn pointer() -> Result<(f64, f64, bool), String> {
        Err("Unsupported platform".into())
    }
    pub fn windows() -> Result<Vec<OtherWindow>, String> {
        Err("Unsupported platform".into())
    }
}

pub use os::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::magnet_geometry::{orb_box, place_widget};

    #[test]
    fn fractional_dpi_resize_keeps_the_orb_corner_and_cached_frame_stable() {
        let area = Rect {
            x: 0.,
            y: 0.,
            width: 1920.,
            height: 1080.,
        };
        for scale in [1.1, 1.25, 1.5] {
            let size = coordinate(92. * scale);
            let orb = Rect {
                x: area.right() - size,
                y: area.bottom() - size,
                width: size,
                height: size,
            };
            let requested = (coordinate(382. * scale), coordinate(690. * scale));
            let (widget, left, top) = place_widget(orb, area, requested.0, requested.1, None);
            assert_eq!((left, top), (false, false));
            let actual = positioned_rect(widget);
            let actual_orb = orb_box(actual, size, left, top);
            assert!((actual_orb.right() - area.right()).abs() < 0.000001);
            assert!((actual_orb.bottom() - area.bottom()).abs() < 0.000001);
            let repeated = place_widget(
                actual_orb,
                area,
                requested.0,
                requested.1,
                Some((left, top)),
            )
            .0;
            assert_eq!(positioned_rect(repeated), actual);
            let collapsed =
                positioned_rect(place_widget(actual_orb, area, size, size, Some((left, top))).0);
            assert!((collapsed.right() - area.right()).abs() < 0.000001);
            assert!((collapsed.bottom() - area.bottom()).abs() < 0.000001);
            #[cfg(target_os = "windows")]
            if scale == 1.25 {
                assert_eq!((actual.width, actual.height), (478., 863.));
                assert_eq!((collapsed.width, collapsed.height), (115., 115.));
            }
        }
    }
}
