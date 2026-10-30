//! Carbon - Laravel Carbon / DateTime Facade & Instance Implementation for RHP
//!
//! Provides full-featured, fluent date/time manipulation modeled after Laravel's Carbon:
//! - Factory constructors: `Carbon::now()`, `Carbon::today()`, `Carbon::yesterday()`,
//!   `Carbon::tomorrow()`, `Carbon::parse()`, `Carbon::create()`, `Carbon::createFromTimestamp()`,
//!   `Carbon::createFromFormat()`, `Carbon::setTestNow()`, `Carbon::hasTestNow()`.
//! - Output & formatting: `toDateTimeString()`, `toDateString()`, `toDatString()`, `toTimeString()`,
//!   `toISOString()`, `toIso8601String()`, `toRfc2822String()`, `toFormattedDateString()`, `toDayDateTimeString()`, `format()`.
//! - Fluent modifiers: `addDay()`, `addDays()`, `subDay()`, `subDays()`, `addWeek()`, `addMonth()`,
//!   `addYear()`, `startOfDay()`, `endOfDay()`, `startOfMonth()`, `endOfMonth()`, `lastOfMonth()`, `firstOfMonth()`.
//! - Comparison & difference: `isToday()`, `isPast()`, `isFuture()`, `isWeekend()`, `isWeekday()`, `isLeapYear()`,
//!   `diffInDays()`, `diffInHours()`, `diffInMinutes()`, `diffInSeconds()`, `diffForHumans()`, `gt()`, `gte()`, `lt()`, `lte()`, `eq()`.

use std::cell::RefCell;
use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Local, NaiveDate, NaiveDateTime, NaiveTime, Offset,
    TimeZone, Timelike, Utc, Weekday,
};

use crate::lexer::span::Span;
use crate::type_system::types::RustIntKind;
use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::facade::{get_arg_str, register_runtime_closure_macro, FacadeResult};
use crate::vm::value::{PhpValue, VmObject};

thread_local! {
    /// Thread-safe mock test fixture time (`Carbon::setTestNow(...)`).
    static TEST_NOW: RefCell<Option<CarbonDate>> = const { RefCell::new(None) };
}

/// Return true if the given year is a leap year.
#[inline]
pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Return the maximum number of days in the given month and year.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Parse a timezone specifier (e.g. `"UTC"`, `"+05:30"`, `"-04:00"`, `"EST"`) into a `FixedOffset` and normalized name.
pub fn parse_timezone(tz: &str) -> (FixedOffset, String) {
    let clean = tz.trim();
    if clean.is_empty()
        || clean.eq_ignore_ascii_case("utc")
        || clean.eq_ignore_ascii_case("gmt")
        || clean.eq_ignore_ascii_case("z")
    {
        return (FixedOffset::east_opt(0).unwrap(), "UTC".to_string());
    }

    // Try parsing explicit offsets like "+05:30", "-04:00", "+0000"
    if clean.starts_with('+') || clean.starts_with('-') {
        let sign = if clean.starts_with('-') { -1 } else { 1 };
        let rest = &clean[1..];
        let parts: Vec<&str> = rest.split(':').collect();
        if let Ok(hours) = parts[0].parse::<i32>() {
            let mins = if parts.len() > 1 {
                parts[1].parse::<i32>().unwrap_or(0)
            } else if rest.len() == 4 {
                rest[2..4].parse::<i32>().unwrap_or(0)
            } else {
                0
            };
            let total_secs = sign * (hours * 3600 + mins * 60);
            if let Some(offset) = FixedOffset::east_opt(total_secs) {
                return (offset, clean.to_string());
            }
        }
    }

    // Common named timezones
    let (offset_secs, canonical) = match clean.to_ascii_uppercase().as_str() {
        "EST" => (-5 * 3600, "EST"),
        "EDT" => (-4 * 3600, "EDT"),
        "CST" => (-6 * 3600, "CST"),
        "CDT" => (-5 * 3600, "CDT"),
        "MST" => (-7 * 3600, "MST"),
        "MDT" => (-6 * 3600, "MDT"),
        "PST" => (-8 * 3600, "PST"),
        "PDT" => (-7 * 3600, "PDT"),
        "IST" | "ASIA/KOLKATA" | "ASIA/CALCUTTA" => (5 * 3600 + 1800, "Asia/Kolkata"),
        "JST" | "ASIA/TOKYO" => (9 * 3600, "Asia/Tokyo"),
        "CET" | "EUROPE/PARIS" | "EUROPE/BERLIN" | "EUROPE/ROME" => (3600, "CET"),
        "EET" | "EUROPE/ATHENS" => (2 * 3600, "EET"),
        "BST" | "EUROPE/LONDON" => (3600, "BST"),
        "AEST" | "AUSTRALIA/SYDNEY" => (10 * 3600, "AEST"),
        _ => (0, "UTC"),
    };

    let offset = FixedOffset::east_opt(offset_secs).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
    (offset, canonical.to_string())
}

/// Representation of a Carbon date-time value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarbonDate {
    pub dt: DateTime<FixedOffset>,
    pub tz_name: String,
}

impl CarbonDate {
    /// Create a new `CarbonDate` from a `DateTime<FixedOffset>` and timezone name.
    pub fn new(dt: DateTime<FixedOffset>, tz_name: impl Into<String>) -> Self {
        Self {
            dt,
            tz_name: tz_name.into(),
        }
    }

    /// Construct `CarbonDate::now([$tz])`. If `TEST_NOW` is set, returns the mock test date.
    pub fn now(tz_opt: Option<&str>) -> Self {
        let mock_opt = TEST_NOW.with(|cell| cell.borrow().clone());
        if let Some(mock) = mock_opt {
            if let Some(tz) = tz_opt {
                return mock.set_timezone(tz);
            }
            return mock;
        }

        let (target_offset, tz_name) = match tz_opt {
            Some(tz) => parse_timezone(tz),
            None => {
                let local_offset = Local::now().offset().fix();
                (local_offset, "UTC".to_string())
            }
        };

        let now_utc = Utc::now();
        let dt = now_utc.with_timezone(&target_offset);
        Self { dt, tz_name }
    }

    /// Construct `CarbonDate::today([$tz])` at 00:00:00.
    pub fn today(tz_opt: Option<&str>) -> Self {
        Self::now(tz_opt).start_of_day()
    }

    /// Construct `CarbonDate::yesterday([$tz])` at 00:00:00.
    pub fn yesterday(tz_opt: Option<&str>) -> Self {
        Self::today(tz_opt).sub_days(1)
    }

    /// Construct `CarbonDate::tomorrow([$tz])` at 00:00:00.
    pub fn tomorrow(tz_opt: Option<&str>) -> Self {
        Self::today(tz_opt).add_days(1)
    }

    /// Construct `CarbonDate::create($year, $month, $day, $hour, $minute, $second, $micro, $tz)`.
    pub fn create(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
        micro: u32,
        tz_opt: Option<&str>,
    ) -> Result<Self, String> {
        let (offset, tz_name) = match tz_opt {
            Some(tz) => parse_timezone(tz),
            None => (FixedOffset::east_opt(0).unwrap(), "UTC".to_string()),
        };

        let naive_date = NaiveDate::from_ymd_opt(year, month, day)
            .ok_or_else(|| format!("Invalid date components: {year}-{month:02}-{day:02}"))?;
        let naive_time = NaiveTime::from_hms_micro_opt(hour, minute, second, micro)
            .ok_or_else(|| format!("Invalid time components: {hour:02}:{minute:02}:{second:02}.{micro:06}"))?;
        let naive_dt = NaiveDateTime::new(naive_date, naive_time);

        let dt = offset
            .from_local_datetime(&naive_dt)
            .single()
            .ok_or_else(|| "Failed to construct DateTime in timezone".to_string())?;

        Ok(Self { dt, tz_name })
    }

