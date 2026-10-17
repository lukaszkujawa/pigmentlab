use std::cell::Cell;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PenState {
    pub pressure: f32,
    pub tilt: [f32; 2],
    pub rotation: f32,
    pub present: bool,
}

impl PenState {
    pub fn brush_pressure(&self, firmness: f32) -> f32 {
        (self.pressure / firmness.max(0.05)).min(1.0).powi(2)
    }
}

pub struct Tablet {
    state: Rc<Cell<PenState>>,
    #[cfg(target_os = "macos")]
    _monitor: Option<objc2::rc::Retained<objc2::runtime::AnyObject>>,
}

impl Tablet {
    pub fn install() -> Self {
        let state = Rc::new(Cell::new(PenState::default()));
        #[cfg(target_os = "macos")]
        let monitor = macos::install(state.clone());
        Self {
            state,
            #[cfg(target_os = "macos")]
            _monitor: monitor,
        }
    }

    pub fn state(&self) -> PenState {
        self.state.get()
    }

    #[cfg(windows)]
    pub fn attach(&self, window: &slint::Window) {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = window.window_handle().window_handle()
            && let RawWindowHandle::Win32(win32) = handle.as_raw()
        {
            windows::install(win32.hwnd.get() as _, self.state.clone());
        }
    }

    #[cfg(not(windows))]
    pub fn attach(&self, _window: &slint::Window) {}
}

