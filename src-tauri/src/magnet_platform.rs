//! Only own-window writes. External-window reads are bounds, visibility and owner identity.
//! No titles, screenshots, accessibility trees, terminal contents or filesystem reads.
use crate::magnet_geometry::{Rect, Screen};
use tauri::WebviewWindow;

#[derive(Clone, Copy, Debug)]
pub struct OtherWindow {
    pub id: u64,
    pub owner_pid: u32,
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

// Exclude the observed Dock shell surface that covers the desktop at Dock
// level. Do not exempt other applications or Dock popups at other levels.
#[cfg(any(target_os = "macos", test))]
fn is_desktop_shell(bundle: Option<&str>, layer: i64) -> bool {
    bundle == Some("com.apple.dock") && layer == 20
}

#[cfg(target_os = "macos")]
mod os {
    use super::*;
    use core_foundation::{
        array::CFArray,
        base::{CFType, TCFType},
        boolean::CFBoolean,
        dictionary::CFDictionary,
        number::CFNumber,
        string::{CFString, CFStringRef},
    };
    use core_graphics::{geometry::CGRect, window::*};
    use objc2::{
        rc::Retained,
        runtime::{AnyClass, AnyObject, Bool, Imp, Method, Sel},
        sel, ClassType, MainThreadMarker,
    };
    use objc2_app_kit::{
        NSEvent, NSRunningApplication, NSScreen, NSWindow, NSWindowCollectionBehavior,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use std::{
        cell::RefCell,
        ptr,
        sync::{
            atomic::{AtomicPtr, Ordering},
            OnceLock,
        },
    };

    type MainWindowImp = unsafe extern "C" fn(*mut AnyObject, Sel) -> Bool;

    struct MainWindowRoute {
        method: &'static Method,
        original: MainWindowImp,
        receiver: AtomicPtr<AnyObject>,
    }
    static MAIN_WINDOW_ROUTE: OnceLock<MainWindowRoute> = OnceLock::new();
    struct RetainedOrb {
        window: Retained<NSWindow>,
    }
    impl Drop for RetainedOrb {
        fn drop(&mut self) {
            if let Some(route) = MAIN_WINDOW_ROUTE.get() {
                // Clear our address before Rust releases the field, including
                // TLS teardown. No AppKit messages or method changes in Drop.
                route.clear_receiver(Retained::as_ptr(&self.window) as *mut AnyObject);
            }
        }
    }
    thread_local! {
        // The single Orb stays retained for this process's main-thread lifetime,
        // so its registered address cannot be reused by a different window.
        static RETAINED_ORB: RefCell<Option<RetainedOrb>> = const { RefCell::new(None) };
    }

    fn routed_imp() -> Imp {
        // SAFETY: Imp is the runtime's erased function-pointer representation.
        unsafe { std::mem::transmute(routed_can_become_main as MainWindowImp) }
    }
    fn same_imp(left: Imp, right: Imp) -> bool {
        left as *const () == right as *const ()
    }
    unsafe extern "C" fn routed_can_become_main(receiver: *mut AnyObject, selector: Sel) -> Bool {
        // The immutable original is published before exposing this IMP. No lock
        // or TLS borrow is held while invoking the original (it may re-enter).
        let route = MAIN_WINDOW_ROUTE
            .get()
            .expect("Orb main-window original must be published before its route");
        unsafe { route.answer(receiver, selector) }
    }
    impl MainWindowRoute {
        // SAFETY: method must have the verified BOOL(id, SEL) AppKit signature.
        unsafe fn new(method: &'static Method) -> Result<Self, String> {
            let original = method.implementation();
            if same_imp(original, routed_imp()) {
                return Err("Own window route cannot capture itself as the original".into());
            }
            Ok(Self {
                method,
                original: unsafe { std::mem::transmute::<Imp, MainWindowImp>(original) },
                receiver: AtomicPtr::new(ptr::null_mut()),
            })
        }
        fn original_imp(&self) -> Imp {
            // SAFETY: inverse of the checked signature conversion in new.
            unsafe { std::mem::transmute(self.original) }
        }
        unsafe fn answer(&self, receiver: *mut AnyObject, selector: Sel) -> Bool {
            if !receiver.is_null() && receiver == self.receiver.load(Ordering::Acquire) {
                Bool::NO
            } else {
                // Preserve both receiver and selector; messaging the selector
                // again would recurse through our installed method.
                unsafe { (self.original)(receiver, selector) }
            }
        }
        fn register(&self, receiver: *mut AnyObject) -> Result<(), String> {
            let registered = self.receiver.load(Ordering::Acquire);
            if receiver.is_null() || (!registered.is_null() && registered != receiver) {
                return Err("Own window route supports only the same retained Orb".into());
            }
            if registered == receiver {
                return if same_imp(self.method.implementation(), routed_imp()) {
                    Ok(())
                } else {
                    Err("Own window main-window method changed after installation".into())
                };
            }
            if !same_imp(self.method.implementation(), self.original_imp()) {
                return Err("Own window main-window method changed before installation".into());
            }
            self.receiver.store(receiver, Ordering::Release);
            // SAFETY: same verified selector ABI; the original and retained
            // receiver are already published. Installation has one main-thread
            // owner; arbitrary concurrent third-party swizzling is unsupported.
            let previous = unsafe { self.method.set_implementation(routed_imp()) };
            if !same_imp(previous, self.original_imp()) {
                self.withdraw(previous);
                return Err("Own window main-window method changed during installation".into());
            }
            Ok(())
        }
        fn withdraw(&self, previous: Imp) {
            // A later caller through a saved router now passes through to the
            // immutable original, even after the strong Orb reference is dropped.
            self.receiver.store(ptr::null_mut(), Ordering::Release);
            if same_imp(self.method.implementation(), routed_imp()) {
                // SAFETY: restore only while our own compatible IMP is installed.
                unsafe { self.method.set_implementation(previous) };
            }
        }
        fn clear_receiver(&self, receiver: *mut AnyObject) {
            let _ = self.receiver.compare_exchange(
                receiver,
                ptr::null_mut(),
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
    fn main_window_method(
        current: &'static AnyClass,
        tao: &'static AnyClass,
    ) -> Result<&'static Method, String> {
        let kvo_name = format!("NSKVONotifying_{}", tao.name().to_string_lossy());
        if tao.superclass() != Some(NSWindow::class())
            || !(ptr::eq(current, tao)
                || (current.superclass() == Some(tao)
                    && current.name().to_bytes() == kvo_name.as_bytes()))
        {
            return Err(format!(
                "Own window route requires TaoWindow or its direct KVO class (class={current})"
            ));
        }
        let selector = sel!(canBecomeMainWindow);
        tao.verify_sel::<(), Bool>(selector)
            .map_err(|error| format!("Own window main-window ABI is incompatible: {error}"))?;
        let method = tao
            .instance_method(selector)
            .ok_or("Tao main-window method is missing")?;
        // Never replace an inherited NSWindow method. KVO must resolve this
        // selector to Tao's own Method; no isa or KVO implementation is changed.
        if !tao.instance_methods().contains(&method)
            || current.instance_method(selector) != Some(method)
            || current.instance_method(sel!(canBecomeKeyWindow))
                != tao.instance_method(sel!(canBecomeKeyWindow))
        {
            return Err(
                "Own window route requires Tao's own main method and inherited key method".into(),
            );
        }
        Ok(method)
    }
    fn verify_own_policy(own: &NSWindow) -> Result<(), String> {
        // Configure before first show; resignMainWindow is a notification callback.
        if own.canBecomeMainWindow() || own.isMainWindow() || !own.canBecomeKeyWindow() {
            return Err(
                "Own window policy must reject main-window status and allow keyboard input".into(),
            );
        }
        Ok(())
    }
    fn install_own_policy(own: &NSWindow, _: MainThreadMarker) -> Result<(), String> {
        if !own.canBecomeKeyWindow() || own.isMainWindow() {
            return Err(
                "Own window route requires a key-eligible window before it becomes main".into(),
            );
        }
        let object: &AnyObject = own;
        let tao = AnyClass::get(c"TaoWindow").ok_or("TaoWindow class is unavailable")?;
        let method = main_window_method(object.class(), tao)?;
        if MAIN_WINDOW_ROUTE.get().is_none() {
            // SAFETY: main_window_method verified the signature and ownership.
            let route = unsafe { MainWindowRoute::new(method)? };
            MAIN_WINDOW_ROUTE
                .set(route)
                .map_err(|_| "Own window route was installed concurrently")?;
        }
        let route = MAIN_WINDOW_ROUTE
            .get()
            .ok_or("Own window route is unavailable")?;
        if !ptr::eq(route.method, method) {
            return Err("Own window main-window Method identity changed".into());
        }
        let receiver = object as *const AnyObject as *mut AnyObject;
        let registered = route.receiver.load(Ordering::Acquire);
        if !registered.is_null() && registered != receiver {
            return Err("Own window route already belongs to another retained Orb".into());
        }
        if registered.is_null() {
            // SAFETY: a live Tauri-owned NSWindow, retained only on the main thread.
            let retained = unsafe { Retained::retain(own as *const NSWindow as *mut NSWindow) }
                .ok_or("Own window could not be retained")?;
            RETAINED_ORB.with(|slot| {
                let mut slot = slot.borrow_mut();
                if slot.is_some() {
                    return Err("Own window retention and route registration disagree");
                }
                *slot = Some(RetainedOrb { window: retained });
                Ok(())
            })?;
        }
        let result = route
            .register(receiver)
            .and_then(|()| verify_own_policy(own));
        if result.is_err() {
            route.withdraw(route.original_imp());
            let retained = RETAINED_ORB.with(|slot| slot.borrow_mut().take());
            drop(retained);
        }
        result
    }

    #[cfg(test)]
    mod policy_tests {
        use super::*;
        use objc2::runtime::ClassBuilder;

        static LAST_RECEIVER: AtomicPtr<AnyObject> = AtomicPtr::new(ptr::null_mut());
        extern "C" fn original(receiver: *mut AnyObject, selector: Sel) -> Bool {
            LAST_RECEIVER.store(receiver, Ordering::Relaxed);
            Bool::from(selector == sel!(canBecomeMainWindow))
        }
        extern "C" fn foreign(_: *mut AnyObject, _: Sel) -> Bool {
            Bool::YES
        }

        #[test]
        fn main_route_preserves_kvo_other_receivers_repeat_and_rollback() {
            // No NSWindow instances are created off AppKit's main thread. These
            // private test classes exercise the real runtime Method operations.
            let mut base =
                ClassBuilder::new(c"ContextOrbRouteTestBase", NSWindow::class()).unwrap();
            unsafe {
                base.add_method(
                    sel!(canBecomeMainWindow),
                    original as extern "C" fn(*mut AnyObject, Sel) -> Bool,
                );
            }
            let base = base.register();
            let kvo = ClassBuilder::new(c"NSKVONotifying_ContextOrbRouteTestBase", base)
                .unwrap()
                .register();
            let method = main_window_method(kvo, base).unwrap();
            let key = kvo.instance_method(sel!(canBecomeKeyWindow));
            let events = kvo.instance_method(sel!(sendEvent:));
            let route = unsafe { MainWindowRoute::new(method).unwrap() };
            let original_imp = route.original_imp();
            let mut owner_storage = 0_u8;
            let mut other_storage = 1_u8;
            let owner = ptr::from_mut(&mut owner_storage).cast::<AnyObject>();
            let other = ptr::from_mut(&mut other_storage).cast::<AnyObject>();
            route.register(owner).unwrap();
            route.register(owner).unwrap();
            assert!(route.register(other).is_err());
            assert_eq!(route.receiver.load(Ordering::Acquire), owner);
            assert!(same_imp(route.original_imp(), original_imp));
            assert!(!unsafe { route.answer(owner, sel!(canBecomeMainWindow)) }.as_bool());
            assert!(unsafe { route.answer(other, sel!(canBecomeMainWindow)) }.as_bool());
            assert_eq!(LAST_RECEIVER.load(Ordering::Relaxed), other);
            assert!(!unsafe { route.answer(other, sel!(isVisible)) }.as_bool());
            assert_eq!(kvo.superclass(), Some(base));
            assert_eq!(kvo.instance_method(sel!(canBecomeKeyWindow)), key);
            assert_eq!(kvo.instance_method(sel!(sendEvent:)), events);
            route.withdraw(original_imp);
            assert!(route.receiver.load(Ordering::Acquire).is_null());
            assert!(same_imp(method.implementation(), original_imp));
            assert!(unsafe { route.answer(owner, sel!(canBecomeMainWindow)) }.as_bool());
            assert_eq!(LAST_RECEIVER.load(Ordering::Relaxed), owner);

            route.register(owner).unwrap();
            route.clear_receiver(other);
            assert_eq!(route.receiver.load(Ordering::Acquire), owner);
            route.clear_receiver(owner);
            assert!(route.receiver.load(Ordering::Acquire).is_null());
            assert!(unsafe { route.answer(owner, sel!(canBecomeMainWindow)) }.as_bool());
            route.withdraw(original_imp);
            route.register(owner).unwrap();
            let foreign_imp =
                unsafe { std::mem::transmute::<MainWindowImp, Imp>(foreign as MainWindowImp) };
            unsafe { method.set_implementation(foreign_imp) };
            route.withdraw(original_imp);
            assert!(same_imp(method.implementation(), foreign_imp));
            assert!(route.register(owner).is_err());
            assert!(route.receiver.load(Ordering::Acquire).is_null());
            unsafe { method.set_implementation(original_imp) };
        }
    }

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
        let main_thread = marker()?;
        if window.label() != "orb" {
            return Err("Own window policy is restricted to the Orb window".into());
        }
        let own: &NSWindow = unsafe {
            &*window
                .ns_window()
                .map_err(|_| "Own window is unavailable")?
                .cast()
        };
        // This restores utility-like main-window eligibility. Exclusion from
        // macOS desktop edge tiling still requires a real pointer-drag check.
        install_own_policy(own, main_thread)?;
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
    fn read_window(dict: &CFDictionary<CFString, CFType>) -> Option<OtherWindow> {
        // Read only this explicit whitelist. The system dictionary is never serialized.
        let number = |key| value(dict, key)?.downcast::<CFNumber>()?.to_i64();
        let pid = u32::try_from(number(unsafe { kCGWindowOwnerPID })?).ok()?;
        let id = u32::try_from(number(unsafe { kCGWindowNumber })?).ok()?;
        let layer = number(unsafe { kCGWindowLayer })?;
        if pid == 0 || id == 0 || pid == std::process::id() {
            return None;
        }
        // A missing IsOnscreen key means off-screen, including minimized windows.
        if !value(dict, unsafe { kCGWindowIsOnscreen })
            .and_then(|v| v.downcast::<CFBoolean>())
            .is_some_and(bool::from)
        {
            return None;
        }
        if value(dict, unsafe { kCGWindowAlpha })
            .and_then(|v| v.downcast::<CFNumber>())
            .and_then(|v| v.to_f64())
            .is_some_and(|alpha| alpha <= 0.0)
        {
            return None;
        }
        let bounds = value(dict, unsafe { kCGWindowBounds })
            .and_then(|v| v.downcast::<CFDictionary>())
            .and_then(|d| CGRect::from_dict_representation(&d))?;
        let r = Rect {
            x: bounds.origin.x,
            y: bounds.origin.y,
            width: bounds.size.width,
            height: bounds.size.height,
        };
        if !r.valid() {
            return None;
        }
        let bundle = NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32)
            .and_then(|app| app.bundleIdentifier())
            .map(|id| id.to_string());
        if is_desktop_shell(bundle.as_deref(), layer) {
            return None;
        }
        Some(OtherWindow {
            id: id as u64,
            owner_pid: pid,
            rect: r,
            codex: bundle.as_deref() == Some("com.openai.codex"),
            dockable: layer == 0 && r.width >= 40.0 && r.height >= 40.0,
        })
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
            let dict: CFDictionary<CFString, CFType> =
                unsafe { CFDictionary::wrap_under_get_rule(*raw as _) };
            if let Some(window) = read_window(&dict) {
                output.push(window);
            }
            if output.len() == 128 {
                break;
            }
        }
        Ok(output)
    }
    pub fn window(id: u64) -> Result<Option<OtherWindow>, String> {
        marker()?;
        let Some(id) = u32::try_from(id).ok().filter(|id| *id != 0) else {
            return Ok(None);
        };
        // CGWindow IDs occupy pointer-sized CFArray slots with no CF callbacks.
        // OnScreenOnly would enumerate every window; this API asks for one ID.
        let ids = CFArray::from_copyable(&[id as usize as *const std::ffi::c_void]);
        let raw = unsafe { CGWindowListCreateDescriptionFromArray(ids.as_concrete_TypeRef()) };
        if raw.is_null() {
            return Err("Window geometry is unavailable in this desktop session".into());
        }
        let descriptions: CFArray<CFDictionary<CFString, CFType>> =
            unsafe { CFArray::wrap_under_create_rule(raw) };
        Ok(descriptions
            .iter()
            .filter_map(|dict| read_window(&dict))
            .find(|window| window.id == id as u64))
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
    unsafe fn read_window(handle: HWND) -> Option<OtherWindow> {
        let mut pid = 0;
        if IsWindow(handle) == 0
            || GetAncestor(handle, GA_ROOT) != handle
            || GetWindowThreadProcessId(handle, &mut pid) == 0
            || pid == 0
            || pid == std::process::id()
            || IsWindowVisible(handle) == 0
            || IsIconic(handle) != 0
        {
            return None;
        }
        let mut cloaked: u32 = 0;
        let cloak_status = DwmGetWindowAttribute(
            handle,
            DWMWA_CLOAKED as u32,
            (&mut cloaked as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
        );
        if cloak_status >= 0 && cloaked != 0 {
            return None;
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
                return None;
            }
        }
        let r = rect(bounds);
        if !r.valid() {
            return None;
        }
        SetLastError(0);
        let style = GetWindowLongPtrW(handle, GWL_EXSTYLE);
        let style_known = style != 0 || GetLastError() == 0;
        Some(OtherWindow {
            id: handle as usize as u64,
            owner_pid: pid,
            rect: r,
            codex: false,
            dockable: cloak_status >= 0
                && style_known
                && (style as u32 & WS_EX_TOOLWINDOW) == 0
                && r.width >= 40.0
                && r.height >= 40.0,
        })
    }
    unsafe extern "system" fn collect(handle: HWND, data: LPARAM) -> i32 {
        let output = &mut *(data as *mut Vec<OtherWindow>);
        if output.len() < 128 {
            if let Some(window) = read_window(handle) {
                output.push(window);
            }
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
    pub fn window(id: u64) -> Result<Option<OtherWindow>, String> {
        let Some(id) = usize::try_from(id).ok().filter(|id| *id != 0) else {
            return Ok(None);
        };
        Ok(unsafe { read_window(id as HWND) })
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
    pub fn window(_: u64) -> Result<Option<OtherWindow>, String> {
        Err("Unsupported platform".into())
    }
}

pub use os::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::magnet_geometry::{orb_box, place_widget};

    #[test]
    fn only_the_observed_dock_shell_is_exempt_from_window_occlusion() {
        assert!(is_desktop_shell(Some("com.apple.dock"), 20));
        for (bundle, layer) in [
            (Some("com.openai.codex"), 0),
            (Some("com.example.popup"), 20),
            (Some("com.apple.dock"), 101),
            (Some("com.apple.dock"), 0),
            (None, 20),
        ] {
            assert!(!is_desktop_shell(bundle, layer));
        }
        let desktop = Rect {
            x: 0.,
            y: 0.,
            width: 1470.,
            height: 956.,
        };
        let codex = Rect {
            x: 89.,
            y: 67.,
            width: 1311.,
            height: 790.,
        };
        let visible = |items: &[(Option<&'static str>, i64, Rect)], x, y| {
            items
                .iter()
                .find(|(bundle, layer, rect)| {
                    !is_desktop_shell(*bundle, *layer) && rect.contains_closed(x, y)
                })
                .map(|item| item.0)
        };
        let windows = [
            (Some("com.apple.dock"), 20, desktop),
            (Some("com.openai.codex"), 0, codex),
        ];
        for (x, y) in [(89., 400.), (1400., 400.), (500., 67.), (500., 857.)] {
            assert_eq!(visible(&windows, x, y), Some(Some("com.openai.codex")));
        }
        let popup = [
            (Some("com.example.popup"), 20, desktop),
            windows[0],
            windows[1],
        ];
        assert_eq!(visible(&popup, 89., 400.), Some(Some("com.example.popup")));
    }

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