    /// Construct `CarbonDate::createFromTimestamp($timestamp, [$tz])`.
    pub fn create_from_timestamp(ts: i64, micro: u32, tz_opt: Option<&str>) -> Self {
        let (offset, tz_name) = match tz_opt {
            Some(tz) => parse_timezone(tz),
            None => (FixedOffset::east_opt(0).unwrap(), "UTC".to_string()),
        };

        let dt_utc = DateTime::from_timestamp(ts, micro * 1000)
            .unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap());
        let dt = dt_utc.with_timezone(&offset);
        Self { dt, tz_name }
    }

    /// Construct `CarbonDate::createFromTimestampMs($timestampMs, [$tz])`.
    pub fn create_from_timestamp_ms(ts_ms: i64, tz_opt: Option<&str>) -> Self {
        let secs = ts_ms / 1000;
        let millis = (ts_ms % 1000).unsigned_abs() as u32;
        Self::create_from_timestamp(secs, millis * 1000, tz_opt)
    }

    /// Construct `CarbonDate::createFromFormat($format, $time, [$tz])`.
    pub fn create_from_format(
        format: &str,
        time_str: &str,
        tz_opt: Option<&str>,
    ) -> Result<Self, String> {
        let chrono_fmt = convert_php_format_to_chrono(format);
        let (offset, tz_name) = match tz_opt {
            Some(tz) => parse_timezone(tz),
            None => (FixedOffset::east_opt(0).unwrap(), "UTC".to_string()),
        };

        if let Ok(parsed_dt) = DateTime::parse_from_str(time_str, &chrono_fmt) {
            return Ok(Self {
                dt: parsed_dt.with_timezone(&offset),
                tz_name,
            });
        }

        if let Ok(naive) = NaiveDateTime::parse_from_str(time_str, &chrono_fmt) {
            if let Some(dt) = offset.from_local_datetime(&naive).single() {
                return Ok(Self { dt, tz_name });
            }
        }

        if let Ok(naive_date) = NaiveDate::parse_from_str(time_str, &chrono_fmt) {
            let naive_dt = naive_date.and_hms_opt(0, 0, 0).unwrap();
            if let Some(dt) = offset.from_local_datetime(&naive_dt).single() {
                return Ok(Self { dt, tz_name });
            }
        }

        // Fallback to general parser if format doesn't match directly
        Self::parse(time_str, tz_opt)
    }

    /// Parse date/time string with support for ISO formats, relative strings, and timestamps.
    pub fn parse(input: &str, tz_opt: Option<&str>) -> Result<Self, String> {
        let raw = input.trim();
        if raw.is_empty() {
            return Err("Cannot parse empty date string into Carbon".to_string());
        }

        let (target_offset, tz_name) = match tz_opt {
            Some(tz) => parse_timezone(tz),
            None => (FixedOffset::east_opt(0).unwrap(), "UTC".to_string()),
        };

        // 1. Check if input is numeric timestamp
        if let Ok(ts) = raw.parse::<i64>() {
            if raw.len() >= 13 {
                return Ok(Self::create_from_timestamp_ms(ts, tz_opt));
            } else {
                return Ok(Self::create_from_timestamp(ts, 0, tz_opt));
            }
        }

        // 2. Relative keywords
        let lower = raw.to_ascii_lowercase();
        match lower.as_str() {
            "now" => return Ok(Self::now(tz_opt)),
            "today" => return Ok(Self::today(tz_opt)),
            "yesterday" => return Ok(Self::yesterday(tz_opt)),
            "tomorrow" => return Ok(Self::tomorrow(tz_opt)),
            "first day of this month" => return Ok(Self::now(tz_opt).start_of_month()),
            "last day of this month" => return Ok(Self::now(tz_opt).end_of_month()),
            "first day of next month" => return Ok(Self::now(tz_opt).add_months(1).start_of_month()),
            "last day of next month" => return Ok(Self::now(tz_opt).add_months(1).end_of_month()),
            "first day of last month" => return Ok(Self::now(tz_opt).sub_months(1).start_of_month()),
            "last day of last month" => return Ok(Self::now(tz_opt).sub_months(1).end_of_month()),
            _ => {}
        }

        // 3. Relative adjustments like "+1 day", "-2 weeks", "+3 months", "+1 year", "-5 hours"
        if let Some(rel) = parse_relative_offset(&lower, tz_opt) {
            return Ok(rel);
        }

        // 4. ISO 8601 / RFC 3339 formats
        if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
            return Ok(Self {
                dt: dt.with_timezone(&target_offset),
                tz_name,
            });
        }
        if let Ok(dt) = DateTime::parse_from_rfc2822(raw) {
            return Ok(Self {
                dt: dt.with_timezone(&target_offset),
                tz_name,
            });
        }

        // 5. Standard date/time format candidates
        let candidates = [
            "%Y-%m-%d %H:%M:%S%.f",
            "%Y-%m-%d %H:%M:%S",
            "%Y-%m-%d %H:%M",
            "%Y-%m-%d",
            "%Y/%m/%d %H:%M:%S%.f",
            "%Y/%m/%d %H:%M:%S",
            "%Y/%m/%d %H:%M",
            "%Y/%m/%d",
            "%d-%m-%Y %H:%M:%S%.f",
            "%d-%m-%Y %H:%M:%S",
            "%d-%m-%Y",
            "%d/%m/%Y %H:%M:%S%.f",
            "%d/%m/%Y %H:%M:%S",
            "%d/%m/%Y",
            "%m/%d/%Y %H:%M:%S",
            "%m/%d/%Y",
            "%Y-%m-%dT%H:%M:%S%.f",
            "%Y-%m-%dT%H:%M:%S",
        ];

        for pattern in &candidates {
            if let Ok(naive) = NaiveDateTime::parse_from_str(raw, pattern) {
                if let Some(dt) = target_offset.from_local_datetime(&naive).single() {
                    return Ok(Self { dt, tz_name });
                }
            }
            if let Ok(naive_date) = NaiveDate::parse_from_str(raw, pattern) {
                let naive_dt = naive_date.and_hms_opt(0, 0, 0).unwrap();
                if let Some(dt) = target_offset.from_local_datetime(&naive_dt).single() {
                    return Ok(Self { dt, tz_name });
                }
            }
        }

        // 6. Regex fallback for flexible YYYY-MM-DD or YYYY/MM/DD
        if let Some(c) = parse_flexible_regex(raw, target_offset, &tz_name) {
            return Ok(c);
        }

        Err(format!("Could not parse date string into Carbon: '{raw}'"))
    }

    /// Return the maximum representable Carbon date.
    pub fn max_value() -> Self {
        Self::create(9999, 12, 31, 23, 59, 59, 999_999, None).unwrap()
    }

    /// Return the minimum representable Carbon date.
    pub fn min_value() -> Self {
        Self::create(1, 1, 1, 0, 0, 0, 0, None).unwrap()
    }

    /// Set a mock test now date (`Carbon::setTestNow(...)`).
    pub fn set_test_now(date: CarbonDate) {
        TEST_NOW.with(|cell| {
            *cell.borrow_mut() = Some(date);
        });
    }

    /// Clear the mock test now date.
    pub fn clear_test_now() {
        TEST_NOW.with(|cell| {
            *cell.borrow_mut() = None;
        });
    }

    /// Check if a mock test now is active.
    pub fn has_test_now() -> bool {
        TEST_NOW.with(|cell| cell.borrow().is_some())
    }

    /// Get current mock test now if any.
    pub fn get_test_now() -> Option<CarbonDate> {
        TEST_NOW.with(|cell| cell.borrow().clone())
    }

    // =========================================================================
    // Getters & Properties
    // =========================================================================

    #[inline]
    pub fn year(&self) -> i32 {
        self.dt.year()
    }

    #[inline]
    pub fn month(&self) -> u32 {
        self.dt.month()
    }

    #[inline]
    pub fn day(&self) -> u32 {
        self.dt.day()
    }

    #[inline]
    pub fn hour(&self) -> u32 {
        self.dt.hour()
    }

    #[inline]
    pub fn minute(&self) -> u32 {
        self.dt.minute()
    }

    #[inline]
    pub fn second(&self) -> u32 {
        self.dt.second()
    }

    #[inline]
    pub fn micro(&self) -> u32 {
        self.dt.nanosecond() / 1000
    }

    #[inline]
    pub fn millisecond(&self) -> u32 {
        self.dt.nanosecond() / 1_000_000
    }

    #[inline]
    pub fn timestamp(&self) -> i64 {
        self.dt.timestamp()
    }

    #[inline]
    pub fn timestamp_ms(&self) -> i64 {
        self.dt.timestamp_millis()
    }

    #[inline]
    pub fn timezone_name(&self) -> &str {
        &self.tz_name
    }

    /// Day of week (0 for Sunday, 1 for Monday, ..., 6 for Saturday) matching PHP Carbon.
    #[inline]
    pub fn day_of_week(&self) -> u32 {
        self.dt.weekday().num_days_from_sunday()
    }

    /// ISO day of week (1 for Monday, ..., 7 for Sunday).
    #[inline]
    pub fn day_of_week_iso(&self) -> u32 {
        self.dt.weekday().number_from_monday()
    }

    #[inline]
    pub fn day_of_year(&self) -> u32 {
        self.dt.ordinal()
    }

    #[inline]
    pub fn week_of_year(&self) -> u32 {
        self.dt.iso_week().week()
    }

    #[inline]
    pub fn quarter(&self) -> u32 {
        ((self.dt.month() - 1) / 3) + 1
    }

    #[inline]
    pub fn days_in_month(&self) -> u32 {
        days_in_month(self.year(), self.month())
    }

    #[inline]
    pub fn age(&self) -> i32 {
        let now = Self::now(Some(&self.tz_name));
        let mut age = now.year() - self.year();
        if (now.month(), now.day()) < (self.month(), self.day()) {
            age -= 1;
        }
        age.max(0)
    }

    // =========================================================================
    // Formatting & String Outputs
    // =========================================================================

    /// `toDateTimeString()` -> `"YYYY-MM-DD HH:MM:SS"`.
    #[inline]
    pub fn to_date_time_string(&self) -> String {
        self.format("Y-m-d H:i:s")
    }

    /// `toDateString()` -> `"YYYY-MM-DD"`.
    #[inline]
    pub fn to_date_string(&self) -> String {
        self.format("Y-m-d")
    }

    /// `toDatString()` -> alias for `toDateString()`.
    #[inline]
    pub fn to_dat_string(&self) -> String {
        self.to_date_string()
    }

    /// `toTimeString()` -> `"HH:MM:SS"`.
    #[inline]
    pub fn to_time_string(&self) -> String {
        self.format("H:i:s")
    }

    /// `toISOString()` -> `"YYYY-MM-DDTHH:MM:SS.uuuuuuZ"`.
    #[inline]
    pub fn to_iso_string(&self) -> String {
        self.dt.to_rfc3339()
    }

    /// `toIso8601String()` / `toAtomString()`.
    #[inline]
    pub fn to_iso8601_string(&self) -> String {
        self.format("Y-m-d\\TH:i:sP")
    }

    /// `toRfc2822String()` / `toRfc822String()`.
    #[inline]
    pub fn to_rfc2822_string(&self) -> String {
        self.format("r")
    }

    /// `toRfc3339String()`.
    #[inline]
    pub fn to_rfc3339_string(&self) -> String {
        self.dt.to_rfc3339()
    }

    /// `toFormattedDateString()` -> `"M j, Y"` (e.g. `"Jan 15, 2024"`).
    #[inline]
    pub fn to_formatted_date_string(&self) -> String {
        self.format("M j, Y")
    }

    /// `toDayDateTimeString()` -> `"D, M j, Y g:i A"` (e.g. `"Mon, Jan 15, 2024 2:30 PM"`).
    #[inline]
    pub fn to_day_date_time_string(&self) -> String {
        self.format("D, M j, Y g:i A")
    }

    /// Format date string with PHP date token translation.
    pub fn format(&self, pattern: &str) -> String {
        let mut out = String::with_capacity(pattern.len() * 2);
        let mut chars = pattern.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '\\' {
                if let Some(escaped) = chars.next() {
                    out.push(escaped);
                }
                continue;
            }

            match ch {
                'Y' => out.push_str(&self.year().to_string()),
                'y' => out.push_str(&format!("{:02}", self.year().rem_euclid(100))),
                'm' => out.push_str(&format!("{:02}", self.month())),
                'n' => out.push_str(&self.month().to_string()),
                'd' => out.push_str(&format!("{:02}", self.day())),
                'j' => out.push_str(&self.day().to_string()),
                'H' => out.push_str(&format!("{:02}", self.hour())),
                'h' => {
                    let h = match self.hour() % 12 {
                        0 => 12,
                        h => h,
                    };
                    out.push_str(&format!("{:02}", h));
                }
                'G' => out.push_str(&self.hour().to_string()),
                'g' => {
                    let h = match self.hour() % 12 {
                        0 => 12,
                        h => h,
                    };
                    out.push_str(&h.to_string());
                }
                'i' => out.push_str(&format!("{:02}", self.minute())),
                's' => out.push_str(&format!("{:02}", self.second())),
                'u' => out.push_str(&format!("{:06}", self.micro())),
                'v' => out.push_str(&format!("{:03}", self.millisecond())),
                'a' => out.push_str(if self.hour() < 12 { "am" } else { "pm" }),
                'A' => out.push_str(if self.hour() < 12 { "AM" } else { "PM" }),
                'w' => out.push_str(&self.day_of_week().to_string()),
                'N' => out.push_str(&self.day_of_week_iso().to_string()),
                'D' => {
                    let name = match self.dt.weekday() {
                        Weekday::Mon => "Mon",
                        Weekday::Tue => "Tue",
                        Weekday::Wed => "Wed",
                        Weekday::Thu => "Thu",
                        Weekday::Fri => "Fri",
                        Weekday::Sat => "Sat",
                        Weekday::Sun => "Sun",
                    };
                    out.push_str(name);
                }
                'l' => {
                    let name = match self.dt.weekday() {
                        Weekday::Mon => "Monday",
                        Weekday::Tue => "Tuesday",
                        Weekday::Wed => "Wednesday",
                        Weekday::Thu => "Thursday",
                        Weekday::Fri => "Friday",
                        Weekday::Sat => "Saturday",
                        Weekday::Sun => "Sunday",
                    };
                    out.push_str(name);
                }
                'M' => {
                    let name = match self.month() {
                        1 => "Jan",
                        2 => "Feb",
                        3 => "Mar",
                        4 => "Apr",
                        5 => "May",
                        6 => "Jun",
                        7 => "Jul",
                        8 => "Aug",
                        9 => "Sep",
                        10 => "Oct",
                        11 => "Nov",
                        12 => "Dec",
                        _ => "",
                    };
                    out.push_str(name);
                }
                'F' => {
                    let name = match self.month() {
                        1 => "January",
                        2 => "February",
                        3 => "March",
                        4 => "April",
                        5 => "May",
                        6 => "June",
                        7 => "July",
                        8 => "August",
                        9 => "September",
                        10 => "October",
                        11 => "November",
                        12 => "December",
                        _ => "",
                    };
                    out.push_str(name);
                }
                'z' => out.push_str(&(self.day_of_year() - 1).to_string()),
                'W' => out.push_str(&self.week_of_year().to_string()),
                't' => out.push_str(&self.days_in_month().to_string()),
                'L' => out.push_str(if self.is_leap_year() { "1" } else { "0" }),
                'U' => out.push_str(&self.timestamp().to_string()),
                'c' => out.push_str(&self.dt.to_rfc3339()),
                'r' => out.push_str(&self.dt.to_rfc2822()),
                'O' => {
                    let total = self.dt.offset().local_minus_utc();
                    let sign = if total >= 0 { '+' } else { '-' };
                    let total = total.abs();
                    out.push_str(&format!("{}{:02}{:02}", sign, total / 3600, (total % 3600) / 60));
                }
                'P' => {
                    let total = self.dt.offset().local_minus_utc();
                    let sign = if total >= 0 { '+' } else { '-' };
                    let total = total.abs();
                    out.push_str(&format!("{}{:02}:{:02}", sign, total / 3600, (total % 3600) / 60));
                }
                'T' | 'e' => out.push_str(&self.tz_name),
                other => out.push(other),
            }
        }

        out
    }

    // =========================================================================
    // Arithmetic & Boundaries
    // =========================================================================

    #[must_use]
    pub fn add_days(&self, days: i64) -> Self {
        Self {
            dt: self.dt + Duration::days(days),
            tz_name: self.tz_name.clone(),
        }
    }

    #[must_use]
    pub fn sub_days(&self, days: i64) -> Self {
        self.add_days(-days)
    }

    #[must_use]
    pub fn add_weeks(&self, weeks: i64) -> Self {
        self.add_days(weeks * 7)
    }

    #[must_use]
    pub fn sub_weeks(&self, weeks: i64) -> Self {
        self.add_weeks(-weeks)
    }

    #[must_use]
    pub fn add_months(&self, months: i32) -> Self {
        let year = self.year();
        let month = self.month() as i32;
        let day = self.day();

        let total_months = (year as i64) * 12 + (month as i64 - 1) + (months as i64);
        let new_year = total_months.div_euclid(12) as i32;
        let new_month = (total_months.rem_euclid(12) + 1) as u32;
        let max_days = days_in_month(new_year, new_month);
        let new_day = day.min(max_days);

        let naive_date = NaiveDate::from_ymd_opt(new_year, new_month, new_day).unwrap();
        let naive_time = self.dt.time();
        let naive_dt = naive_date.and_time(naive_time);
        let dt = self.dt.offset().from_local_datetime(&naive_dt).single().unwrap_or(self.dt);

        Self {
            dt,
            tz_name: self.tz_name.clone(),
        }
    }

    #[must_use]
    pub fn sub_months(&self, months: i32) -> Self {
        self.add_months(-months)
    }

    #[must_use]
    pub fn add_years(&self, years: i32) -> Self {
        self.add_months(years * 12)
    }

    #[must_use]
    pub fn sub_years(&self, years: i32) -> Self {
        self.add_years(-years)
    }

    #[must_use]
    pub fn add_quarters(&self, quarters: i32) -> Self {
        self.add_months(quarters * 3)
    }

    #[must_use]
    pub fn sub_quarters(&self, quarters: i32) -> Self {
        self.add_quarters(-quarters)
    }

    #[must_use]
    pub fn add_decades(&self, decades: i32) -> Self {
        self.add_years(decades * 10)
    }

    #[must_use]
    pub fn sub_decades(&self, decades: i32) -> Self {
        self.add_decades(-decades)
    }

    #[must_use]
    pub fn add_centuries(&self, centuries: i32) -> Self {
        self.add_years(centuries * 100)
    }

    #[must_use]
    pub fn sub_centuries(&self, centuries: i32) -> Self {
        self.add_centuries(-centuries)
    }

    #[must_use]
    pub fn add_hours(&self, hours: i64) -> Self {
        Self {
            dt: self.dt + Duration::hours(hours),
            tz_name: self.tz_name.clone(),
        }
    }

    #[must_use]
    pub fn sub_hours(&self, hours: i64) -> Self {
        self.add_hours(-hours)
    }

    #[must_use]
    pub fn add_minutes(&self, minutes: i64) -> Self {
        Self {
            dt: self.dt + Duration::minutes(minutes),
            tz_name: self.tz_name.clone(),
        }
    }

    #[must_use]
    pub fn sub_minutes(&self, minutes: i64) -> Self {
        self.add_minutes(-minutes)
    }

    #[must_use]
    pub fn add_seconds(&self, seconds: i64) -> Self {
        Self {
            dt: self.dt + Duration::seconds(seconds),
            tz_name: self.tz_name.clone(),
        }
    }

    #[must_use]
    pub fn sub_seconds(&self, seconds: i64) -> Self {
        self.add_seconds(-seconds)
    }

    #[must_use]
    pub fn add_milliseconds(&self, ms: i64) -> Self {
        Self {
            dt: self.dt + Duration::milliseconds(ms),
            tz_name: self.tz_name.clone(),
        }
    }

    #[must_use]
    pub fn sub_milliseconds(&self, ms: i64) -> Self {
        self.add_milliseconds(-ms)
    }

    #[must_use]
    pub fn add_microseconds(&self, micros: i64) -> Self {
        Self {
            dt: self.dt + Duration::microseconds(micros),
            tz_name: self.tz_name.clone(),
        }
    }

    #[must_use]
    pub fn sub_microseconds(&self, micros: i64) -> Self {
        self.add_microseconds(-micros)
    }

    #[must_use]
    pub fn start_of_day(&self) -> Self {
        let naive_date = self.dt.date_naive();
        let naive_time = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
        let naive_dt = naive_date.and_time(naive_time);
        let dt = self.dt.offset().from_local_datetime(&naive_dt).single().unwrap_or(self.dt);
        Self {
            dt,
            tz_name: self.tz_name.clone(),
        }
    }

    #[must_use]
    pub fn end_of_day(&self) -> Self {
        let naive_date = self.dt.date_naive();
        let naive_time = NaiveTime::from_hms_opt(23, 59, 59).unwrap();
        let naive_dt = naive_date.and_time(naive_time);
        let dt = self.dt.offset().from_local_datetime(&naive_dt).single().unwrap_or(self.dt);
        Self {
            dt,
            tz_name: self.tz_name.clone(),
        }
    }

    #[must_use]
    pub fn start_of_month(&self) -> Self {
        let naive_date = NaiveDate::from_ymd_opt(self.year(), self.month(), 1).unwrap();
        let naive_time = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
        let naive_dt = naive_date.and_time(naive_time);
        let dt = self.dt.offset().from_local_datetime(&naive_dt).single().unwrap_or(self.dt);
        Self {
            dt,
            tz_name: self.tz_name.clone(),
        }
    }

    /// End of current month at 23:59:59.
    #[must_use]
    pub fn end_of_month(&self) -> Self {
        let last_day = self.days_in_month();
        let naive_date = NaiveDate::from_ymd_opt(self.year(), self.month(), last_day).unwrap();
        let naive_time = NaiveTime::from_hms_opt(23, 59, 59).unwrap();
        let naive_dt = naive_date.and_time(naive_time);
        let dt = self.dt.offset().from_local_datetime(&naive_dt).single().unwrap_or(self.dt);
        Self {
            dt,
            tz_name: self.tz_name.clone(),
        }
    }

    /// Alias for `end_of_month()`.
    #[must_use]
    pub fn last_of_month(&self) -> Self {
        self.end_of_month()
    }

    /// Alias for `start_of_month()`.
    #[must_use]
    pub fn first_of_month(&self) -> Self {
        self.start_of_month()
    }

    #[must_use]
    pub fn start_of_year(&self) -> Self {
        let naive_date = NaiveDate::from_ymd_opt(self.year(), 1, 1).unwrap();
        let naive_time = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
        let naive_dt = naive_date.and_time(naive_time);
        let dt = self.dt.offset().from_local_datetime(&naive_dt).single().unwrap_or(self.dt);
        Self {
            dt,
            tz_name: self.tz_name.clone(),
        }
    }

    #[must_use]
    pub fn end_of_year(&self) -> Self {
        let naive_date = NaiveDate::from_ymd_opt(self.year(), 12, 31).unwrap();
        let naive_time = NaiveTime::from_hms_opt(23, 59, 59).unwrap();
        let naive_dt = naive_date.and_time(naive_time);
        let dt = self.dt.offset().from_local_datetime(&naive_dt).single().unwrap_or(self.dt);
        Self {
            dt,
            tz_name: self.tz_name.clone(),
        }
    }

    #[must_use]
    pub fn start_of_week(&self) -> Self {
        let days_from_monday = self.dt.weekday().num_days_from_monday() as i64;
        self.sub_days(days_from_monday).start_of_day()
    }

    #[must_use]
    pub fn end_of_week(&self) -> Self {
        let days_to_sunday = 6 - (self.dt.weekday().num_days_from_monday() as i64);
        self.add_days(days_to_sunday).end_of_day()
    }

    #[must_use]
    pub fn start_of_quarter(&self) -> Self {
        let start_month = (self.quarter() - 1) * 3 + 1;
        let naive_date = NaiveDate::from_ymd_opt(self.year(), start_month, 1).unwrap();
        let naive_time = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
        let naive_dt = naive_date.and_time(naive_time);
        let dt = self.dt.offset().from_local_datetime(&naive_dt).single().unwrap_or(self.dt);
        Self {
            dt,
            tz_name: self.tz_name.clone(),
        }
    }

    #[must_use]
    pub fn end_of_quarter(&self) -> Self {
        let end_month = self.quarter() * 3;
        let max_days = days_in_month(self.year(), end_month);
        let naive_date = NaiveDate::from_ymd_opt(self.year(), end_month, max_days).unwrap();
        let naive_time = NaiveTime::from_hms_opt(23, 59, 59).unwrap();
        let naive_dt = naive_date.and_time(naive_time);
        let dt = self.dt.offset().from_local_datetime(&naive_dt).single().unwrap_or(self.dt);
        Self {
            dt,
            tz_name: self.tz_name.clone(),
        }
    }

    #[must_use]
    pub fn start_of_hour(&self) -> Self {
        let naive_time = NaiveTime::from_hms_opt(self.hour(), 0, 0).unwrap();
        let naive_dt = self.dt.date_naive().and_time(naive_time);
        let dt = self.dt.offset().from_local_datetime(&naive_dt).single().unwrap_or(self.dt);
        Self {
            dt,
            tz_name: self.tz_name.clone(),
        }
    }

    #[must_use]
    pub fn end_of_hour(&self) -> Self {
        let naive_time = NaiveTime::from_hms_opt(self.hour(), 59, 59).unwrap();
        let naive_dt = self.dt.date_naive().and_time(naive_time);
        let dt = self.dt.offset().from_local_datetime(&naive_dt).single().unwrap_or(self.dt);
        Self {
            dt,
            tz_name: self.tz_name.clone(),
        }
    }

    #[must_use]
    pub fn start_of_minute(&self) -> Self {
        let naive_time = NaiveTime::from_hms_opt(self.hour(), self.minute(), 0).unwrap();
        let naive_dt = self.dt.date_naive().and_time(naive_time);
        let dt = self.dt.offset().from_local_datetime(&naive_dt).single().unwrap_or(self.dt);
        Self {
            dt,
            tz_name: self.tz_name.clone(),
        }
    }

    #[must_use]
    pub fn end_of_minute(&self) -> Self {
        let naive_time = NaiveTime::from_hms_opt(self.hour(), self.minute(), 59).unwrap();
        let naive_dt = self.dt.date_naive().and_time(naive_time);
        let dt = self.dt.offset().from_local_datetime(&naive_dt).single().unwrap_or(self.dt);
        Self {
            dt,
            tz_name: self.tz_name.clone(),
        }
    }

    // =========================================================================
    // Modifiers / Setters
    // =========================================================================

    pub fn set_year(&self, year: i32) -> Result<Self, String> {
        let max_days = days_in_month(year, self.month());
        let day = self.day().min(max_days);
        Self::create(
            year,
            self.month(),
            day,
            self.hour(),
            self.minute(),
            self.second(),
            self.micro(),
            Some(&self.tz_name),
        )
    }

    pub fn set_month(&self, month: u32) -> Result<Self, String> {
        let max_days = days_in_month(self.year(), month);
        let day = self.day().min(max_days);
        Self::create(
            self.year(),
            month,
            day,
            self.hour(),
            self.minute(),
            self.second(),
            self.micro(),
            Some(&self.tz_name),
        )
    }

    pub fn set_day(&self, day: u32) -> Result<Self, String> {
        Self::create(
            self.year(),
            self.month(),
            day,
            self.hour(),
            self.minute(),
            self.second(),
            self.micro(),
            Some(&self.tz_name),
        )
    }

    pub fn set_hour(&self, hour: u32) -> Result<Self, String> {
        Self::create(
            self.year(),
            self.month(),
            self.day(),
            hour,
            self.minute(),
            self.second(),
            self.micro(),
            Some(&self.tz_name),
        )
    }

    pub fn set_minute(&self, minute: u32) -> Result<Self, String> {
        Self::create(
            self.year(),
            self.month(),
            self.day(),
            self.hour(),
            minute,
            self.second(),
            self.micro(),
            Some(&self.tz_name),
        )
    }

    pub fn set_second(&self, second: u32) -> Result<Self, String> {
        Self::create(
            self.year(),
            self.month(),
            self.day(),
            self.hour(),
            self.minute(),
            second,
            self.micro(),
            Some(&self.tz_name),
        )
    }

    pub fn set_time(&self, hour: u32, minute: u32, second: u32, micro: u32) -> Result<Self, String> {
        Self::create(
            self.year(),
            self.month(),
            self.day(),
            hour,
            minute,
            second,
            micro,
            Some(&self.tz_name),
        )
    }

    pub fn set_date(&self, year: i32, month: u32, day: u32) -> Result<Self, String> {
        Self::create(
            year,
            month,
            day,
            self.hour(),
            self.minute(),
            self.second(),
            self.micro(),
            Some(&self.tz_name),
        )
    }

    pub fn set_timezone(self, tz: &str) -> Self {
        let (new_offset, tz_name) = parse_timezone(tz);
        Self {
            dt: self.dt.with_timezone(&new_offset),
            tz_name,
        }
    }

    pub fn next(&self, day_of_week_opt: Option<u32>) -> Self {
        if let Some(target_dow) = day_of_week_opt {
            let current = self.day_of_week();
            let mut diff = ((target_dow as i32) - (current as i32)).rem_euclid(7);
            if diff == 0 {
                diff = 7;
            }
            self.add_days(diff as i64)
        } else {
            self.add_days(1)
        }
    }

    pub fn previous(&self, day_of_week_opt: Option<u32>) -> Self {
        if let Some(target_dow) = day_of_week_opt {
            let current = self.day_of_week();
            let mut diff = ((current as i32) - (target_dow as i32)).rem_euclid(7);
            if diff == 0 {
                diff = 7;
            }
            self.sub_days(diff as i64)
        } else {
            self.sub_days(1)
        }
    }

    // =========================================================================
    // Comparisons
    // =========================================================================

    #[inline]
    pub fn is_today(&self) -> bool {
        let now = Self::now(Some(&self.tz_name));
        self.year() == now.year() && self.month() == now.month() && self.day() == now.day()
    }

    #[inline]
    pub fn is_yesterday(&self) -> bool {
        let y = Self::yesterday(Some(&self.tz_name));
        self.year() == y.year() && self.month() == y.month() && self.day() == y.day()
    }

    #[inline]
    pub fn is_tomorrow(&self) -> bool {
        let t = Self::tomorrow(Some(&self.tz_name));
        self.year() == t.year() && self.month() == t.month() && self.day() == t.day()
    }

    #[inline]
    pub fn is_future(&self) -> bool {
        let now = Self::now(Some(&self.tz_name));
        self.dt > now.dt
    }

    #[inline]
    pub fn is_past(&self) -> bool {
        let now = Self::now(Some(&self.tz_name));
        self.dt < now.dt
    }

    #[inline]
    pub fn is_current_year(&self) -> bool {
        self.year() == Self::now(Some(&self.tz_name)).year()
    }

    #[inline]
    pub fn is_current_month(&self) -> bool {
        let now = Self::now(Some(&self.tz_name));
        self.year() == now.year() && self.month() == now.month()
    }

    #[inline]
    pub fn is_same_day(&self, other: &CarbonDate) -> bool {
        self.year() == other.year() && self.month() == other.month() && self.day() == other.day()
    }

    #[inline]
    pub fn is_same_month(&self, other: &CarbonDate) -> bool {
        self.year() == other.year() && self.month() == other.month()
    }

    #[inline]
    pub fn is_same_year(&self, other: &CarbonDate) -> bool {
        self.year() == other.year()
    }

    #[inline]
    pub fn is_weekend(&self) -> bool {
        self.day_of_week() == 0 || self.day_of_week() == 6
    }

    #[inline]
    pub fn is_weekday(&self) -> bool {
        !self.is_weekend()
    }

    #[inline]
    pub fn is_monday(&self) -> bool {
        self.dt.weekday() == Weekday::Mon
    }

    #[inline]
    pub fn is_tuesday(&self) -> bool {
        self.dt.weekday() == Weekday::Tue
    }

    #[inline]
    pub fn is_wednesday(&self) -> bool {
        self.dt.weekday() == Weekday::Wed
    }

    #[inline]
    pub fn is_thursday(&self) -> bool {
        self.dt.weekday() == Weekday::Thu
    }

    #[inline]
    pub fn is_friday(&self) -> bool {
        self.dt.weekday() == Weekday::Fri
    }

    #[inline]
    pub fn is_saturday(&self) -> bool {
        self.dt.weekday() == Weekday::Sat
    }

    #[inline]
    pub fn is_sunday(&self) -> bool {
        self.dt.weekday() == Weekday::Sun
    }

    #[inline]
    pub fn is_leap_year(&self) -> bool {
        is_leap_year(self.year())
    }

    #[inline]
    pub fn is_start_of_day(&self) -> bool {
        self.hour() == 0 && self.minute() == 0 && self.second() == 0
    }

    #[inline]
    pub fn is_end_of_day(&self) -> bool {
        self.hour() == 23 && self.minute() == 59 && self.second() == 59
    }

    #[inline]
    pub fn is_birthday(&self, other_opt: Option<&CarbonDate>) -> bool {
        let other = match other_opt {
            Some(o) => o.clone(),
            None => Self::now(Some(&self.tz_name)),
        };
        self.month() == other.month() && self.day() == other.day()
    }

    #[inline]
    pub fn eq(&self, other: &CarbonDate) -> bool {
        self.dt == other.dt
    }

    #[inline]
    pub fn ne(&self, other: &CarbonDate) -> bool {
        self.dt != other.dt
    }

    #[inline]
    pub fn gt(&self, other: &CarbonDate) -> bool {
        self.dt > other.dt
    }

    #[inline]
    pub fn gte(&self, other: &CarbonDate) -> bool {
        self.dt >= other.dt
    }

    #[inline]
    pub fn lt(&self, other: &CarbonDate) -> bool {
        self.dt < other.dt
    }

    #[inline]
    pub fn lte(&self, other: &CarbonDate) -> bool {
        self.dt <= other.dt
    }

    pub fn between(&self, start: &CarbonDate, end: &CarbonDate, equal: bool) -> bool {
        if equal {
            self.gte(start) && self.lte(end)
        } else {
            self.gt(start) && self.lt(end)
        }
    }

    // =========================================================================
    // Differences
    // =========================================================================

    pub fn diff_in_seconds(&self, other: &CarbonDate, abs: bool) -> i64 {
        let diff = self.timestamp() - other.timestamp();
        if abs {
            diff.abs()
        } else {
            diff
        }
    }

    pub fn diff_in_minutes(&self, other: &CarbonDate, abs: bool) -> i64 {
        self.diff_in_seconds(other, abs) / 60
    }

    pub fn diff_in_hours(&self, other: &CarbonDate, abs: bool) -> i64 {
        self.diff_in_seconds(other, abs) / 3600
    }

    pub fn diff_in_days(&self, other: &CarbonDate, abs: bool) -> i64 {
        self.diff_in_seconds(other, abs) / 86400
    }

    pub fn diff_in_weeks(&self, other: &CarbonDate, abs: bool) -> i64 {
        self.diff_in_days(other, abs) / 7
    }

    pub fn diff_in_months(&self, other: &CarbonDate, abs: bool) -> i64 {
        let diff = (self.year() as i64 - other.year() as i64) * 12
            + (self.month() as i64 - other.month() as i64);
        if abs {
            diff.abs()
        } else {
            diff
        }
    }

    pub fn diff_in_quarters(&self, other: &CarbonDate, abs: bool) -> i64 {
        self.diff_in_months(other, abs) / 3
    }

    pub fn diff_in_years(&self, other: &CarbonDate, abs: bool) -> i64 {
        self.diff_in_months(other, abs) / 12
    }

    /// Human-readable time diff (`diffForHumans()`).
    pub fn diff_for_humans(&self, other_opt: Option<&CarbonDate>) -> String {
        let other = match other_opt {
            Some(o) => o.clone(),
            None => Self::now(Some(&self.tz_name)),
        };

        let is_future = self.gt(&other);
        let secs = self.diff_in_seconds(&other, true);

        if secs < 4 {
            return "just now".to_string();
        }

        let (count, unit) = if secs < 60 {
            (secs, "second")
        } else if secs < 3600 {
            (secs / 60, "minute")
        } else if secs < 86400 {
            (secs / 3600, "hour")
        } else if secs < 86400 * 7 {
            (secs / 86400, "day")
        } else if secs < 86400 * 30 {
            (secs / (86400 * 7), "week")
        } else if secs < 86400 * 365 {
            (secs / (86400 * 30), "month")
        } else {
            (secs / (86400 * 365), "year")
        };

        let unit_str = if count == 1 {
            unit.to_string()
        } else {
            format!("{unit}s")
        };

        if is_future {
            format!("in {count} {unit_str}")
        } else {
            format!("{count} {unit_str} ago")
        }
    }

    // =========================================================================
    // VmObject Synchronization
    // =========================================================================

    /// Convert `CarbonDate` into a full `VmObject` of class `"Carbon"`.
    pub fn to_vm_object(&self) -> VmObject {
        let mut obj = VmObject::new("Carbon");
        self.sync_properties(&mut obj);
        obj
    }

    /// Synchronize all standard Carbon public properties into the given `VmObject`.
    pub fn sync_properties(&self, obj: &mut VmObject) {
        let formatted = self.to_date_time_string();
        let iso = self.to_iso_string();
        obj.set_property("formatted", PhpValue::string(&formatted));
        obj.set_property("iso", PhpValue::string(&iso));
        obj.set_property("year", PhpValue::Int(self.year()));
        obj.set_property("month", PhpValue::Int(self.month() as i32));
        obj.set_property("day", PhpValue::Int(self.day() as i32));
        obj.set_property("hour", PhpValue::Int(self.hour() as i32));
        obj.set_property("minute", PhpValue::Int(self.minute() as i32));
        obj.set_property("second", PhpValue::Int(self.second() as i32));
        obj.set_property("micro", PhpValue::Int(self.micro() as i32));
        obj.set_property("timestamp", PhpValue::RustInt { value: self.timestamp() as i128, kind: RustIntKind::I64 });
        obj.set_property("timezone", PhpValue::string(&self.tz_name));
        obj.set_property("dayOfWeek", PhpValue::Int(self.day_of_week() as i32));
        obj.set_property("dayOfYear", PhpValue::Int(self.day_of_year() as i32));
        obj.set_property("weekOfYear", PhpValue::Int(self.week_of_year() as i32));
        obj.set_property("quarter", PhpValue::Int(self.quarter() as i32));
        obj.set_property("daysInMonth", PhpValue::Int(self.days_in_month() as i32));
    }
}

