use std::cell::Cell;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PenState {
    pub pressure: f32,
    pub tilt: [f32; 2],
    pub rotation: f32,
    pub present: bool,
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
        let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            let event_ref = unsafe { event.as_ref() };
            if carries_pen_data(event_ref) {
                let tilt = event_ref.tilt();
                state.set(PenState {
                    pressure: event_ref.pressure().clamp(0.0, 1.0),
                    tilt: [tilt.x as f32, tilt.y as f32],
                    rotation: event_ref.rotation(),
                    present: true,
                });
            }
            event.as_ptr()
        });
        let mask = NSEventMask::TabletPoint
            | NSEventMask::LeftMouseDown
            | NSEventMask::LeftMouseDragged
            | NSEventMask::LeftMouseUp
            | NSEventMask::MouseMoved;
        unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) }
    }

    fn carries_pen_data(event: &NSEvent) -> bool {
        match event.r#type() {
            NSEventType::TabletPoint => true,
            NSEventType::LeftMouseDown
            | NSEventType::LeftMouseDragged
            | NSEventType::LeftMouseUp
            | NSEventType::MouseMoved => event.subtype() == NSEventSubtype::TabletPoint,
            _ => false,
        }
    }
}
