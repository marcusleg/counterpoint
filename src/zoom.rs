//! The editor's zoom level, a percentage clamped to a fixed range and stepped by 10 points.

/// The smallest zoom level, in percent.
pub const MIN: u32 = 50;
/// The largest zoom level, in percent.
pub const MAX: u32 = 300;
/// The size of one zoom step, in percentage points.
pub const STEP: u32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Zoom(u32);

impl Default for Zoom {
    fn default() -> Self {
        Self(100)
    }
}

impl Zoom {
    /// Clamps `percent` to `MIN..=MAX` and rounds it to the nearest multiple of `STEP`, so a
    /// hand-edited value can never produce an odd level.
    pub fn from_percent(percent: u32) -> Self {
        let clamped = percent.clamp(MIN, MAX);
        let rounded = (clamped + STEP / 2) / STEP * STEP;
        Self(rounded.clamp(MIN, MAX))
    }

    pub fn zoom_in(self) -> Self {
        Self((self.0 + STEP).min(MAX))
    }

    pub fn zoom_out(self) -> Self {
        Self(self.0.saturating_sub(STEP).max(MIN))
    }

    pub fn reset() -> Self {
        Self::default()
    }

    pub fn percent(self) -> u32 {
        self.0
    }

    pub fn can_zoom_in(self) -> bool {
        self.0 < MAX
    }

    pub fn can_zoom_out(self) -> bool {
        self.0 > MIN
    }

    pub fn label(self) -> String {
        format!("{}%", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_100_percent() {
        assert_eq!(Zoom::default().percent(), 100);
    }

    #[test]
    fn zoom_in_steps_by_ten() {
        assert_eq!(Zoom::default().zoom_in().percent(), 110);
    }

    #[test]
    fn zoom_out_steps_by_ten() {
        assert_eq!(Zoom::default().zoom_out().percent(), 90);
    }

    #[test]
    fn zoom_out_clamps_at_the_minimum() {
        let mut zoom = Zoom::default();
        for _ in 0..20 {
            zoom = zoom.zoom_out();
        }
        assert_eq!(zoom.percent(), MIN);
    }

    #[test]
    fn zoom_in_clamps_at_the_maximum() {
        let mut zoom = Zoom::default();
        for _ in 0..30 {
            zoom = zoom.zoom_in();
        }
        assert_eq!(zoom.percent(), MAX);
    }

    #[test]
    fn reset_returns_to_100_percent() {
        assert_eq!(Zoom::reset(), Zoom::default());
        assert_eq!(Zoom::reset().percent(), 100);
    }

    #[test]
    fn can_zoom_out_is_false_exactly_at_the_minimum() {
        let mut zoom = Zoom::default();
        while zoom.can_zoom_out() {
            zoom = zoom.zoom_out();
        }
        assert_eq!(zoom.percent(), MIN);
        assert!(!zoom.can_zoom_out());
    }

    #[test]
    fn can_zoom_in_is_false_exactly_at_the_maximum() {
        let mut zoom = Zoom::default();
        while zoom.can_zoom_in() {
            zoom = zoom.zoom_in();
        }
        assert_eq!(zoom.percent(), MAX);
        assert!(!zoom.can_zoom_in());
    }

    #[test]
    fn label_formats_the_percentage() {
        assert_eq!(Zoom::default().label(), "100%");
        assert_eq!(Zoom::default().zoom_in().label(), "110%");
    }

    #[test]
    fn from_percent_clamps_below_the_minimum() {
        assert_eq!(Zoom::from_percent(7).percent(), MIN);
    }

    #[test]
    fn from_percent_clamps_above_the_maximum() {
        assert_eq!(Zoom::from_percent(1000).percent(), MAX);
    }

    #[test]
    fn from_percent_rounds_to_the_nearest_step() {
        assert_eq!(Zoom::from_percent(104).percent(), 100);
        assert_eq!(Zoom::from_percent(105).percent(), 110);
    }
}