/// Convert a `PhpValue` argument into a `CarbonDate`.
pub fn from_php_value(val: &PhpValue) -> Result<CarbonDate, String> {
    match val {
        PhpValue::Object(rc) => {
            let obj = rc.borrow();
            if obj.class_name == "Carbon" {
                if let Some(ts_val) = obj.get_property("timestamp") {
                    let ts = ts_val.to_i64().unwrap_or(0);
                    let micro = obj.get_property("micro").and_then(|v| v.to_i64()).unwrap_or(0) as u32;
                    let tz = obj
                        .get_property("timezone")
                        .map(|v| v.to_php_string())
                        .unwrap_or_else(|| "UTC".to_string());
                    return Ok(CarbonDate::create_from_timestamp(ts, micro, Some(&tz)));
                } else if let Some(formatted) = obj.get_property("formatted") {
                    let tz = obj
                        .get_property("timezone")
                        .map(|v| v.to_php_string())
                        .unwrap_or_else(|| "UTC".to_string());
                    return CarbonDate::parse(&formatted.to_php_string(), Some(&tz));
                }
            }
            Err("Expected an instance of Carbon".to_string())
        }
        PhpValue::String(s) => CarbonDate::parse(s, None),
        PhpValue::Int(n) => Ok(CarbonDate::create_from_timestamp(*n as i64, 0, None)),
        PhpValue::RustInt { value, .. } => Ok(CarbonDate::create_from_timestamp(*value as i64, 0, None)),
        _ => Err("Invalid value provided for Carbon date conversion".to_string()),
    }
}

