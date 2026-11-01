//! Локальное время и метка Ledger.

use std::fmt;

/// Локальное время суток в минутах от полуночи, от 0 до 1439.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MinuteOfDay(u16);

/// Невалидное локальное время: часы за пределами суток или минуты за
/// пределами часа.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidLocalTime;

impl MinuteOfDay {
    pub const MIN: Self = Self(0);
    pub const MAX: Self = Self(24 * 60 - 1);

    pub fn from_hms(hours: u64, minutes: u64) -> Result<Self, InvalidLocalTime> {
        if hours >= 24 || minutes >= 60 {
            return Err(InvalidLocalTime);
        }
        Ok(Self((hours * 60 + minutes) as u16))
    }

    pub fn parse(raw: &str) -> Result<Self, InvalidLocalTime> {
        let (hours, minutes) = raw.split_once(':').ok_or(InvalidLocalTime)?;
        let hours = hours.parse::<u64>().map_err(|_| InvalidLocalTime)?;
        let minutes = minutes.parse::<u64>().map_err(|_| InvalidLocalTime)?;
        Self::from_hms(hours, minutes)
    }

    pub fn minutes(self) -> u16 {
        self.0
    }
}

impl fmt::Display for MinuteOfDay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:02}:{:02}", self.0 / 60, self.0 % 60)
    }
}

impl fmt::Display for InvalidLocalTime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid local time")
    }
}

impl std::error::Error for InvalidLocalTime {}

/// Обрезанная метка Ledger: вывод системной команды времени без правил
/// формата, диапазона и непустоты.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timestamp(String);

impl Timestamp {
    pub fn new(raw: &str) -> Self {
        Self(raw.trim().to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{MinuteOfDay, Timestamp};

    #[test]
    fn parses_local_time_in_hh_mm_format() {
        let time = MinuteOfDay::parse("12:34").expect("valid local time");
        assert_eq!(
            time,
            MinuteOfDay::from_hms(12, 34).expect("valid local time")
        );
        assert_eq!(time.to_string(), "12:34");
    }

    #[test]
    fn parses_midnight_and_latest_minute() {
        assert_eq!(MinuteOfDay::parse("00:00").unwrap().to_string(), "00:00");
        assert_eq!(MinuteOfDay::parse("23:59").unwrap().to_string(), "23:59");
    }

    #[test]
    fn rejects_hours_outside_a_day() {
        assert!(MinuteOfDay::parse("24:00").is_err());
        assert!(MinuteOfDay::from_hms(24, 0).is_err());
        assert!(MinuteOfDay::parse("99:59").is_err());
    }

    #[test]
    fn rejects_minutes_outside_an_hour() {
        assert!(MinuteOfDay::parse("12:60").is_err());
        assert!(MinuteOfDay::from_hms(12, 60).is_err());
        assert!(MinuteOfDay::parse("12:99").is_err());
    }

    #[test]
    fn rejects_malformed_time_strings() {
        for raw in [
            "", "12", "12:", ":34", "12:34:56", "ab:cd", "12 34", "-1:00",
        ] {
            assert!(MinuteOfDay::parse(raw).is_err(), "time: {raw:?}");
        }
    }

    #[test]
    fn formats_with_leading_zeros() {
        assert_eq!(MinuteOfDay::from_hms(1, 5).unwrap().to_string(), "01:05");
        assert_eq!(MinuteOfDay::from_hms(9, 0).unwrap().to_string(), "09:00");
    }

    #[test]
    fn timestamp_trims_the_captured_mark() {
        let timestamp = Timestamp::new("  2026/11/01 12:34:56\n");
        assert_eq!(timestamp.as_str(), "2026/11/01 12:34:56");
    }

    #[test]
    fn timestamp_preserves_unusual_marks_without_validation() {
        for raw in ["", "garbage", "12:34", "i 2026/11/01 12:34:56 x\ny"] {
            let timestamp = Timestamp::new(raw);
            assert_eq!(timestamp.as_str(), raw.trim());
        }
    }
}
