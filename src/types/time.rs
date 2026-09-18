use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// ASS time representation in H:MM:SS.CC format
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Time {
    hours: u32,
    minutes: u32,
    seconds: u32,
    centiseconds: u32,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum TimeError {
    #[error("Invalid time format: {0}")]
    InvalidFormat(String),
    #[error("Invalid time component: {0}")]
    InvalidComponent(String),
}

impl Time {
    /// Normalizing constructor: out-of-range components are clamped.
    /// Prefer [`Time::try_new`] for validating external input.
    pub fn new(hours: u32, minutes: u32, seconds: u32, centiseconds: u32) -> Self {
        Self {
            hours,
            minutes: minutes.min(59),
            seconds: seconds.min(59),
            centiseconds: centiseconds.min(99),
        }
    }

    /// Checked constructor: rejects minutes > 59, seconds > 59, and
    /// centiseconds > 99 instead of silently clamping them.
    pub fn try_new(
        hours: u32,
        minutes: u32,
        seconds: u32,
        centiseconds: u32,
    ) -> Result<Self, TimeError> {
        if minutes > 59 {
            return Err(TimeError::InvalidComponent(format!(
                "minutes out of range 0-59: {}",
                minutes
            )));
        }
        if seconds > 59 {
            return Err(TimeError::InvalidComponent(format!(
                "seconds out of range 0-59: {}",
                seconds
            )));
        }
        if centiseconds > 99 {
            return Err(TimeError::InvalidComponent(format!(
                "centiseconds out of range 0-99: {}",
                centiseconds
            )));
        }
        Ok(Self {
            hours,
            minutes,
            seconds,
            centiseconds,
        })
    }

    pub fn from_millis(millis: u64) -> Self {
        let total_cs = millis / 10;
        // Saturate: past ~476k years the hours no longer fit u32, and a
        // bare `as u32` would wrap. Minutes/seconds/cs are mod-bounded.
        let hours = u32::try_from(total_cs / 360000).unwrap_or(u32::MAX);
        let minutes = ((total_cs % 360000) / 6000) as u32;
        let seconds = ((total_cs % 6000) / 100) as u32;
        let centiseconds = (total_cs % 100) as u32;

        Self::new(hours, minutes, seconds, centiseconds)
    }

    pub fn to_millis(&self) -> u64 {
        let total_cs = self.hours as u64 * 360000
            + self.minutes as u64 * 6000
            + self.seconds as u64 * 100
            + self.centiseconds as u64;
        total_cs * 10
    }

    pub fn to_seconds(&self) -> f64 {
        self.to_millis() as f64 / 1000.0
    }

    pub fn hours(&self) -> u32 {
        self.hours
    }

    pub fn minutes(&self) -> u32 {
        self.minutes
    }

    pub fn seconds(&self) -> u32 {
        self.seconds
    }

    pub fn centiseconds(&self) -> u32 {
        self.centiseconds
    }

    pub fn zero() -> Self {
        Self::new(0, 0, 0, 0)
    }
}

impl FromStr for Time {
    type Err = TimeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        // Try H:MM:SS.CC format
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() == 3 {
            let hours: u32 = parts[0]
                .parse()
                .map_err(|_| TimeError::InvalidComponent(format!("hours: {}", parts[0])))?;

            let minutes: u32 = parts[1]
                .parse()
                .map_err(|_| TimeError::InvalidComponent(format!("minutes: {}", parts[1])))?;

            let sec_parts: Vec<&str> = parts[2].split('.').collect();
            if sec_parts.len() == 2 {
                let seconds: u32 = sec_parts[0].parse().map_err(|_| {
                    TimeError::InvalidComponent(format!("seconds: {}", sec_parts[0]))
                })?;
                let centiseconds: u32 = sec_parts[1].parse().map_err(|_| {
                    TimeError::InvalidComponent(format!("centiseconds: {}", sec_parts[1]))
                })?;

                return Self::try_new(hours, minutes, seconds, centiseconds);
            } else if sec_parts.len() == 1 {
                let seconds: u32 = sec_parts[0].parse().map_err(|_| {
                    TimeError::InvalidComponent(format!("seconds: {}", sec_parts[0]))
                })?;
                return Self::try_new(hours, minutes, seconds, 0);
            }
        }

        Err(TimeError::InvalidFormat(s.to_string()))
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:01}:{:02}:{:02}.{:02}",
            self.hours, self.minutes, self.seconds, self.centiseconds
        )
    }
}

impl Default for Time {
    fn default() -> Self {
        Self::zero()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_time() {
        let time: Time = "0:00:01.00".parse().unwrap();
        assert_eq!(time.hours(), 0);
        assert_eq!(time.minutes(), 0);
        assert_eq!(time.seconds(), 1);
        assert_eq!(time.centiseconds(), 0);
    }

    #[test]
    fn test_parse_time_with_hours() {
        let time: Time = "1:30:45.50".parse().unwrap();
        assert_eq!(time.hours(), 1);
        assert_eq!(time.minutes(), 30);
        assert_eq!(time.seconds(), 45);
        assert_eq!(time.centiseconds(), 50);
    }

    #[test]
    fn test_parse_invalid_time() {
        assert!("invalid".parse::<Time>().is_err());
        assert!("not-a-time".parse::<Time>().is_err());
        assert!("".parse::<Time>().is_err());
    }

    #[test]
    fn test_parse_rejects_out_of_range_components() {
        // Must not silently clamp to 0:59:59.99
        assert!("0:99:99.999".parse::<Time>().is_err());
        assert!("0:99:00.00".parse::<Time>().is_err());
        assert!("0:00:99.00".parse::<Time>().is_err());
        assert!("0:00:00.999".parse::<Time>().is_err());
        assert!("0:59:59.99".parse::<Time>().is_ok());
    }

    #[test]
    fn test_try_new_validates() {
        assert!(Time::try_new(0, 59, 59, 99).is_ok());
        assert!(Time::try_new(0, 60, 0, 0).is_err());
        assert!(Time::try_new(0, 0, 60, 0).is_err());
        assert!(Time::try_new(0, 0, 0, 100).is_err());
        // new() remains the normalizing helper
        assert_eq!(Time::new(0, 99, 99, 999).minutes(), 59);
    }

    #[test]
    fn test_to_millis() {
        let time = Time::new(0, 1, 30, 50);
        assert_eq!(time.to_millis(), 90500);
    }

    #[test]
    fn test_from_millis() {
        let time = Time::from_millis(90500);
        assert_eq!(time.hours(), 0);
        assert_eq!(time.minutes(), 1);
        assert_eq!(time.seconds(), 30);
        assert_eq!(time.centiseconds(), 50);
    }

    #[test]
    fn test_display() {
        let time = Time::new(1, 30, 45, 50);
        assert_eq!(time.to_string(), "1:30:45.50");
    }

    #[test]
    fn test_zero() {
        let time = Time::zero();
        assert_eq!(time.to_millis(), 0);
        assert_eq!(time.to_string(), "0:00:00.00");
    }
}