/// Get static class constant for `Carbon::*` (e.g. `Carbon::MONDAY`, `Carbon::SUNDAY`).
pub fn get_carbon_constant(name: &str) -> Option<PhpValue> {
    match name {
        "SUNDAY" => Some(PhpValue::Int(0)),
        "MONDAY" => Some(PhpValue::Int(1)),
        "TUESDAY" => Some(PhpValue::Int(2)),
        "WEDNESDAY" => Some(PhpValue::Int(3)),
        "THURSDAY" => Some(PhpValue::Int(4)),
        "FRIDAY" => Some(PhpValue::Int(5)),
        "SATURDAY" => Some(PhpValue::Int(6)),
        "YEARS_PER_CENTURY" => Some(PhpValue::Int(100)),
        "YEARS_PER_DECADE" => Some(PhpValue::Int(10)),
        "MONTHS_PER_YEAR" => Some(PhpValue::Int(12)),
        "WEEKS_PER_YEAR" => Some(PhpValue::Int(52)),
        "DAYS_PER_WEEK" => Some(PhpValue::Int(7)),
        "HOURS_PER_DAY" => Some(PhpValue::Int(24)),
        "MINUTES_PER_HOUR" => Some(PhpValue::Int(60)),
        "SECONDS_PER_MINUTE" => Some(PhpValue::Int(60)),
        _ => None,
    }
}

