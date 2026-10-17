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
