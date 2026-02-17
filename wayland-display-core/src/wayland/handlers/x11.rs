//! X11/Xwayland window handler for gst-wayland-display compositor
//!
//! This module implements `XwmHandler` to support X11 applications via Xwayland.
//! Each X11 window becomes a separate surface that can be routed to different
//! outputs using the multi-output logic.
//!
//! Key design: X11 windows are routed using the SAME logic as Wayland toplevels:
//! - First window → primary space (HEADLESS-1)
//! - Second window → secondary space (HEADLESS-2)
//!
//! This enables dual-screen emulators (Azahar, melonDS) to work without modification.

use std::os::unix::io::OwnedFd;

use smithay::{
    delegate_xwayland_shell,
    desktop::{Window, WindowSurface},
    utils::{Logical, Rectangle, SERIAL_COUNTER},
    wayland::{
        selection::{
            data_device::{
                clear_data_device_selection, current_data_device_selection_userdata,
                request_data_device_client_selection, set_data_device_selection,
            },
            SelectionTarget,
        },
        xwayland_shell::{XWaylandShellHandler, XWaylandShellState},
    },
    xwayland::{
        xwm::{Reorder, ResizeEdge as X11ResizeEdge, XwmId},
        X11Surface, X11Wm, XwmHandler,
    },
};
use tracing::{debug, error, info, trace, warn};

use crate::comp::{FocusTarget, State};

/// Helper to check if a Window wraps a specific X11Surface
fn window_matches_x11(window: &Window, x11_surface: &X11Surface) -> bool {
    match window.underlying_surface() {
        WindowSurface::X11(s) => *s == *x11_surface,
        WindowSurface::Wayland(_) => false,
    }
}

/// Helper to extract X11Surface from a Window if it wraps one
fn get_x11_surface(window: &Window) -> Option<X11Surface> {
    match window.underlying_surface() {
        WindowSurface::X11(s) => Some(s.clone()),
        WindowSurface::Wayland(_) => None,
    }
}

// Implement XWaylandShellHandler for the xwayland-shell protocol
impl XWaylandShellHandler for State {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.xwayland_shell_state
    }
}

delegate_xwayland_shell!(State);

impl XwmHandler for State {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut X11Wm {
        self.xwm.as_mut().expect("X11Wm not initialized")
    }

    fn new_window(&mut self, _xwm: XwmId, window: X11Surface) {
        debug!("New X11 window created: {:?}", window.window_id());
    }

    fn new_override_redirect_window(&mut self, _xwm: XwmId, window: X11Surface) {
        debug!(
            "New X11 override_redirect window: {:?}",
            window.window_id()
        );
    }

    fn map_window_request(&mut self, _xwm: XwmId, window: X11Surface) {
        // Allow the window to be mapped
        if let Err(e) = window.set_mapped(true) {
            error!("Failed to set X11 window mapped: {:?}", e);
            return;
        }

        // Wrap X11 surface in a Window for the desktop space
        let win = Window::new_x11_window(window.clone());

        // Route to primary or secondary space using multi-output logic
        // This is the SAME logic as for Wayland toplevels in compositor.rs
        let primary_count = self.space.elements().count();
        let secondary_count = self.secondary_space.elements().count();
        let use_secondary = self.multi_output_enabled
            && self.secondary_output.is_some()
            && primary_count >= 1
            && secondary_count == 0;

        let loc = (0, 0);

        if use_secondary {
            info!(
                "Mapping X11 window {:?} to SECONDARY space (multi-output)",
                window.window_id()
            );

            // Configure window size to match secondary output
            if let Some(ref output) = self.secondary_output {
                if let Some(mode) = output.current_mode() {
                    let size = mode
                        .size
                        .to_f64()
                        .to_logical(output.current_scale().fractional_scale())
                        .to_i32_round();
                    let geo = Rectangle::from_loc_and_size(loc, size);
                    if let Err(e) = window.configure(Some(geo)) {
                        warn!("Failed to configure X11 window for secondary output: {:?}", e);
                    }
                }
            }

            self.secondary_space.map_element(win.clone(), loc, true);
        } else {
            info!(
                "Mapping X11 window {:?} to PRIMARY space",
                window.window_id()
            );

            // Configure window size to match primary output
            if let Some(ref output) = self.output {
                if let Some(mode) = output.current_mode() {
                    let size = mode
                        .size
                        .to_f64()
                        .to_logical(output.current_scale().fractional_scale())
                        .to_i32_round();
                    let geo = Rectangle::from_loc_and_size(loc, size);
                    if let Err(e) = window.configure(Some(geo)) {
                        warn!("Failed to configure X11 window for primary output: {:?}", e);
                    }
                }
            }

            self.space.map_element(win.clone(), loc, true);
        }

        // Track toplevel count for routing future windows
        self.toplevel_count += 1;

        // Give keyboard focus to the new window
        self.seat.get_keyboard().unwrap().set_focus(
            self,
            Some(FocusTarget::from(win)),
            SERIAL_COUNTER.next_serial(),
        );
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, window: X11Surface) {
        // Override redirect windows (like menus, tooltips) are always mapped to primary space
        // at their requested location
        let location = window.geometry().loc;
        let win = Window::new_x11_window(window);
        self.space.map_element(win, location, true);
    }