/// Convert standard PHP date format specifiers to chrono format specifiers.
fn convert_php_format_to_chrono(php_fmt: &str) -> String {
    let mut out = String::with_capacity(php_fmt.len() * 2);
    let mut chars = php_fmt.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                out.push(escaped);
            }
            continue;
        }
        match ch {
            'Y' => out.push_str("%Y"),
            'y' => out.push_str("%y"),
            'm' => out.push_str("%m"),
            'd' => out.push_str("%d"),
            'H' => out.push_str("%H"),
            'h' => out.push_str("%I"),
            'i' => out.push_str("%M"),
            's' => out.push_str("%S"),
            'u' => out.push_str("%6f"),
            'v' => out.push_str("%3f"),
            'a' | 'A' => out.push_str("%p"),
            'c' => out.push_str("%+"),
            'r' => out.push_str("%a, %d %b %Y %H:%M:%S %z"),
            other => out.push(other),
        }
    }
    out
}

/// Helper to parse relative time phrases (e.g., `"+1 day"`, `"-2 weeks"`, `"+3 months"`, `"+1 year"`).
fn parse_relative_offset(lower: &str, tz_opt: Option<&str>) -> Option<CarbonDate> {
    let trimmed = lower.trim();
    let (sign, rest) = if let Some(stripped) = trimmed.strip_prefix('+') {
        (1i64, stripped.trim())
    } else if let Some(stripped) = trimmed.strip_prefix('-') {
        (-1i64, stripped.trim())
    } else {
        (1i64, trimmed)
    };

    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() != 2 {
        return None;
    }

    let qty = parts[0].parse::<i64>().ok()? * sign;
    let unit = parts[1].trim_end_matches('s');

    let base = CarbonDate::now(tz_opt);
    let result = match unit {
        "day" => base.add_days(qty),
        "week" => base.add_weeks(qty),
        "month" => base.add_months(qty as i32),
        "year" => base.add_years(qty as i32),
        "quarter" => base.add_quarters(qty as i32),
        "hour" => base.add_hours(qty),
        "minute" | "min" => base.add_minutes(qty),
        "second" | "sec" => base.add_seconds(qty),
        _ => return None,
    };
    Some(result)
}

