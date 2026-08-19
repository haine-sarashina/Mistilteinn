//! Where the browser thinks it is.
//!
//! There is no satellite receiver and no location service here, so the honest
//! answer to `getCurrentPosition` is usually that the position is unavailable —
//! which is a defined outcome the API has an error code for, and which lets a
//! page fall back the way it would on a desktop with the radio switched off.
//!
//! A position can be supplied instead: browsers let a developer set one, and a
//! reader who wants a site to work can say where they are without giving the
//! browser a way to find out on its own. `MISTILTEIN_GEOLOCATION="35.68,139.77"`
//! does that.

use std::cell::RefCell;

/// A fix, as the API reports one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub latitude: f64,
    pub longitude: f64,
    /// Radius of uncertainty in metres. A supplied position is exact by
    /// definition, so this is small rather than absent.
    pub accuracy: f64,
}

/// Why a request produced no position, using the API's own numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionError {
    PermissionDenied = 1,
    PositionUnavailable = 2,
    Timeout = 3,
}

impl PositionError {
    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn message(self) -> &'static str {
        match self {
            PositionError::PermissionDenied => "User denied Geolocation",
            PositionError::PositionUnavailable => {
                "No location provider is available to this browser"
            }
            PositionError::Timeout => "Timed out looking for a position",
        }
    }
}

thread_local! {
    static FIXED_POSITION: RefCell<Option<Position>> = const { RefCell::new(None) };
    static READ_ENVIRONMENT: RefCell<bool> = const { RefCell::new(true) };
}

/// Supply a position for this session, overriding the environment.
pub fn set_position(position: Option<Position>) {
    FIXED_POSITION.with(|fixed| *fixed.borrow_mut() = position);
    READ_ENVIRONMENT.with(|read| *read.borrow_mut() = false);
}

/// Parse `"<lat>,<lon>"`, as the environment variable is written.
///
/// Out-of-range coordinates are refused rather than clamped: a page told it is
/// at latitude 500 would draw a map of nowhere.
pub fn parse_position(text: &str) -> Option<Position> {
    let (latitude, longitude) = text.split_once(',')?;
    let latitude: f64 = latitude.trim().parse().ok()?;
    let longitude: f64 = longitude.trim().parse().ok()?;
    if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
        return None;
    }
    Some(Position {
        latitude,
        longitude,
        accuracy: 1.0,
    })
}

/// The current position, if this browser has one to give.
pub fn current_position() -> Result<Position, PositionError> {
    if let Some(position) = FIXED_POSITION.with(|fixed| *fixed.borrow()) {
        return Ok(position);
    }
    let from_environment = READ_ENVIRONMENT.with(|read| *read.borrow());
    if from_environment
        && let Ok(text) = std::env::var("MISTILTEIN_GEOLOCATION")
        && let Some(position) = parse_position(&text)
    {
        return Ok(position);
    }
    Err(PositionError::PositionUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_coordinate_pair_is_read_as_a_position() {
        let position = parse_position("35.68, 139.77").expect("that is a position");
        assert!((position.latitude - 35.68).abs() < 1e-6);
        assert!((position.longitude - 139.77).abs() < 1e-6);
    }

    #[test]
    fn a_negative_coordinate_is_a_coordinate() {
        let position = parse_position("-33.87,-151.21").expect("that is a position");
        assert!(position.latitude < 0.0);
        assert!(position.longitude < 0.0);
    }

    #[test]
    fn something_that_is_not_a_position_is_refused() {
        assert_eq!(parse_position("somewhere nice"), None);
        assert_eq!(parse_position("35.68"), None);
        assert_eq!(parse_position(""), None);
    }

    #[test]
    fn a_coordinate_off_the_globe_is_refused() {
        assert_eq!(parse_position("500,0"), None);
        assert_eq!(parse_position("0,200"), None);
    }

    #[test]
    fn a_supplied_position_is_what_is_reported() {
        set_position(Some(Position {
            latitude: 51.5,
            longitude: -0.12,
            accuracy: 1.0,
        }));
        let position = current_position().expect("a position was supplied");
        assert!((position.latitude - 51.5).abs() < 1e-6);
    }

    #[test]
    fn with_nothing_to_go_on_the_position_is_unavailable() {
        set_position(None);
        assert_eq!(current_position(), Err(PositionError::PositionUnavailable));
    }

    #[test]
    fn the_error_codes_are_the_ones_the_api_defines() {
        assert_eq!(PositionError::PermissionDenied.code(), 1);
        assert_eq!(PositionError::PositionUnavailable.code(), 2);
        assert_eq!(PositionError::Timeout.code(), 3);
    }
}
