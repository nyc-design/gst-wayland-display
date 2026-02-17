use crate::comp::State;
use smithay::{
    backend::input::KeyState,
    desktop::{PopupKind, Window, WindowSurface},
    input::{
        Seat,
        keyboard::{KeyboardTarget, KeysymHandle, ModifiersState},
        pointer::{
            AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent,
            GesturePinchBeginEvent, GesturePinchEndEvent, GesturePinchUpdateEvent,
            GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent, MotionEvent,
            PointerTarget, RelativeMotionEvent,
        },
        touch::{DownEvent, OrientationEvent, ShapeEvent, TouchTarget, UpEvent},
    },
    reexports::wayland_server::{backend::ObjectId, protocol::wl_surface::WlSurface},
    utils::{IsAlive, Serial},
    wayland::seat::WaylandFocus,
};
use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq)]
pub enum FocusTarget {
    Window(Window),
    Popup(PopupKind),
}

impl IsAlive for FocusTarget {
    fn alive(&self) -> bool {
        match self {
            FocusTarget::Window(w) => w.alive(),
            FocusTarget::Popup(p) => p.alive(),
        }
    }
}

impl From<Window> for FocusTarget {
    fn from(w: Window) -> Self {
        FocusTarget::Window(w)
    }
}

impl From<PopupKind> for FocusTarget {
    fn from(p: PopupKind) -> Self {
        FocusTarget::Popup(p)
    }
}

impl KeyboardTarget<State> for FocusTarget {
    fn enter(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        keys: Vec<KeysymHandle<'_>>,
        serial: Serial,
    ) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    KeyboardTarget::enter(wl.wl_surface(), seat, data, keys, serial)
                }
                WindowSurface::X11(x11) => {
                    KeyboardTarget::enter(x11.clone(), seat, data, keys, serial)
                }
            },
            FocusTarget::Popup(p) => {
                KeyboardTarget::enter(p.wl_surface(), seat, data, keys, serial)
            }
        }
    }

    fn leave(&self, seat: &Seat<State>, data: &mut State, serial: Serial) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    KeyboardTarget::leave(wl.wl_surface(), seat, data, serial)
                }
                WindowSurface::X11(x11) => KeyboardTarget::leave(x11.clone(), seat, data, serial),
            },
            FocusTarget::Popup(p) => KeyboardTarget::leave(p.wl_surface(), seat, data, serial),
        }
    }

    fn key(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        key: KeysymHandle<'_>,
        state: KeyState,
        serial: Serial,
        time: u32,
    ) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    KeyboardTarget::key(wl.wl_surface(), seat, data, key, state, serial, time)
                }
                WindowSurface::X11(x11) => {
                    KeyboardTarget::key(x11.clone(), seat, data, key, state, serial, time)
                }
            },
            FocusTarget::Popup(p) => {
                KeyboardTarget::key(p.wl_surface(), seat, data, key, state, serial, time)
            }
        }
    }

    fn modifiers(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        modifiers: ModifiersState,
        serial: Serial,
    ) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    KeyboardTarget::modifiers(wl.wl_surface(), seat, data, modifiers, serial)
                }
                WindowSurface::X11(x11) => {
                    KeyboardTarget::modifiers(x11.clone(), seat, data, modifiers, serial)
                }
            },
            FocusTarget::Popup(p) => p.wl_surface().modifiers(seat, data, modifiers, serial),
        }
    }
}