    fn unmapped_window(&mut self, _xwm: XwmId, window: X11Surface) {
        debug!("X11 window unmapped: {:?}", window.window_id());

        // Find and remove from primary space
        let maybe_primary = self
            .space
            .elements()
            .find(|e| window_matches_x11(e, &window))
            .cloned();
        if let Some(elem) = maybe_primary {
            self.space.unmap_elem(&elem);
        }

        // Find and remove from secondary space
        let maybe_secondary = self
            .secondary_space
            .elements()
            .find(|e| window_matches_x11(e, &window))
            .cloned();
        if let Some(elem) = maybe_secondary {
            self.secondary_space.unmap_elem(&elem);
        }

        if !window.is_override_redirect() {
            if let Err(e) = window.set_mapped(false) {
                warn!("Failed to set X11 window unmapped: {:?}", e);
            }
        }
    }

    fn destroyed_window(&mut self, _xwm: XwmId, window: X11Surface) {
        debug!("X11 window destroyed: {:?}", window.window_id());
        // Window cleanup is handled by unmapped_window
    }

    fn configure_request(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        _x: Option<i32>,
        _y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _reorder: Option<Reorder>,
    ) {
        // Allow size changes but not position (we control positioning)
        let mut geo = window.geometry();
        if let Some(w) = w {
            geo.size.w = w as i32;
        }
        if let Some(h) = h {
            geo.size.h = h as i32;
        }
        let _ = window.configure(geo);
    }

    fn configure_notify(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        geometry: Rectangle<i32, Logical>,
        _above: Option<u32>,
    ) {
        // Update element position if it changed
        // Clone the element first to avoid borrow issues
        let primary_elem = self
            .space
            .elements()
            .find(|e| window_matches_x11(e, &window))
            .cloned();
        if let Some(elem) = primary_elem {
            self.space.map_element(elem, geometry.loc, false);
        }

        let secondary_elem = self
            .secondary_space
            .elements()
            .find(|e| window_matches_x11(e, &window))
            .cloned();
        if let Some(elem) = secondary_elem {
            self.secondary_space.map_element(elem, geometry.loc, false);
        }
    }

    fn maximize_request(&mut self, _xwm: XwmId, window: X11Surface) {
        // For streaming, we typically want fullscreen anyway, so just maximize
        if let Err(e) = window.set_maximized(true) {
            warn!("Failed to maximize X11 window: {:?}", e);
        }
    }

    fn unmaximize_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Err(e) = window.set_maximized(false) {
            warn!("Failed to unmaximize X11 window: {:?}", e);
        }
    }

    fn fullscreen_request(&mut self, _xwm: XwmId, window: X11Surface) {
        // Check if window is in primary or secondary space and fullscreen accordingly
        let is_secondary = self
            .secondary_space
            .elements()
            .any(|e| window_matches_x11(e, &window));

        let output = if is_secondary {
            self.secondary_output.as_ref()
        } else {
            self.output.as_ref()
        };

        if let Some(output) = output {
            if let Some(geometry) = output.current_mode().map(|m| {
                Rectangle::from_loc_and_size(
                    (0, 0),
                    m.size
                        .to_f64()
                        .to_logical(output.current_scale().fractional_scale())
                        .to_i32_round(),
                )
            }) {
                if let Err(e) = window.set_fullscreen(true) {
                    warn!("Failed to set X11 window fullscreen: {:?}", e);
                }
                if let Err(e) = window.configure(geometry) {
                    warn!("Failed to configure X11 window geometry: {:?}", e);
                }
            }
        }
    }

    fn unfullscreen_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Err(e) = window.set_fullscreen(false) {
            warn!("Failed to unfullscreen X11 window: {:?}", e);
        }
    }

    fn resize_request(
        &mut self,
        _xwm: XwmId,
        _window: X11Surface,
        _button: u32,
        _edges: X11ResizeEdge,
    ) {
        // We don't support interactive resize for streaming compositor
        // Windows are sized to output dimensions
    }

    fn move_request(&mut self, _xwm: XwmId, _window: X11Surface, _button: u32) {
        // We don't support interactive move for streaming compositor
        // Windows are positioned at (0,0) on their output
    }

    fn allow_selection_access(&mut self, xwm: XwmId, _selection: SelectionTarget) -> bool {
        // Allow clipboard access when an X11 window is focused
        if let Some(keyboard) = self.seat.get_keyboard() {
            if let Some(FocusTarget::Window(w)) = keyboard.current_focus() {
                if let Some(surface) = get_x11_surface(&w) {
                    if surface.xwm_id().unwrap() == xwm {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn send_selection(
        &mut self,
        _xwm: XwmId,
        selection: SelectionTarget,
        mime_type: String,
        fd: OwnedFd,
    ) {
        match selection {
            SelectionTarget::Clipboard => {
                if let Err(err) = request_data_device_client_selection(&self.seat, mime_type, fd) {
                    error!(
                        ?err,
                        "Failed to request current wayland clipboard for Xwayland"
                    );
                }
            }
            SelectionTarget::Primary => {
                // Primary selection not commonly used in our streaming context
                trace!("Primary selection request ignored");
            }
        }
    }

    fn new_selection(&mut self, _xwm: XwmId, selection: SelectionTarget, mime_types: Vec<String>) {
        trace!(?selection, ?mime_types, "Got Selection from X11");
        if selection == SelectionTarget::Clipboard {
            set_data_device_selection(&self.dh, &self.seat, mime_types, ())
        }
    }

    fn cleared_selection(&mut self, _xwm: XwmId, selection: SelectionTarget) {
        if selection == SelectionTarget::Clipboard {
            if current_data_device_selection_userdata(&self.seat).is_some() {
                clear_data_device_selection(&self.dh, &self.seat)
            }
        }
    }

    fn disconnected(&mut self, _xwm: XwmId) {
        warn!("Xwayland disconnected");
        self.xwm = None;
    }
}