/// Fallback regex parser for dates.
fn parse_flexible_regex(raw: &str, _offset: FixedOffset, tz_name: &str) -> Option<CarbonDate> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"^(\d{4})[-/.](\d{1,2})[-/.](\d{1,2})(?:[ T](\d{1,2}):(\d{1,2})(?::(\d{1,2}))?)?$").unwrap()
    });

    let caps = re.captures(raw)?;
    let y = caps[1].parse::<i32>().ok()?;
    let m = caps[2].parse::<u32>().ok()?;
    let d = caps[3].parse::<u32>().ok()?;
    let h = caps.get(4).map(|v| v.as_str().parse::<u32>().unwrap_or(0)).unwrap_or(0);
    let min = caps.get(5).map(|v| v.as_str().parse::<u32>().unwrap_or(0)).unwrap_or(0);
    let s = caps.get(6).map(|v| v.as_str().parse::<u32>().unwrap_or(0)).unwrap_or(0);

    CarbonDate::create(y, m, d, h, min, s, 0, Some(tz_name)).ok()
}

// =========================================================================
// Dispatch Static Facade Calls: Carbon::*
// =========================================================================

pub fn dispatch_carbon(method: &str, args: &[PhpValue]) -> FacadeResult {
    // Dynamic macro registration
    if method == "macro" {
        let name = get_arg_str(args, 0, "Carbon::macro")?;
        let closure = match args.get(1) {
            Some(PhpValue::Closure(c)) => c,
            _ => {
                return Err(RuntimeErrorKind::Custom {
                    message: format!(
                        "TypeError: Carbon::macro() expects argument 2 to be a Closure, got {}",
                        args.get(1).map(|v| v.type_name()).unwrap_or("none")
                    ),
                });
            }
        };
        register_runtime_closure_macro("Carbon", &name, (**closure).clone());
        return Ok(PhpValue::Null);
    }

    match method {
        "now" => {
            let tz = args.first().map(|v| v.to_php_string());
            let c = CarbonDate::now(tz.as_deref());
            Ok(PhpValue::object(c.to_vm_object()))
        }
        "today" => {
            let tz = args.first().map(|v| v.to_php_string());
            let c = CarbonDate::today(tz.as_deref());
            Ok(PhpValue::object(c.to_vm_object()))
        }
        "yesterday" => {
            let tz = args.first().map(|v| v.to_php_string());
            let c = CarbonDate::yesterday(tz.as_deref());
            Ok(PhpValue::object(c.to_vm_object()))
        }
        "tomorrow" => {
            let tz = args.first().map(|v| v.to_php_string());
            let c = CarbonDate::tomorrow(tz.as_deref());
            Ok(PhpValue::object(c.to_vm_object()))
        }
        "parse" => {
            let s = get_arg_str(args, 0, "Carbon::parse")?;
            let tz = args.get(1).map(|v| v.to_php_string());
            match CarbonDate::parse(&s, tz.as_deref()) {
                Ok(c) => Ok(PhpValue::object(c.to_vm_object())),
                Err(err) => Err(RuntimeErrorKind::Custom { message: err }),
            }
        }
        "create" => {
            let year = args.first().and_then(|v| v.to_i64()).unwrap_or(0) as i32;
            let month = args.get(1).and_then(|v| v.to_i64()).unwrap_or(1) as u32;
            let day = args.get(2).and_then(|v| v.to_i64()).unwrap_or(1) as u32;
            let hour = args.get(3).and_then(|v| v.to_i64()).unwrap_or(0) as u32;
            let min = args.get(4).and_then(|v| v.to_i64()).unwrap_or(0) as u32;
            let sec = args.get(5).and_then(|v| v.to_i64()).unwrap_or(0) as u32;
            let tz = args.get(6).map(|v| v.to_php_string());
            match CarbonDate::create(year, month, day, hour, min, sec, 0, tz.as_deref()) {
                Ok(c) => Ok(PhpValue::object(c.to_vm_object())),
                Err(e) => Err(RuntimeErrorKind::Custom { message: e }),
            }
        }
        "createFromDate" => {
            let year = args.first().and_then(|v| v.to_i64()).unwrap_or(0) as i32;
            let month = args.get(1).and_then(|v| v.to_i64()).unwrap_or(1) as u32;
            let day = args.get(2).and_then(|v| v.to_i64()).unwrap_or(1) as u32;
            let tz = args.get(3).map(|v| v.to_php_string());
            match CarbonDate::create(year, month, day, 0, 0, 0, 0, tz.as_deref()) {
                Ok(c) => Ok(PhpValue::object(c.to_vm_object())),
                Err(e) => Err(RuntimeErrorKind::Custom { message: e }),
            }
        }
        "createFromTime" => {
            let hour = args.first().and_then(|v| v.to_i64()).unwrap_or(0) as u32;
            let min = args.get(1).and_then(|v| v.to_i64()).unwrap_or(0) as u32;
            let sec = args.get(2).and_then(|v| v.to_i64()).unwrap_or(0) as u32;
            let tz = args.get(3).map(|v| v.to_php_string());
            let today = CarbonDate::today(tz.as_deref());
            match today.set_time(hour, min, sec, 0) {
                Ok(c) => Ok(PhpValue::object(c.to_vm_object())),
                Err(e) => Err(RuntimeErrorKind::Custom { message: e }),
            }
        }
        "createFromTimestamp" => {
            let ts = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
            let tz = args.get(1).map(|v| v.to_php_string());
            let c = CarbonDate::create_from_timestamp(ts, 0, tz.as_deref());
            Ok(PhpValue::object(c.to_vm_object()))
        }
        "createFromTimestampMs" => {
            let ts_ms = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
            let tz = args.get(1).map(|v| v.to_php_string());
            let c = CarbonDate::create_from_timestamp_ms(ts_ms, tz.as_deref());
            Ok(PhpValue::object(c.to_vm_object()))
        }
        "createFromFormat" => {
            let fmt = get_arg_str(args, 0, "Carbon::createFromFormat")?;
            let time_str = get_arg_str(args, 1, "Carbon::createFromFormat")?;
            let tz = args.get(2).map(|v| v.to_php_string());
            match CarbonDate::create_from_format(&fmt, &time_str, tz.as_deref()) {
                Ok(c) => Ok(PhpValue::object(c.to_vm_object())),
                Err(e) => Err(RuntimeErrorKind::Custom { message: e }),
            }
        }
        "maxValue" => Ok(PhpValue::object(CarbonDate::max_value().to_vm_object())),
        "minValue" => Ok(PhpValue::object(CarbonDate::min_value().to_vm_object())),
        "setTestNow" => {
            if args.is_empty() || args[0].is_null() {
                CarbonDate::clear_test_now();
            } else {
                let parsed = from_php_value(&args[0])
                    .map_err(|e| RuntimeErrorKind::Custom { message: e })?;
                CarbonDate::set_test_now(parsed);
            }
            Ok(PhpValue::Null)
        }
        "getTestNow" => {
            if let Some(tn) = CarbonDate::get_test_now() {
                Ok(PhpValue::object(tn.to_vm_object()))
            } else {
                Ok(PhpValue::Null)
            }
        }
        "hasTestNow" => Ok(PhpValue::Bool(CarbonDate::has_test_now())),
        // Static invocations of instance methods operate on `Carbon::now()`
        _ => {
            let now = CarbonDate::now(None);
            let receiver = PhpValue::object(now.to_vm_object());
            exec_carbon_instance_method(&receiver, method, args, Span::new(0, 0, 0, 1, 1))
                .map_err(|e| e.kind)
        }
    }
}

