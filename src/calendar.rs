use std::fmt;
use std::ops::RangeInclusive;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const YEARS: RangeInclusive<i32> = 1..=9999;

pub const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

const WEEKDAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

/// A proleptic Gregorian date, stored in board files as `YYYY-MM-DD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StoryDate {
    year: i32,
    month: u8,
    day: u8,
}

impl StoryDate {
    pub fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        let valid = YEARS.contains(&year)
            && (1..=12).contains(&month)
            && (1..=days_in_month(year, month)).contains(&day);
        valid.then_some(Self { year, month, day })
    }

    /// Today in UTC; good enough as a starting suggestion for a story.
    pub fn today() -> Self {
        let days = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs() / SECONDS_PER_DAY);
        i64::try_from(days)
            .ok()
            .and_then(Self::from_days_since_epoch)
            .unwrap_or(Self {
                year: 1970,
                month: 1,
                day: 1,
            })
    }

    pub fn year(self) -> i32 {
        self.year
    }

    pub fn month(self) -> u8 {
        self.month
    }

    pub fn day(self) -> u8 {
        self.day
    }

    /// Keeps the day of the month where it can, else the month's last day,
    /// so stepping from 31 Jan to February lands on the 28th or 29th.
    pub fn with_year_month_day(year: i32, month: u8, day: u8) -> Self {
        let year = year.clamp(*YEARS.start(), *YEARS.end());
        let month = month.clamp(1, 12);
        let day = day.clamp(1, days_in_month(year, month));
        Self { year, month, day }
    }

    pub fn plus_days(self, days: i64) -> Option<Self> {
        Self::from_days_since_epoch(self.days_since_epoch().checked_add(days)?)
    }

    pub fn days_until(self, other: Self) -> i64 {
        other.days_since_epoch() - self.days_since_epoch()
    }

    /// For display, e.g. `Mon 16 Mar 2026`.
    pub fn long_label(self) -> String {
        format!(
            "{} {} {} {}",
            WEEKDAY_NAMES[self.weekday_index()],
            self.day,
            MONTH_NAMES[usize::from(self.month - 1)],
            self.year
        )
    }

    fn weekday_index(self) -> usize {
        let thursday_based = (self.days_since_epoch() + 3).rem_euclid(7);
        usize::try_from(thursday_based).unwrap_or_default()
    }

    // Howard Hinnant's days_from_civil; 1970-01-01 is day 0.
    fn days_since_epoch(self) -> i64 {
        let month = i64::from(self.month);
        let year = i64::from(self.year) - i64::from(month <= 2);
        let era = year.div_euclid(400);
        let year_of_era = year - era * 400;
        let day_of_year =
            (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    // Howard Hinnant's civil_from_days.
    fn from_days_since_epoch(days: i64) -> Option<Self> {
        let days = days.checked_add(719_468)?;
        let era = days.div_euclid(146_097);
        let day_of_era = days - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let shifted_month = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
        let month = if shifted_month < 10 {
            shifted_month + 3
        } else {
            shifted_month - 9
        };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        Self::new(
            i32::try_from(year).ok()?,
            u8::try_from(month).ok()?,
            u8::try_from(day).ok()?,
        )
    }
}

pub fn date_of_day(story_start: StoryDate, day: i32) -> Option<StoryDate> {
    story_start.plus_days(i64::from(day) - 1)
}

pub fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

impl fmt::Display for StoryDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidDate;

impl fmt::Display for InvalidDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected a date as YYYY-MM-DD")
    }
}

impl std::error::Error for InvalidDate {}

impl FromStr for StoryDate {
    type Err = InvalidDate;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parts = text.split('-');
        let (Some(year), Some(month), Some(day), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(InvalidDate);
        };
        if year.len() != 4 || month.len() != 2 || day.len() != 2 {
            return Err(InvalidDate);
        }
        if ![year, month, day]
            .iter()
            .all(|part| part.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(InvalidDate);
        }
        let (Ok(year), Ok(month), Ok(day)) = (year.parse(), month.parse(), day.parse()) else {
            return Err(InvalidDate);
        };
        Self::new(year, month, day).ok_or(InvalidDate)
    }
}

impl Serialize for StoryDate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for StoryDate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> StoryDate {
        text.parse().unwrap()
    }

    #[test]
    fn parses_and_prints_iso_dates() {
        assert_eq!(date("2026-03-16").to_string(), "2026-03-16");
        assert_eq!(date("0001-01-01").to_string(), "0001-01-01");
        for bad in [
            "",
            "2026-3-16",
            "2026-02-30",
            "2026-13-01",
            "0000-01-01",
            "2026-03-16-01",
            "+026-03-16",
            "2026/03/16",
        ] {
            assert_eq!(bad.parse::<StoryDate>(), Err(InvalidDate), "{bad}");
        }
    }

    #[test]
    fn day_arithmetic_crosses_months_years_and_leap_days() {
        assert_eq!(date("2024-02-28").plus_days(1), Some(date("2024-02-29")));
        assert_eq!(date("2023-02-28").plus_days(1), Some(date("2023-03-01")));
        assert_eq!(date("2026-12-31").plus_days(1), Some(date("2027-01-01")));
        assert_eq!(date("2026-01-01").plus_days(-1), Some(date("2025-12-31")));
        assert_eq!(date("2000-01-01").days_until(date("2001-01-01")), 366);
        assert_eq!(date("1970-01-01").days_since_epoch(), 0);
    }

    #[test]
    fn plus_days_stays_within_supported_years() {
        assert_eq!(date("0001-01-01").plus_days(-1), None);
        assert_eq!(date("9999-12-31").plus_days(1), None);
        assert_eq!(date("2026-01-01").plus_days(i64::MAX), None);
    }

    #[test]
    fn every_day_round_trips_through_the_epoch_count() {
        let mut day = date("1899-12-25");
        for _ in 0..(366 * 8) {
            let next = day.plus_days(1).unwrap();
            assert_eq!(day.days_until(next), 1);
            assert_eq!(
                StoryDate::from_days_since_epoch(day.days_since_epoch()),
                Some(day)
            );
            day = next;
        }
    }

    #[test]
    fn long_label_includes_weekday() {
        assert_eq!(date("2026-03-16").long_label(), "Mon 16 Mar 2026");
        assert_eq!(date("1970-01-01").long_label(), "Thu 1 Jan 1970");
        assert_eq!(date("1600-02-29").long_label(), "Tue 29 Feb 1600");
    }

    #[test]
    fn changing_month_clamps_the_day() {
        assert_eq!(
            StoryDate::with_year_month_day(2026, 2, 31),
            date("2026-02-28")
        );
        assert_eq!(
            StoryDate::with_year_month_day(2024, 2, 31),
            date("2024-02-29")
        );
        assert_eq!(
            StoryDate::with_year_month_day(12_000, 1, 1),
            date("9999-01-01")
        );
    }
}
