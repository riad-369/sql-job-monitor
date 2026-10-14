//! Window chrome, initial placement, URL opening, and the primary modifier.
//! Windows keeps the Win32 caption and work-area sizing. macOS uses a unified
//! title bar beside the traffic lights.

use slint::winit_030::winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    keyboard::ModifiersState,
    window::Window,
};

/// Logical width the traffic-light buttons occupy at the left of the title strip.
pub const TRAFFIC_LIGHT_INSET: f32 = 78.0;

/// Logical space reserved for the menu bar (including notched displays) and the Dock,
/// because winit reports the full monitor rather than the visible frame.
const MENU_BAR_HEIGHT: f64 = 38.0;
const DOCK_ALLOWANCE: f64 = 72.0;

/// Selects the winit backend. On macOS the content extends under a transparent
/// title bar. Windows keeps Slint's default backend setup.
pub fn select_backend() {
    #[cfg(not(target_os = "windows"))]
    {
        let selector = slint::BackendSelector::new().backend_name("winit".into());
        #[cfg(target_os = "macos")]
        let selector = selector.with_winit_window_attributes_hook(|attributes| {
            use slint::winit_030::winit::platform::macos::WindowAttributesExtMacOS;
            attributes
                .with_titlebar_transparent(true)
                .with_title_hidden(true)
                .with_fullsize_content_view(true)
        });
        // A platform may already be installed (for example by tests); keep it.
        let _ = selector.select();
    }
}

/// True when the native title bar is merged into the window content.
pub fn uses_unified_title_bar() -> bool {
    cfg!(target_os = "macos")
}

/// Command on macOS, Control on Windows.
pub fn primary_modifier(state: ModifiersState) -> bool {
    if cfg!(target_os = "macos") {
        state.super_key()
    } else {
        state.control_key()
    }
}

pub fn open_in_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        open_in_browser_windows(url);
    }
    #[cfg(target_os = "macos")]
    {
        spawn_opener("/usr/bin/open", url);
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        spawn_opener("xdg-open", url);
    }
}

#[cfg(not(target_os = "windows"))]
fn spawn_opener(program: &str, url: &str) {
    let _ = std::process::Command::new(program)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(target_os = "windows")]
fn open_in_browser_windows(url: &str) {
    use std::os::windows::ffi::OsStrExt;
    let operation: Vec<u16> = std::ffi::OsStr::new("open")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let file: Vec<u16> = std::ffi::OsStr::new(url)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // ShellExecuteW hands the URL to the default browser and never creates a console.
    let _ = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
}

#[cfg(target_os = "windows")]
#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteW(
        hwnd: *mut core::ffi::c_void,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> isize;
}

/// Dark caption colors on Windows. Other platforms draw their own title bar.
pub fn apply_caption(window: &Window) -> bool {
    #[cfg(target_os = "windows")]
    {
        apply_dark_title(window)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        true
    }
}

#[cfg(target_os = "windows")]
fn apply_dark_title(window: &Window) -> bool {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };
    let hwnd = win32.hwnd.get();
    unsafe {
        // 19 is the Windows 10 value; 20 is the current dark-mode attribute.
        set_caption_attribute(hwnd, 19, 1);
        set_caption_attribute(hwnd, 20, 1);
        set_caption_attribute(hwnd, 35, color_ref(0x10, 0x15, 0x1D));
        set_caption_attribute(hwnd, 34, color_ref(0x2A, 0x39, 0x4B));
        set_caption_attribute(hwnd, 36, color_ref(0xE9, 0xEE, 0xF5));
    }
    true
}

#[cfg(target_os = "windows")]
fn color_ref(red: u32, green: u32, blue: u32) -> u32 {
    red | (green << 8) | (blue << 16)
}

#[cfg(target_os = "windows")]
struct WinWorkArea {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[cfg(target_os = "windows")]
fn monitor_work_area(window: &Window) -> Option<WinWorkArea> {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = window.window_handle().ok()?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return None;
    };
    unsafe { work_area_from_hwnd(win32.hwnd.get()) }
}