// =========================================================================
// Dispatch Object Instance Calls: $carbon->*
// =========================================================================

pub fn exec_carbon_instance_method(
    receiver: &PhpValue,
    method: &str,
    args: &[PhpValue],
    span: Span,
) -> Result<PhpValue, RuntimeError> {
    let rc = match receiver {
        PhpValue::Object(rc) => rc,
        _ => {
            return Err(RuntimeError::new(
                RuntimeErrorKind::Custom {
                    message: "Method call on non-object receiver".to_string(),
                },
                span,
            ));
        }
    };

    let mut current = from_php_value(receiver).map_err(|e| {
        RuntimeError::new(RuntimeErrorKind::Custom { message: e }, span)
    })?;

    match method {
        // --- Output & Formatting ---
        "toDateTimeString" => Ok(PhpValue::string(current.to_date_time_string())),
        "toDateString" | "toDatString" => Ok(PhpValue::string(current.to_date_string())),
        "toTimeString" => Ok(PhpValue::string(current.to_time_string())),
        "toISOString" => Ok(PhpValue::string(current.to_iso_string())),
        "toIso8601String" | "toAtomString" => Ok(PhpValue::string(current.to_iso8601_string())),
        "toRfc2822String" | "toRfc822String" => Ok(PhpValue::string(current.to_rfc2822_string())),
        "toRfc3339String" => Ok(PhpValue::string(current.to_rfc3339_string())),
        "toFormattedDateString" => Ok(PhpValue::string(current.to_formatted_date_string())),
        "toDayDateTimeString" => Ok(PhpValue::string(current.to_day_date_time_string())),
        "toJson" => Ok(PhpValue::string(format!("\"{}\"", current.to_iso_string()))),
        "format" => {
            let fmt = args.first().map(|v| v.to_php_string()).unwrap_or_else(|| "Y-m-d H:i:s".to_string());
            Ok(PhpValue::string(current.format(&fmt)))
        }

        // --- Getters & Queries ---
        "getTimestamp" | "timestamp" => Ok(PhpValue::Int(current.timestamp() as i32)),
        "getTimestampMs" | "timestampMs" => Ok(PhpValue::RustInt {
            value: current.timestamp_ms() as i128,
            kind: RustIntKind::I64,
        }),
        "getTimezone" | "timezoneName" | "getTimezoneName" => {
            Ok(PhpValue::string(current.timezone_name()))
        }
        "year" => Ok(PhpValue::Int(current.year())),
        "month" => Ok(PhpValue::Int(current.month() as i32)),
        "day" => Ok(PhpValue::Int(current.day() as i32)),
        "hour" => Ok(PhpValue::Int(current.hour() as i32)),
        "minute" => Ok(PhpValue::Int(current.minute() as i32)),
        "second" => Ok(PhpValue::Int(current.second() as i32)),
        "micro" | "microsecond" => Ok(PhpValue::Int(current.micro() as i32)),
        "millisecond" => Ok(PhpValue::Int(current.millisecond() as i32)),
        "dayOfWeek" => Ok(PhpValue::Int(current.day_of_week() as i32)),
        "dayOfWeekIso" => Ok(PhpValue::Int(current.day_of_week_iso() as i32)),
        "dayOfYear" => Ok(PhpValue::Int(current.day_of_year() as i32)),
        "weekOfYear" => Ok(PhpValue::Int(current.week_of_year() as i32)),
        "quarter" => Ok(PhpValue::Int(current.quarter() as i32)),
        "daysInMonth" => Ok(PhpValue::Int(current.days_in_month() as i32)),
        "age" => Ok(PhpValue::Int(current.age())),

        // --- Comparisons ---
        "isToday" => Ok(PhpValue::Bool(current.is_today())),
        "isYesterday" => Ok(PhpValue::Bool(current.is_yesterday())),
        "isTomorrow" => Ok(PhpValue::Bool(current.is_tomorrow())),
        "isFuture" => Ok(PhpValue::Bool(current.is_future())),
        "isPast" => Ok(PhpValue::Bool(current.is_past())),
        "isCurrentYear" => Ok(PhpValue::Bool(current.is_current_year())),
        "isCurrentMonth" => Ok(PhpValue::Bool(current.is_current_month())),
        "isWeekend" => Ok(PhpValue::Bool(current.is_weekend())),
        "isWeekday" => Ok(PhpValue::Bool(current.is_weekday())),
        "isMonday" => Ok(PhpValue::Bool(current.is_monday())),
        "isTuesday" => Ok(PhpValue::Bool(current.is_tuesday())),
        "isWednesday" => Ok(PhpValue::Bool(current.is_wednesday())),
        "isThursday" => Ok(PhpValue::Bool(current.is_thursday())),
        "isFriday" => Ok(PhpValue::Bool(current.is_friday())),
        "isSaturday" => Ok(PhpValue::Bool(current.is_saturday())),
        "isSunday" => Ok(PhpValue::Bool(current.is_sunday())),
        "isLeapYear" => Ok(PhpValue::Bool(current.is_leap_year())),
        "isStartOfDay" | "isMidnight" => Ok(PhpValue::Bool(current.is_start_of_day())),
        "isEndOfDay" => Ok(PhpValue::Bool(current.is_end_of_day())),
        "isSameDay" => {
            let other = parse_arg_date(args, 0, span)?;
            Ok(PhpValue::Bool(current.is_same_day(&other)))
        }
        "isSameMonth" => {
            let other = parse_arg_date(args, 0, span)?;
            Ok(PhpValue::Bool(current.is_same_month(&other)))
        }
        "isSameYear" => {
            let other = parse_arg_date(args, 0, span)?;
            Ok(PhpValue::Bool(current.is_same_year(&other)))
        }
        "isBirthday" => {
            let other = if args.is_empty() {
                None
            } else {
                Some(parse_arg_date(args, 0, span)?)
            };
            Ok(PhpValue::Bool(current.is_birthday(other.as_ref())))
        }
        "eq" | "equalTo" => {
            let other = parse_arg_date(args, 0, span)?;
            Ok(PhpValue::Bool(current.eq(&other)))
        }
        "ne" | "notEqualTo" => {
            let other = parse_arg_date(args, 0, span)?;
            Ok(PhpValue::Bool(current.ne(&other)))
        }
        "gt" | "greaterThan" => {
            let other = parse_arg_date(args, 0, span)?;
            Ok(PhpValue::Bool(current.gt(&other)))
        }
        "gte" | "greaterThanOrEqualTo" => {
            let other = parse_arg_date(args, 0, span)?;
            Ok(PhpValue::Bool(current.gte(&other)))
        }
        "lt" | "lessThan" => {
            let other = parse_arg_date(args, 0, span)?;
            Ok(PhpValue::Bool(current.lt(&other)))
        }
        "lte" | "lessThanOrEqualTo" => {
            let other = parse_arg_date(args, 0, span)?;
            Ok(PhpValue::Bool(current.lte(&other)))
        }
        "between" => {
            let start = parse_arg_date(args, 0, span)?;
            let end = parse_arg_date(args, 1, span)?;
            let equal = args.get(2).and_then(|v| v.to_bool()).unwrap_or(true);
            Ok(PhpValue::Bool(current.between(&start, &end, equal)))
        }

        // --- Differences ---
        "diffInSeconds" => {
            let other = parse_arg_date_or_now(args, 0, &current.tz_name, span)?;
            let abs = args.get(1).and_then(|v| v.to_bool()).unwrap_or(true);
            Ok(PhpValue::Int(current.diff_in_seconds(&other, abs) as i32))
        }
        "diffInMinutes" => {
            let other = parse_arg_date_or_now(args, 0, &current.tz_name, span)?;
            let abs = args.get(1).and_then(|v| v.to_bool()).unwrap_or(true);
            Ok(PhpValue::Int(current.diff_in_minutes(&other, abs) as i32))
        }
        "diffInHours" => {
            let other = parse_arg_date_or_now(args, 0, &current.tz_name, span)?;
            let abs = args.get(1).and_then(|v| v.to_bool()).unwrap_or(true);
            Ok(PhpValue::Int(current.diff_in_hours(&other, abs) as i32))
        }
        "diffInDays" => {
            let other = parse_arg_date_or_now(args, 0, &current.tz_name, span)?;
            let abs = args.get(1).and_then(|v| v.to_bool()).unwrap_or(true);
            Ok(PhpValue::Int(current.diff_in_days(&other, abs) as i32))
        }
        "diffInWeeks" => {
            let other = parse_arg_date_or_now(args, 0, &current.tz_name, span)?;
            let abs = args.get(1).and_then(|v| v.to_bool()).unwrap_or(true);
            Ok(PhpValue::Int(current.diff_in_weeks(&other, abs) as i32))
        }
        "diffInMonths" => {
            let other = parse_arg_date_or_now(args, 0, &current.tz_name, span)?;
            let abs = args.get(1).and_then(|v| v.to_bool()).unwrap_or(true);
            Ok(PhpValue::Int(current.diff_in_months(&other, abs) as i32))
        }
        "diffInQuarters" => {
            let other = parse_arg_date_or_now(args, 0, &current.tz_name, span)?;
            let abs = args.get(1).and_then(|v| v.to_bool()).unwrap_or(true);
            Ok(PhpValue::Int(current.diff_in_quarters(&other, abs) as i32))
        }
        "diffInYears" => {
            let other = parse_arg_date_or_now(args, 0, &current.tz_name, span)?;
            let abs = args.get(1).and_then(|v| v.to_bool()).unwrap_or(true);
            Ok(PhpValue::Int(current.diff_in_years(&other, abs) as i32))
        }
        "diffForHumans" => {
            let other = if args.is_empty() || args[0].is_null() {
                None
            } else {
                Some(parse_arg_date(args, 0, span)?)
            };
            Ok(PhpValue::string(current.diff_for_humans(other.as_ref())))
        }

        // --- Copy & Clone ---
        "copy" | "clone" => {
            let clone = current.clone();
            Ok(PhpValue::object(clone.to_vm_object()))
        }

        // --- Min / Max Helpers ---
        "min" => {
            let other = parse_arg_date_or_now(args, 0, &current.tz_name, span)?;
            if current.lt(&other) {
                Ok(receiver.clone())
            } else {
                Ok(PhpValue::object(other.to_vm_object()))
            }
        }
        "max" => {
            let other = parse_arg_date_or_now(args, 0, &current.tz_name, span)?;
            if current.gt(&other) {
                Ok(receiver.clone())
            } else {
                Ok(PhpValue::object(other.to_vm_object()))
            }
        }

        // --- Modifiers & Fluent Arithmetic (Mutates and returns $this) ---
        "addDay" | "addDays" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1);
            current = current.add_days(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "subDay" | "subDays" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1);
            current = current.sub_days(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "addWeek" | "addWeeks" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1);
            current = current.add_weeks(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "subWeek" | "subWeeks" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1);
            current = current.sub_weeks(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "addMonth" | "addMonths" | "addMonthsNoOverflow" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1) as i32;
            current = current.add_months(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "subMonth" | "subMonths" | "subMonthsNoOverflow" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1) as i32;
            current = current.sub_months(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "addQuarter" | "addQuarters" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1) as i32;
            current = current.add_quarters(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "subQuarter" | "subQuarters" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1) as i32;
            current = current.sub_quarters(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "addYear" | "addYears" | "addYearsNoOverflow" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1) as i32;
            current = current.add_years(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "subYear" | "subYears" | "subYearsNoOverflow" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1) as i32;
            current = current.sub_years(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "addDecade" | "addDecades" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1) as i32;
            current = current.add_decades(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "subDecade" | "subDecades" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1) as i32;
            current = current.sub_decades(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "addCentury" | "addCenturies" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1) as i32;
            current = current.add_centuries(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "subCentury" | "subCenturies" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1) as i32;
            current = current.sub_centuries(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "addHour" | "addHours" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1);
            current = current.add_hours(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "subHour" | "subHours" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1);
            current = current.sub_hours(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "addMinute" | "addMinutes" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1);
            current = current.add_minutes(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "subMinute" | "subMinutes" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1);
            current = current.sub_minutes(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "addSecond" | "addSeconds" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1);
            current = current.add_seconds(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "subSecond" | "subSeconds" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1);
            current = current.sub_seconds(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "addMilliseconds" | "addMillisecond" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1);
            current = current.add_milliseconds(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "subMilliseconds" | "subMillisecond" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1);
            current = current.sub_milliseconds(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "addMicroseconds" | "addMicrosecond" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1);
            current = current.add_microseconds(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "subMicroseconds" | "subMicrosecond" => {
            let n = args.first().and_then(|v| v.to_i64()).unwrap_or(1);
            current = current.sub_microseconds(n);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "addUnit" => {
            let unit = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            let qty = args.get(1).and_then(|v| v.to_i64()).unwrap_or(1);
            current = match unit.trim_end_matches('s').to_ascii_lowercase().as_str() {
                "day" => current.add_days(qty),
                "week" => current.add_weeks(qty),
                "month" => current.add_months(qty as i32),
                "year" => current.add_years(qty as i32),
                "quarter" => current.add_quarters(qty as i32),
                "hour" => current.add_hours(qty),
                "minute" => current.add_minutes(qty),
                "second" => current.add_seconds(qty),
                _ => current,
            };
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "subUnit" => {
            let unit = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            let qty = args.get(1).and_then(|v| v.to_i64()).unwrap_or(1);
            current = match unit.trim_end_matches('s').to_ascii_lowercase().as_str() {
                "day" => current.sub_days(qty),
                "week" => current.sub_weeks(qty),
                "month" => current.sub_months(qty as i32),
                "year" => current.sub_years(qty as i32),
                "quarter" => current.sub_quarters(qty as i32),
                "hour" => current.sub_hours(qty),
                "minute" => current.sub_minutes(qty),
                "second" => current.sub_seconds(qty),
                _ => current,
            };
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "startOfDay" => {
            current = current.start_of_day();
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "endOfDay" => {
            current = current.end_of_day();
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "startOfMonth" | "firstOfMonth" => {
            current = current.start_of_month();
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "endOfMonth" | "lastOfMonth" => {
            current = current.end_of_month();
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "startOfYear" => {
            current = current.start_of_year();
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "endOfYear" | "lastOfYear" => {
            current = current.end_of_year();
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "startOfWeek" => {
            current = current.start_of_week();
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "endOfWeek" => {
            current = current.end_of_week();
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "startOfQuarter" => {
            current = current.start_of_quarter();
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "endOfQuarter" => {
            current = current.end_of_quarter();
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "startOfHour" => {
            current = current.start_of_hour();
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "endOfHour" => {
            current = current.end_of_hour();
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "startOfMinute" => {
            current = current.start_of_minute();
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "endOfMinute" => {
            current = current.end_of_minute();
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }

        // --- Setters ---
        "setYear" => {
            let y = args.first().and_then(|v| v.to_i64()).unwrap_or(0) as i32;
            current = current
                .set_year(y)
                .map_err(|e| RuntimeError::new(RuntimeErrorKind::Custom { message: e }, span))?;
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "setMonth" => {
            let m = args.first().and_then(|v| v.to_i64()).unwrap_or(1) as u32;
            current = current
                .set_month(m)
                .map_err(|e| RuntimeError::new(RuntimeErrorKind::Custom { message: e }, span))?;
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "setDay" => {
            let d = args.first().and_then(|v| v.to_i64()).unwrap_or(1) as u32;
            current = current
                .set_day(d)
                .map_err(|e| RuntimeError::new(RuntimeErrorKind::Custom { message: e }, span))?;
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "setHour" => {
            let h = args.first().and_then(|v| v.to_i64()).unwrap_or(0) as u32;
            current = current
                .set_hour(h)
                .map_err(|e| RuntimeError::new(RuntimeErrorKind::Custom { message: e }, span))?;
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "setMinute" => {
            let m = args.first().and_then(|v| v.to_i64()).unwrap_or(0) as u32;
            current = current
                .set_minute(m)
                .map_err(|e| RuntimeError::new(RuntimeErrorKind::Custom { message: e }, span))?;
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "setSecond" => {
            let s = args.first().and_then(|v| v.to_i64()).unwrap_or(0) as u32;
            current = current
                .set_second(s)
                .map_err(|e| RuntimeError::new(RuntimeErrorKind::Custom { message: e }, span))?;
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "setTime" => {
            let h = args.first().and_then(|v| v.to_i64()).unwrap_or(0) as u32;
            let m = args.get(1).and_then(|v| v.to_i64()).unwrap_or(0) as u32;
            let s = args.get(2).and_then(|v| v.to_i64()).unwrap_or(0) as u32;
            let micro = args.get(3).and_then(|v| v.to_i64()).unwrap_or(0) as u32;
            current = current
                .set_time(h, m, s, micro)
                .map_err(|e| RuntimeError::new(RuntimeErrorKind::Custom { message: e }, span))?;
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "setDate" => {
            let y = args.first().and_then(|v| v.to_i64()).unwrap_or(0) as i32;
            let m = args.get(1).and_then(|v| v.to_i64()).unwrap_or(1) as u32;
            let d = args.get(2).and_then(|v| v.to_i64()).unwrap_or(1) as u32;
            current = current
                .set_date(y, m, d)
                .map_err(|e| RuntimeError::new(RuntimeErrorKind::Custom { message: e }, span))?;
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "setTimezone" => {
            let tz = args.first().map(|v| v.to_php_string()).unwrap_or_else(|| "UTC".to_string());
            current = current.set_timezone(&tz);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "next" => {
            let dow = args.first().and_then(|v| v.to_i64()).map(|d| d as u32);
            current = current.next(dow);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }
        "previous" => {
            let dow = args.first().and_then(|v| v.to_i64()).map(|d| d as u32);
            current = current.previous(dow);
            current.sync_properties(&mut rc.borrow_mut());
            Ok(receiver.clone())
        }

        _ => Err(RuntimeError::new(
            RuntimeErrorKind::UndefinedMethod {
                class: "Carbon".to_string(),
                method: method.to_string(),
            },
            span,
        )),
    }
}

fn parse_arg_date(args: &[PhpValue], index: usize, span: Span) -> Result<CarbonDate, RuntimeError> {
    let arg = args.get(index).ok_or_else(|| {
        RuntimeError::new(
            RuntimeErrorKind::Custom {
                message: format!("Missing required date argument at index {index}"),
            },
            span,
        )
    })?;
    from_php_value(arg).map_err(|e| RuntimeError::new(RuntimeErrorKind::Custom { message: e }, span))
}

fn parse_arg_date_or_now(
    args: &[PhpValue],
    index: usize,
    tz_name: &str,
    span: Span,
) -> Result<CarbonDate, RuntimeError> {
    match args.get(index) {
        None => Ok(CarbonDate::now(Some(tz_name))),
        Some(val) if val.is_null() => Ok(CarbonDate::now(Some(tz_name))),
        Some(val) => from_php_value(val)
            .map_err(|e| RuntimeError::new(RuntimeErrorKind::Custom { message: e }, span)),
    }
}
