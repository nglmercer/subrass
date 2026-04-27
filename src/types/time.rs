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
    pub fn new(hours: u32, minutes: u32, seconds: u32, centiseconds: u32) -> Self {
        Self {
            hours,
            minutes: minutes.min(59),
            seconds: seconds.min(59),
            centiseconds: centiseconds.min(99),
        }
    }

    pub fn from_millis(millis: u64) -> Self {
        let total_cs = millis / 10;
        let hours = (total_cs / 360000) as u32;
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

                return Ok(Self::new(hours, minutes, seconds, centiseconds));
            } else if sec_parts.len() == 1 {
                let seconds: u32 = sec_parts[0].parse().map_err(|_| {
                    TimeError::InvalidComponent(format!("seconds: {}", sec_parts[0]))
                })?;
                return Ok(Self::new(hours, minutes, seconds, 0));
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