impl PointerTarget<State> for FocusTarget {
    fn enter(&self, seat: &Seat<State>, data: &mut State, event: &MotionEvent) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    PointerTarget::enter(wl.wl_surface(), seat, data, event)
                }
                WindowSurface::X11(x11) => PointerTarget::enter(x11.clone(), seat, data, event),
            },
            FocusTarget::Popup(p) => PointerTarget::enter(p.wl_surface(), seat, data, event),
        }
    }

    fn motion(&self, seat: &Seat<State>, data: &mut State, event: &MotionEvent) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    PointerTarget::motion(wl.wl_surface(), seat, data, event)
                }
                WindowSurface::X11(x11) => PointerTarget::motion(x11.clone(), seat, data, event),
            },
            FocusTarget::Popup(p) => PointerTarget::motion(p.wl_surface(), seat, data, event),
        }
    }

    fn relative_motion(&self, seat: &Seat<State>, data: &mut State, event: &RelativeMotionEvent) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    PointerTarget::relative_motion(wl.wl_surface(), seat, data, event)
                }
                WindowSurface::X11(x11) => {
                    PointerTarget::relative_motion(x11.clone(), seat, data, event)
                }
            },
            FocusTarget::Popup(p) => {
                PointerTarget::relative_motion(p.wl_surface(), seat, data, event)
            }
        }
    }

    fn button(&self, seat: &Seat<State>, data: &mut State, event: &ButtonEvent) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    PointerTarget::button(wl.wl_surface(), seat, data, event)
                }
                WindowSurface::X11(x11) => PointerTarget::button(x11.clone(), seat, data, event),
            },
            FocusTarget::Popup(p) => PointerTarget::button(p.wl_surface(), seat, data, event),
        }
    }

    fn axis(&self, seat: &Seat<State>, data: &mut State, frame: AxisFrame) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    PointerTarget::axis(wl.wl_surface(), seat, data, frame)
                }
                WindowSurface::X11(x11) => PointerTarget::axis(x11.clone(), seat, data, frame),
            },
            FocusTarget::Popup(p) => PointerTarget::axis(p.wl_surface(), seat, data, frame),
        }
    }

    fn frame(&self, seat: &Seat<State>, data: &mut State) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => PointerTarget::frame(wl.wl_surface(), seat, data),
                WindowSurface::X11(x11) => PointerTarget::frame(x11.clone(), seat, data),
            },
            FocusTarget::Popup(p) => PointerTarget::frame(p.wl_surface(), seat, data),
        }
    }

    fn gesture_swipe_begin(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &GestureSwipeBeginEvent,
    ) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    PointerTarget::gesture_swipe_begin(wl.wl_surface(), seat, data, event)
                }
                WindowSurface::X11(x11) => {
                    PointerTarget::gesture_swipe_begin(x11.clone(), seat, data, event)
                }
            },
            FocusTarget::Popup(p) => {
                PointerTarget::gesture_swipe_begin(p.wl_surface(), seat, data, event)
            }
        }
    }

    fn gesture_swipe_update(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &GestureSwipeUpdateEvent,
    ) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    PointerTarget::gesture_swipe_update(wl.wl_surface(), seat, data, event)
                }
                WindowSurface::X11(x11) => {
                    PointerTarget::gesture_swipe_update(x11.clone(), seat, data, event)
                }
            },
            FocusTarget::Popup(p) => {
                PointerTarget::gesture_swipe_update(p.wl_surface(), seat, data, event)
            }
        }
    }

    fn gesture_swipe_end(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &GestureSwipeEndEvent,
    ) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    PointerTarget::gesture_swipe_end(wl.wl_surface(), seat, data, event)
                }
                WindowSurface::X11(x11) => {
                    PointerTarget::gesture_swipe_end(x11.clone(), seat, data, event)
                }
            },
            FocusTarget::Popup(p) => {
                PointerTarget::gesture_swipe_end(p.wl_surface(), seat, data, event)
            }
        }
    }

    fn gesture_pinch_begin(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &GesturePinchBeginEvent,
    ) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    PointerTarget::gesture_pinch_begin(wl.wl_surface(), seat, data, event)
                }
                WindowSurface::X11(x11) => {
                    PointerTarget::gesture_pinch_begin(x11.clone(), seat, data, event)
                }
            },
            FocusTarget::Popup(p) => {
                PointerTarget::gesture_pinch_begin(p.wl_surface(), seat, data, event)
            }
        }
    }

    fn gesture_pinch_update(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &GesturePinchUpdateEvent,
    ) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    PointerTarget::gesture_pinch_update(wl.wl_surface(), seat, data, event)
                }
                WindowSurface::X11(x11) => {
                    PointerTarget::gesture_pinch_update(x11.clone(), seat, data, event)
                }
            },
            FocusTarget::Popup(p) => {
                PointerTarget::gesture_pinch_update(p.wl_surface(), seat, data, event)
            }
        }
    }

    fn gesture_pinch_end(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &GesturePinchEndEvent,
    ) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    PointerTarget::gesture_pinch_end(wl.wl_surface(), seat, data, event)
                }
                WindowSurface::X11(x11) => {
                    PointerTarget::gesture_pinch_end(x11.clone(), seat, data, event)
                }
            },
            FocusTarget::Popup(p) => {
                PointerTarget::gesture_pinch_end(p.wl_surface(), seat, data, event)
            }
        }
    }

    fn gesture_hold_begin(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &GestureHoldBeginEvent,
    ) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    PointerTarget::gesture_hold_begin(wl.wl_surface(), seat, data, event)
                }
                WindowSurface::X11(x11) => {
                    PointerTarget::gesture_hold_begin(x11.clone(), seat, data, event)
                }
            },
            FocusTarget::Popup(p) => {
                PointerTarget::gesture_hold_begin(p.wl_surface(), seat, data, event)
            }
        }
    }

    fn gesture_hold_end(&self, seat: &Seat<State>, data: &mut State, event: &GestureHoldEndEvent) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    PointerTarget::gesture_hold_end(wl.wl_surface(), seat, data, event)
                }
                WindowSurface::X11(x11) => {
                    PointerTarget::gesture_hold_end(x11.clone(), seat, data, event)
                }
            },
            FocusTarget::Popup(p) => {
                PointerTarget::gesture_hold_end(p.wl_surface(), seat, data, event)
            }
        }
    }

    fn leave(&self, seat: &Seat<State>, data: &mut State, serial: Serial, time: u32) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    PointerTarget::leave(wl.wl_surface(), seat, data, serial, time)
                }
                WindowSurface::X11(x11) => {
                    PointerTarget::leave(x11.clone(), seat, data, serial, time)
                }
            },
            FocusTarget::Popup(p) => PointerTarget::leave(p.wl_surface(), seat, data, serial, time),
        }
    }
}