#[cfg(windows)]
mod windows {
    use super::PenState;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::LazyLock;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Controls::{
        FEEDBACK_PEN_BARRELVISUALIZATION, FEEDBACK_PEN_DOUBLETAP, FEEDBACK_PEN_PRESSANDHOLD, FEEDBACK_PEN_RIGHTTAP,
        FEEDBACK_PEN_TAP, SetWindowFeedbackSetting,
    };
    use windows_sys::Win32::UI::Input::Pointer::{GetPointerPenInfo, GetPointerType, POINTER_PEN_INFO};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, PEN_MASK_PRESSURE, PEN_MASK_ROTATION, PEN_MASK_TILT_X, PEN_MASK_TILT_Y, PT_PEN, WM_POINTERDOWN,
        WM_POINTERLEAVE, WM_POINTERUP, WM_POINTERUPDATE,
    };

    const PEN_SUBCLASS: usize = 1;
    const MAX_PRESSURE: f32 = 1024.0;
    const FULL_TILT_DEGREES: f32 = 60.0;

    static LOG: LazyLock<bool> = LazyLock::new(|| std::env::var_os("PIGMENTLAB_LOG_PEN").is_some());

    pub fn install(window: HWND, state: Rc<Cell<PenState>>) {
        let disabled = 0i32;
        for feedback in [
            FEEDBACK_PEN_BARRELVISUALIZATION,
            FEEDBACK_PEN_TAP,
            FEEDBACK_PEN_DOUBLETAP,
            FEEDBACK_PEN_PRESSANDHOLD,
            FEEDBACK_PEN_RIGHTTAP,
        ] {
            unsafe { SetWindowFeedbackSetting(window, feedback, 0, size_of::<i32>() as u32, (&raw const disabled).cast()) };
        }
        unsafe { SetWindowSubclass(window, Some(pen_messages), PEN_SUBCLASS, Rc::into_raw(state) as usize) };
    }

    unsafe extern "system" fn pen_messages(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass: usize,
        state: usize,
    ) -> LRESULT {
        let pointer = (wparam & 0xffff) as u32;
        let pointer_message = matches!(message, WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP | WM_POINTERLEAVE);
        if !pointer_message || !is_pen(pointer) {
            return unsafe { DefSubclassProc(window, message, wparam, lparam) };
        }
        let state = unsafe { &*(state as *const Cell<PenState>) };
        if message == WM_POINTERLEAVE {
            state.set(PenState::default());
        } else if let Some(pen) = pen_info(pointer) {
            if *LOG {
                eprintln!(
                    "pen {message:#x} mask {:#x} pressure {} tilt {} {} rotation {}",
                    pen.penMask, pen.pressure, pen.tiltX, pen.tiltY, pen.rotation
                );
            }
            state.set(pen_state(&pen));
        }
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    fn is_pen(pointer: u32) -> bool {
        let mut kind = 0;
        unsafe { GetPointerType(pointer, &mut kind) != 0 && kind == PT_PEN }
    }

    fn pen_info(pointer: u32) -> Option<POINTER_PEN_INFO> {
        let mut info = unsafe { std::mem::zeroed() };
        (unsafe { GetPointerPenInfo(pointer, &mut info) } != 0).then_some(info)
    }

    fn pen_state(pen: &POINTER_PEN_INFO) -> PenState {
        let reported = |mask: u32| pen.penMask & mask != 0;
        let tilt = |degrees: i32, mask: u32| {
            if reported(mask) { (degrees as f32 / FULL_TILT_DEGREES).clamp(-1.0, 1.0) } else { 0.0 }
        };
        PenState {
            pressure: if reported(PEN_MASK_PRESSURE) { (pen.pressure as f32 / MAX_PRESSURE).min(1.0) } else { 0.0 },
            tilt: [tilt(pen.tiltX, PEN_MASK_TILT_X), -tilt(pen.tiltY, PEN_MASK_TILT_Y)],
            rotation: if reported(PEN_MASK_ROTATION) { pen.rotation as f32 } else { 0.0 },
            present: true,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn pen(pressure: u32, tilt_x: i32, tilt_y: i32) -> POINTER_PEN_INFO {
            POINTER_PEN_INFO {
                penMask: PEN_MASK_PRESSURE | PEN_MASK_TILT_X | PEN_MASK_TILT_Y,
                pressure,
                tiltX: tilt_x,
                tiltY: tilt_y,
                ..unsafe { std::mem::zeroed() }
            }
        }

        #[test]
        fn pressure_is_normalised() {
            assert_eq!(pen_state(&pen(512, 0, 0)).pressure, 0.5);
            assert_eq!(pen_state(&pen(1024, 0, 0)).pressure, 1.0);
        }

        #[test]
        fn tilt_matches_cocoa_axes() {
            assert_eq!(pen_state(&pen(0, 30, 0)).tilt, [0.5, 0.0]);
            assert_eq!(pen_state(&pen(0, 0, 30)).tilt, [0.0, -0.5]);
            assert_eq!(pen_state(&pen(0, 90, -90)).tilt, [1.0, 1.0]);
        }

        #[test]
        fn unreported_axes_stay_neutral() {
            let state = pen_state(&POINTER_PEN_INFO { pressure: 700, tiltX: 40, ..unsafe { std::mem::zeroed() } });
            assert_eq!((state.pressure, state.tilt), (0.0, [0.0; 2]));
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::PenState;
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventSubtype, NSEventType};
    use std::cell::Cell;
    use std::ptr::NonNull;
    use std::rc::Rc;

    pub fn install(state: Rc<Cell<PenState>>) -> Option<Retained<AnyObject>> {
        NSEvent::setMouseCoalescingEnabled(false);
        let log = std::env::var_os("PIGMENTLAB_LOG_PEN").is_some();
        let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            let event_ref = unsafe { event.as_ref() };
            let updated = updated_state(event_ref, state.get());
            if log && updated != state.get() {
                eprintln!("pen {:?} {updated:?}", event_ref.r#type());
            }
            state.set(updated);
            event.as_ptr()
        });
        let mask = NSEventMask::TabletPoint
            | NSEventMask::TabletProximity
            | NSEventMask::LeftMouseDown
            | NSEventMask::LeftMouseDragged
            | NSEventMask::LeftMouseUp
            | NSEventMask::MouseMoved;
        unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) }
    }

    fn updated_state(event: &NSEvent, previous: PenState) -> PenState {
        let kind = event.r#type();
        let subtype = if kind == NSEventType::TabletPoint || kind == NSEventType::TabletProximity {
            None
        } else {
            Some(event.subtype())
        };
        if kind == NSEventType::TabletProximity || subtype == Some(NSEventSubtype::TabletProximity) {
            return PenState { present: event.isEnteringProximity(), ..PenState::default() };
        }
        if kind == NSEventType::TabletPoint || subtype == Some(NSEventSubtype::TabletPoint) {
            let tilt = event.tilt();
            return PenState {
                pressure: event.pressure().clamp(0.0, 1.0),
                tilt: [tilt.x as f32, tilt.y as f32],
                rotation: event.rotation(),
                present: true,
            };
        }
        PenState { present: false, ..previous }
    }
}

#[cfg(test)]
mod tests {
    use super::PenState;

    fn pen(pressure: f32) -> PenState {
        PenState { pressure, present: true, ..PenState::default() }
    }

    #[test]
    fn pressing_at_firmness_gives_full_pressure() {
        assert_eq!(pen(0.7).brush_pressure(0.7), 1.0);
        assert_eq!(pen(0.9).brush_pressure(0.7), 1.0);
    }

    #[test]
    fn light_touches_stay_light() {
        assert!(pen(0.1).brush_pressure(0.7) < 0.03);
        assert_eq!(pen(0.0).brush_pressure(0.7), 0.0);
    }

    #[test]
    fn response_rises_monotonically() {
        let curve: Vec<f32> = (0..=10).map(|i| pen(i as f32 / 10.0).brush_pressure(0.7)).collect();
        assert!(curve.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}
