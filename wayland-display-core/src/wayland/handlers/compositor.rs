use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_single_pixel_buffer,
    desktop::PopupKind,
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel::State as XdgState,
        wayland_server::{
            Client,
            protocol::{wl_buffer::WlBuffer, wl_surface::WlSurface},
        },
    },
    utils::SERIAL_COUNTER,
    wayland::{
        buffer::BufferHandler,
        compositor::{CompositorClientState, CompositorHandler, CompositorState, with_states},
        seat::WaylandFocus,
        shell::xdg::{SurfaceCachedState, XdgPopupSurfaceData, XdgToplevelSurfaceData},
    },
};

use crate::comp::{ClientState, FocusTarget, State};

impl BufferHandler for State {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl CompositorHandler for State {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);

        if let Some(window) = self
            .space
            .elements()
            .find(|w| w.wl_surface().map(|s| &*s == surface).unwrap_or(false))
        {
            window.on_commit();
        }
        // Also check secondary space for multi-output mode
        if let Some(window) = self
            .secondary_space
            .elements()
            .find(|w| w.wl_surface().map(|s| &*s == surface).unwrap_or(false))
        {
            window.on_commit();
        }
        self.popups.commit(surface);

        // send the initial configure if relevant
        if let Some(idx) = self
            .pending_windows
            .iter_mut()
            .position(|w| w.wl_surface().map(|s| &*s == surface).unwrap_or(false))
        {
            let window = self.pending_windows.swap_remove(idx);

            let toplevel = window.toplevel().unwrap();
            let (initial_configure_sent, max_size) = with_states(surface, |states| {
                let attributes = states.data_map.get::<XdgToplevelSurfaceData>().unwrap();
                let attributes_guard = attributes.lock().unwrap();

                (
                    attributes_guard.initial_configure_sent,
                    states
                        .cached_state
                        .get::<SurfaceCachedState>()
                        .current()
                        .max_size,
                )
            });

            if self.output.is_none() {
                return;
            }

            if !initial_configure_sent {
                // Determine which output to use for sizing this toplevel.
                // In multi-output mode, route first live toplevel to primary space
                // and second live toplevel to secondary space.
                let primary_count = self.space.elements().count();
                let secondary_count = self.secondary_space.elements().count();
                let use_secondary = self.multi_output_enabled
                    && self.secondary_output.is_some()
                    && primary_count >= 1
                    && secondary_count == 0;

                let target_output = if use_secondary {
                    self.secondary_output.as_ref().unwrap()
                } else {
                    self.output.as_ref().unwrap()
                };

                if max_size.w == 0 && max_size.h == 0 {
                    toplevel.with_pending_state(|state| {
                        state.size = Some(
                            target_output
                                .current_mode()
                                .unwrap()
                                .size
                                .to_f64()
                                .to_logical(
                                    target_output
                                        .current_scale()
                                        .fractional_scale(),
                                )
                                .to_i32_round(),
                        );
                        state.states.set(XdgState::Fullscreen);
                    });
                }
                toplevel.with_pending_state(|state| {
                    state.states.set(XdgState::Activated);
                });
                toplevel.send_configure();
                self.pending_windows.push(window);
            } else {
                // Determine which space to map into.
                // In multi-output mode, route first live toplevel to primary space
                // and second live toplevel to secondary space.
                let primary_count = self.space.elements().count();
                let secondary_count = self.secondary_space.elements().count();
                let use_secondary = self.multi_output_enabled
                    && self.secondary_output.is_some()
                    && primary_count >= 1
                    && secondary_count == 0;

                let loc = (0, 0);
                if use_secondary {
                    tracing::info!("Mapping toplevel to secondary space");
                    self.secondary_space.map_element(window.clone(), loc, true);
                } else {
                    tracing::info!("Mapping toplevel to primary space");
                    self.space.map_element(window.clone(), loc, true);
                }
                self.toplevel_count += 1;

                self.seat.get_keyboard().unwrap().set_focus(
                    self,
                    Some(FocusTarget::from(window)),
                    SERIAL_COUNTER.next_serial(),
                );
            }

            return;
        }

        if let Some(popup) = self.popups.find_popup(surface) {
            let PopupKind::Xdg(ref popup) = popup else {
                // Our compositor doesn't do input handling in the popup code
                unreachable!()
            };
            let initial_configure_sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<XdgPopupSurfaceData>()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .initial_configure_sent
            });
            if !initial_configure_sent {
                // NOTE: This should never fail as the initial configure is always
                // allowed.
                popup.send_configure().expect("initial configure failed");
            }

            return;
        };
    }
}

delegate_compositor!(State);
delegate_single_pixel_buffer!(State);