#[cfg(target_os = "windows")]
unsafe fn work_area_from_hwnd(hwnd: isize) -> Option<WinWorkArea> {
    #[repr(C)]
    struct WinRect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[repr(C)]
    struct MonitorInfo {
        size: u32,
        monitor: WinRect,
        work: WinRect,
        flags: u32,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MonitorFromWindow(hwnd: isize, flags: u32) -> isize;
        fn GetMonitorInfoW(monitor: isize, info: *mut MonitorInfo) -> i32;
    }

    unsafe {
        let monitor = MonitorFromWindow(hwnd, 2);
        if monitor == 0 {
            return None;
        }
        let mut info = MonitorInfo {
            size: size_of::<MonitorInfo>() as u32,
            monitor: WinRect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            work: WinRect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            flags: 0,
        };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        Some(WinWorkArea {
            left: info.work.left,
            top: info.work.top,
            right: info.work.right,
            bottom: info.work.bottom,
        })
    }
}

#[cfg(target_os = "windows")]
unsafe fn set_caption_attribute(hwnd: isize, attribute: u32, value: u32) {
    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: isize,
            dw_attribute: u32,
            pv_attribute: *const u32,
            cb_attribute: u32,
        ) -> i32;
    }
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, attribute, &value, size_of::<u32>() as u32);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkArea {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

/// The usable part of a monitor once the menu bar and Dock allowance are removed.
pub fn work_area(position: PhysicalPosition<i32>, size: PhysicalSize<u32>, scale: f64) -> WorkArea {
    let menu = (MENU_BAR_HEIGHT * scale).round() as u32;
    let dock = (DOCK_ALLOWANCE * scale).round() as u32;
    WorkArea {
        left: position.x,
        top: position.y + menu as i32,
        width: size.width,
        height: size.height.saturating_sub(menu + dock),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub position: PhysicalPosition<i32>,
    pub inner: PhysicalSize<u32>,
    pub min_logical: (f32, f32),
}

/// Centers a 1440×860 logical window in the work area, shrinking it on small displays.
pub fn placement(work: WorkArea, frame: PhysicalSize<u32>, scale: f64) -> Placement {
    let logical_w = f64::from(work.width) / scale;
    let logical_h = f64::from(work.height) / scale;
    let min_logical = (
        (logical_w - 32.0).clamp(320.0, 800.0) as f32,
        (logical_h - 48.0).clamp(240.0, 480.0) as f32,
    );

    let preferred_w = (1440.0 * scale).round() as u32;
    let preferred_h = (860.0 * scale).round() as u32;
    let max_inner_w = work.width.saturating_sub(frame.width).saturating_sub(24);
    let max_inner_h = work.height.saturating_sub(frame.height).saturating_sub(24);
    let inner_w = preferred_w.min(max_inner_w).max(1);
    let inner_h = preferred_h.min(max_inner_h).max(1);
    let outer_w = (inner_w + frame.width) as i32;
    let outer_h = (inner_h + frame.height) as i32;
    let x = work.left + (work.width as i32 - outer_w) / 2;
    let y = work.top + (work.height as i32 - outer_h) / 2;
    Placement {
        position: PhysicalPosition::new(x, y),
        inner: PhysicalSize::new(inner_w, inner_h),
        min_logical,
    }
}

/// Returns false until the window has a size, so the caller retries on the next frame.
pub fn fit_window_to_monitor(window: &Window, set_min: impl FnOnce(f32, f32)) -> bool {
    #[cfg(target_os = "windows")]
    {
        fit_window_windows(window, set_min)
    }
    #[cfg(not(target_os = "windows"))]
    {
        fit_window_macos(window, set_min)
    }
}

#[cfg(target_os = "windows")]
fn fit_window_windows(window: &Window, set_min: impl FnOnce(f32, f32)) -> bool {
    let inner = window.inner_size();
    if inner.width == 0 || inner.height == 0 {
        return false;
    }
    let Some(work) = monitor_work_area(window) else {
        return true;
    };

    let scale = window.scale_factor();
    let outer = window.outer_size();
    let frame_w = outer.width.saturating_sub(inner.width);
    let frame_h = outer.height.saturating_sub(inner.height);
    let work_w = work.right.saturating_sub(work.left);
    let work_h = work.bottom.saturating_sub(work.top);
    let logical_w = work_w as f64 / scale;
    let logical_h = work_h as f64 / scale;
    set_min(
        (logical_w - 32.0).clamp(320.0, 800.0) as f32,
        (logical_h - 48.0).clamp(240.0, 480.0) as f32,
    );

    let preferred_w = (1440.0 * scale).round() as u32;
    let preferred_h = (860.0 * scale).round() as u32;
    let max_inner_w = (work_w as u32).saturating_sub(frame_w).saturating_sub(24);
    let max_inner_h = (work_h as u32).saturating_sub(frame_h).saturating_sub(24);
    let inner_w = preferred_w.min(max_inner_w).max(1);
    let inner_h = preferred_h.min(max_inner_h).max(1);
    let outer_w = inner_w + frame_w;
    let outer_h = inner_h + frame_h;
    let x = work.left + (work_w - outer_w as i32) / 2;
    let y = work.top + (work_h - outer_h as i32) / 2;

    window.set_outer_position(PhysicalPosition::new(x, y));
    let _ = window.request_inner_size(PhysicalSize::new(inner_w, inner_h));
    true
}

#[cfg(not(target_os = "windows"))]
fn fit_window_macos(window: &Window, set_min: impl FnOnce(f32, f32)) -> bool {
    let inner = window.inner_size();
    if inner.width == 0 || inner.height == 0 {
        return false;
    }
    let Some(monitor) = window.current_monitor() else {
        return true;
    };
    let scale = window.scale_factor();
    let outer = window.outer_size();
    let frame = PhysicalSize::new(
        outer.width.saturating_sub(inner.width),
        outer.height.saturating_sub(inner.height),
    );
    let fit = placement(
        work_area(monitor.position(), monitor.size(), scale),
        frame,
        scale,
    );
    set_min(fit.min_logical.0, fit.min_logical.1);
    window.set_outer_position(fit.position);
    let _ = window.request_inner_size(fit.inner);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_preferred_size_on_a_large_retina_display() {
        let work = work_area(
            PhysicalPosition::new(0, 0),
            PhysicalSize::new(5120, 2880),
            2.0,
        );
        assert_eq!(work.top, 76);
        let fit = placement(work, PhysicalSize::new(0, 0), 2.0);
        assert_eq!(fit.inner, PhysicalSize::new(2880, 1720));
        assert_eq!(fit.position.x, (5120 - 2880) / 2);
        assert!(fit.position.y >= work.top);
        assert_eq!(fit.min_logical, (800.0, 480.0));
    }

    #[test]
    fn shrinks_to_fit_a_small_laptop_display() {
        let work = work_area(
            PhysicalPosition::new(0, 0),
            PhysicalSize::new(2560, 1600),
            2.0,
        );
        let fit = placement(work, PhysicalSize::new(0, 0), 2.0);
        assert!(fit.inner.width <= work.width - 24);
        assert!(fit.inner.height <= work.height - 24);
        assert!(fit.position.y >= work.top);
        let bottom = fit.position.y + fit.inner.height as i32;
        assert!(bottom <= work.top + work.height as i32);
    }

    #[test]
    fn respects_a_secondary_monitor_origin() {
        let work = work_area(
            PhysicalPosition::new(-1920, 0),
            PhysicalSize::new(1920, 1080),
            1.0,
        );
        let fit = placement(work, PhysicalSize::new(0, 0), 1.0);
        assert!(fit.position.x >= -1920 && fit.position.x < 0);
    }
}
