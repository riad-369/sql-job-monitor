//! macOS integration: native window chrome, initial window placement, URL opening,
//! and the Command-key modifier. Everything else in the app is platform-neutral.

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

/// Selects the winit backend and, on macOS, extends the content under a transparent
/// title bar so the app's own title strip sits beside the native traffic lights.
pub fn select_backend() {
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

/// True when the native title bar is merged into the window content.
pub fn uses_unified_title_bar() -> bool {
    cfg!(target_os = "macos")
}

/// Command on macOS, Control elsewhere (only used for development builds on Linux).
pub fn primary_modifier(state: ModifiersState) -> bool {
    if cfg!(target_os = "macos") {
        state.super_key()
    } else {
        state.control_key()
    }
}

pub fn open_in_browser(url: &str) {
    let opener = if cfg!(target_os = "macos") {
        "/usr/bin/open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(opener)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
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