impl WaylandFocus for FocusTarget {
    fn wl_surface(&self) -> Option<Cow<'_, WlSurface>> {
        match self {
            FocusTarget::Window(w) => w.wl_surface(),
            FocusTarget::Popup(p) => Some(Cow::Borrowed(p.wl_surface())),
        }
    }

    fn same_client_as(&self, object_id: &ObjectId) -> bool {
        match self {
            FocusTarget::Window(w) => w.same_client_as(object_id),
            FocusTarget::Popup(p) => p.wl_surface().same_client_as(object_id),
        }
    }
}

impl TouchTarget<State> for FocusTarget {
    fn down(&self, seat: &Seat<State>, data: &mut State, event: &DownEvent, seq: Serial) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    TouchTarget::down(wl.wl_surface(), seat, data, event, seq)
                }
                WindowSurface::X11(x11) => TouchTarget::down(x11.clone(), seat, data, event, seq),
            },
            FocusTarget::Popup(p) => TouchTarget::down(p.wl_surface(), seat, data, event, seq),
        }
    }

    fn up(&self, seat: &Seat<State>, data: &mut State, event: &UpEvent, seq: Serial) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    TouchTarget::up(wl.wl_surface(), seat, data, event, seq)
                }
                WindowSurface::X11(x11) => TouchTarget::up(x11.clone(), seat, data, event, seq),
            },
            FocusTarget::Popup(p) => TouchTarget::up(p.wl_surface(), seat, data, event, seq),
        }
    }

    fn motion(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &smithay::input::touch::MotionEvent,
        seq: Serial,
    ) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    TouchTarget::motion(wl.wl_surface(), seat, data, event, seq)
                }
                WindowSurface::X11(x11) => {
                    TouchTarget::motion(x11.clone(), seat, data, event, seq)
                }
            },
            FocusTarget::Popup(p) => TouchTarget::motion(p.wl_surface(), seat, data, event, seq),
        }
    }

    fn frame(&self, seat: &Seat<State>, data: &mut State, seq: Serial) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => TouchTarget::frame(wl.wl_surface(), seat, data, seq),
                WindowSurface::X11(x11) => TouchTarget::frame(x11.clone(), seat, data, seq),
            },
            FocusTarget::Popup(p) => TouchTarget::frame(p.wl_surface(), seat, data, seq),
        }
    }

    fn cancel(&self, seat: &Seat<State>, data: &mut State, seq: Serial) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => TouchTarget::cancel(wl.wl_surface(), seat, data, seq),
                WindowSurface::X11(x11) => TouchTarget::cancel(x11.clone(), seat, data, seq),
            },
            FocusTarget::Popup(p) => TouchTarget::cancel(p.wl_surface(), seat, data, seq),
        }
    }

    fn shape(&self, seat: &Seat<State>, data: &mut State, event: &ShapeEvent, seq: Serial) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    TouchTarget::shape(wl.wl_surface(), seat, data, event, seq)
                }
                WindowSurface::X11(x11) => TouchTarget::shape(x11.clone(), seat, data, event, seq),
            },
            FocusTarget::Popup(p) => TouchTarget::shape(p.wl_surface(), seat, data, event, seq),
        }
    }

    fn orientation(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        event: &OrientationEvent,
        seq: Serial,
    ) {
        match self {
            FocusTarget::Window(w) => match w.underlying_surface() {
                WindowSurface::Wayland(wl) => {
                    TouchTarget::orientation(wl.wl_surface(), seat, data, event, seq)
                }
                WindowSurface::X11(x11) => {
                    TouchTarget::orientation(x11.clone(), seat, data, event, seq)
                }
            },
            FocusTarget::Popup(p) => {
                TouchTarget::orientation(p.wl_surface(), seat, data, event, seq)
            }
        }
    }
}
