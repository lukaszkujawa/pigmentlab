mod deposit;
mod gauge;
mod level;
pub(crate) mod marks;
pub(crate) mod rheology;
mod squeeze;
mod tube;
mod window;

pub use gauge::{BrushContents, GAUGE_CHANNELS, Gauge, OVERLOADED_FILL};
pub use deposit::{Deposit, MEDIUM_THINNING, StrokeState};
pub use level::{Levelling, WetBounds, WetRegions};
pub use marks::{MARK_CAPACITY, Mark, Marks, Profile};
pub use rheology::{YIELD_STRESS_OILY, YIELD_STRESS_STIFF};
pub use tube::{NOZZLE_RADIUS, Tube};
pub use window::{ActiveWindow, SMALLEST_WINDOW, WindowRegion};
