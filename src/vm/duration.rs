//! Duration — interval helper with the same behavior as Carbon's `CarbonInterval`.
//!
//! Static factories (`Duration::hours(2)`, `Duration::fromString("1y 2m")`, ISO `P1Y2M`),
//! fluent setters and `add*` / `sub*` modifiers, `cascade`, `total`, `forHumans`, `spec`,
//! `format`, arithmetic (`times`, `divide`), and comparisons.
//!
//! Month length follows Carbon's default cascade factors: 1 month = 4 weeks = 28 days,
//! 1 year = 12 months = 336 days. `forHumans` cascades a copy before it prints.

use crate::lexer::span::Span;
use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::facade::{get_arg_str, register_runtime_closure_macro, FacadeResult};
use crate::vm::value::{PhpValue, VmObject};

const MICROS_PER_SECOND: i64 = 1_000_000;
const SECONDS_PER_MINUTE: i64 = 60;
const MINUTES_PER_HOUR: i64 = 60;
const HOURS_PER_DAY: i64 = 24;
const DAYS_PER_WEEK: i64 = 7;
const WEEKS_PER_MONTH: i64 = 4;
const MONTHS_PER_YEAR: i64 = 12;

const DIFF_ABSOLUTE: i64 = 1;
const DIFF_RELATIVE_TO_NOW: i64 = 2;
const DIFF_RELATIVE_TO_OTHER: i64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unit {
    Year,
    Month,
    Week,
    Day,
    Hour,
    Minute,
    Second,
    Millisecond,
    Microsecond,
}

const UNIT_ORDER: [Unit; 9] = [
    Unit::Year,
    Unit::Month,
    Unit::Week,
    Unit::Day,
    Unit::Hour,
    Unit::Minute,
    Unit::Second,
    Unit::Millisecond,
    Unit::Microsecond,
];

/// A signed span of calendar and clock units, stored the way CarbonInterval stores them:
/// non-negative components plus an `invert` flag when the whole span is negative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurationInterval {
    pub years: i64,
    pub months: i64,
    pub weeks: i64,
    pub days: i64,
    pub hours: i64,
    pub minutes: i64,
    pub seconds: i64,
    pub microseconds: i64,
    pub invert: bool,
}

impl Default for DurationInterval {
    fn default() -> Self {
        Self {
            years: 0,
            months: 0,
            weeks: 0,
            days: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
            microseconds: 0,
            invert: false,
        }
    }
}

impl DurationInterval {
    pub fn signed_years(&self) -> i64 {
        self.signed(self.years)
    }
    pub fn signed_months(&self) -> i64 {
        self.signed(self.months)
    }
    pub fn signed_weeks(&self) -> i64 {
        self.signed(self.weeks)
    }
    pub fn signed_days(&self) -> i64 {
        self.signed(self.days)
    }
    pub fn signed_hours(&self) -> i64 {
        self.signed(self.hours)
    }
    pub fn signed_minutes(&self) -> i64 {
        self.signed(self.minutes)
    }
    pub fn signed_seconds(&self) -> i64 {
        self.signed(self.seconds)
    }
    pub fn signed_microseconds(&self) -> i64 {
        self.signed(self.microseconds)
    }

    fn signed(&self, value: i64) -> i64 {
        if self.invert {
            -value
        } else {
            value
        }
    }

    /// Carry overflow toward larger units using Carbon's factors, then store the sign in `invert`.
    pub fn cascade(&self) -> Self {
        let mut years = self.signed(self.years);
        let mut months = self.signed(self.months);
        let mut weeks = self.signed(self.weeks);
        let mut days = self.signed(self.days);
        let mut hours = self.signed(self.hours);
        let mut minutes = self.signed(self.minutes);
        let mut seconds = self.signed(self.seconds);
        let mut micros = self.signed(self.microseconds);

        let carry = |small: &mut i64, large: &mut i64, factor: i64| {
            let q = small.div_euclid(factor);
            *small = small.rem_euclid(factor);
            *large += q;
        };
        carry(&mut micros, &mut seconds, MICROS_PER_SECOND);
        carry(&mut seconds, &mut minutes, SECONDS_PER_MINUTE);
        carry(&mut minutes, &mut hours, MINUTES_PER_HOUR);
        carry(&mut hours, &mut days, HOURS_PER_DAY);
        carry(&mut days, &mut weeks, DAYS_PER_WEEK);
        carry(&mut weeks, &mut months, WEEKS_PER_MONTH);
        carry(&mut months, &mut years, MONTHS_PER_YEAR);

        let mut invert = false;
        if years < 0 {
            years = -years;
            months = -months;
            weeks = -weeks;
            days = -days;
            hours = -hours;
            minutes = -minutes;
            seconds = -seconds;
            micros = -micros;
            carry(&mut micros, &mut seconds, MICROS_PER_SECOND);
            carry(&mut seconds, &mut minutes, SECONDS_PER_MINUTE);
            carry(&mut minutes, &mut hours, MINUTES_PER_HOUR);
            carry(&mut hours, &mut days, HOURS_PER_DAY);
            carry(&mut days, &mut weeks, DAYS_PER_WEEK);
            carry(&mut weeks, &mut months, WEEKS_PER_MONTH);
            carry(&mut months, &mut years, MONTHS_PER_YEAR);
            invert = true;
        }

        if years == 0
            && months == 0
            && weeks == 0
            && days == 0
            && hours == 0
            && minutes == 0
            && seconds == 0
            && micros == 0
        {
            invert = false;
        }

        Self {
            years,
            months,
            weeks,
            days,
            hours,
            minutes,
            seconds,
            microseconds: micros,
            invert,
        }
    }

    /// Flip an all-negative component set into `invert` plus absolute components.
    fn absorb_sign(&mut self) {
        let comps = [
            self.years,
            self.months,
            self.weeks,
            self.days,
            self.hours,
            self.minutes,
            self.seconds,
            self.microseconds,
        ];
        if comps.iter().any(|c| *c > 0) {
            return;
        }
        if comps.iter().all(|c| *c == 0) {
            self.invert = false;
            return;
        }
        self.years = -self.years;
        self.months = -self.months;
        self.weeks = -self.weeks;
        self.days = -self.days;
        self.hours = -self.hours;
        self.minutes = -self.minutes;
        self.seconds = -self.seconds;
        self.microseconds = -self.microseconds;
        self.invert = !self.invert;
    }

    pub fn total_micros(&self) -> i128 {
        let days = self.part(self.years) * (MONTHS_PER_YEAR * WEEKS_PER_MONTH * DAYS_PER_WEEK) as i128
            + self.part(self.months) * (WEEKS_PER_MONTH * DAYS_PER_WEEK) as i128
            + self.part(self.weeks) * DAYS_PER_WEEK as i128
            + self.part(self.days);
        let seconds = days * (HOURS_PER_DAY * MINUTES_PER_HOUR * SECONDS_PER_MINUTE) as i128
            + self.part(self.hours) * (MINUTES_PER_HOUR * SECONDS_PER_MINUTE) as i128
            + self.part(self.minutes) * SECONDS_PER_MINUTE as i128
            + self.part(self.seconds);
        seconds * MICROS_PER_SECOND as i128 + self.part(self.microseconds)
    }

    fn part(&self, value: i64) -> i128 {
        let n = value as i128;
        if self.invert {
            -n
        } else {
            n
        }
    }

    pub(crate) fn total(&self, unit: Unit) -> f64 {
        let micros = self.total_micros() as f64;
        let per = match unit {
            Unit::Year => {
                (MONTHS_PER_YEAR * WEEKS_PER_MONTH * DAYS_PER_WEEK * HOURS_PER_DAY * MINUTES_PER_HOUR * SECONDS_PER_MINUTE
                    * MICROS_PER_SECOND) as f64
            }
            Unit::Month => {
                (WEEKS_PER_MONTH * DAYS_PER_WEEK * HOURS_PER_DAY * MINUTES_PER_HOUR * SECONDS_PER_MINUTE * MICROS_PER_SECOND)
                    as f64
            }
            Unit::Week => (DAYS_PER_WEEK * HOURS_PER_DAY * MINUTES_PER_HOUR * SECONDS_PER_MINUTE * MICROS_PER_SECOND) as f64,
            Unit::Day => (HOURS_PER_DAY * MINUTES_PER_HOUR * SECONDS_PER_MINUTE * MICROS_PER_SECOND) as f64,
            Unit::Hour => (MINUTES_PER_HOUR * SECONDS_PER_MINUTE * MICROS_PER_SECOND) as f64,
            Unit::Minute => (SECONDS_PER_MINUTE * MICROS_PER_SECOND) as f64,
            Unit::Second => MICROS_PER_SECOND as f64,
            Unit::Millisecond => 1000.0,
            Unit::Microsecond => 1.0,
        };
        micros / per
    }

    pub(crate) fn get_unit(&self, unit: Unit) -> i64 {
        match unit {
            Unit::Year => self.years,
            Unit::Month => self.months,
            Unit::Week => self.weeks,
            Unit::Day => self.days,
            Unit::Hour => self.hours,
            Unit::Minute => self.minutes,
            Unit::Second => self.seconds,
            Unit::Millisecond => self.microseconds.div_euclid(1000),
            Unit::Microsecond => self.microseconds,
        }
    }

    fn zero_unit(&mut self, unit: Unit) {
        match unit {
            Unit::Year => self.years = 0,
            Unit::Month => self.months = 0,
            Unit::Week => self.weeks = 0,
            Unit::Day => self.days = 0,
            Unit::Hour => self.hours = 0,
            Unit::Minute => self.minutes = 0,
            Unit::Second => self.seconds = 0,
            Unit::Millisecond | Unit::Microsecond => self.microseconds = 0,
        }
    }

    fn add_raw(&mut self, unit: Unit, amount: i64) {
        match unit {
            Unit::Year => self.years += amount,
            Unit::Month => self.months += amount,
            Unit::Week => self.weeks += amount,
            Unit::Day => self.days += amount,
            Unit::Hour => self.hours += amount,
            Unit::Minute => self.minutes += amount,
            Unit::Second => self.seconds += amount,
            Unit::Millisecond => self.microseconds += amount.saturating_mul(1000),
            Unit::Microsecond => self.microseconds += amount,
        }
    }

    /// Set or add `value` of `unit`. A fractional part spills into the next smaller unit.
    fn write_unit(&mut self, unit: Unit, value: f64, replace: bool) {
        if replace {
            self.zero_unit(unit);
        }
        if value == 0.0 {
            return;
        }
        let neg = value < 0.0;
        let mut rest = value.abs();
        let mut current = Some(unit);
        while let Some(u) = current {
            let factor = factor_to_next(u);
            let whole = if factor.is_some() {
                rest.floor()
            } else {
                rest.round()
            };
            let amount = whole as i64;
            self.add_raw(u, if neg { -amount } else { amount });
            match factor {
                Some(factor) => {
                    rest = (rest - whole) * factor as f64;
                    if rest < 1e-8 {
                        break;
                    }
                    current = next_smaller(u);
                }
                None => break,
            }
        }
    }

    pub(crate) fn add_unit(&self, unit: Unit, value: f64) -> Self {
        let mut out = self.as_signed_components();
        out.write_unit(unit, value, false);
        out.normalize_mixed()
    }

    pub(crate) fn set_unit(&self, unit: Unit, value: f64) -> Self {
        let mut out = self.clone();
        out.write_unit(unit, value, true);
        out.absorb_sign();
        out
    }

    fn as_signed_components(&self) -> Self {
        Self {
            years: self.signed(self.years),
            months: self.signed(self.months),
            weeks: self.signed(self.weeks),
            days: self.signed(self.days),
            hours: self.signed(self.hours),
            minutes: self.signed(self.minutes),
            seconds: self.signed(self.seconds),
            microseconds: self.signed(self.microseconds),
            invert: false,
        }
    }

    pub fn add_interval(&self, other: &Self) -> Self {
        let a = self.as_signed_components();
        let b = other.as_signed_components();
        let out = Self {
            years: a.years + b.years,
            months: a.months + b.months,
            weeks: a.weeks + b.weeks,
            days: a.days + b.days,
            hours: a.hours + b.hours,
            minutes: a.minutes + b.minutes,
            seconds: a.seconds + b.seconds,
            microseconds: a.microseconds + b.microseconds,
            invert: false,
        };
        out.normalize_mixed()
    }

    /// Keep positive overflow as stored. Borrow only when a component went negative.
    fn normalize_mixed(mut self) -> Self {
        self.absorb_sign();
        if self.years < 0
            || self.months < 0
            || self.weeks < 0
            || self.days < 0
            || self.hours < 0
            || self.minutes < 0
            || self.seconds < 0
            || self.microseconds < 0
        {
            self.cascade()
        } else {
            self
        }
    }

    pub fn sub_interval(&self, other: &Self) -> Self {
        let mut flipped = other.clone();
        flipped.invert = !other.invert;
        self.add_interval(&flipped)
    }

    pub fn scale(&self, factor: f64) -> Self {
        let signed_factor = if self.invert { -factor } else { factor };
        let mut out = Self::default();
        out.write_unit(Unit::Year, self.years as f64 * signed_factor, false);
        out.write_unit(Unit::Month, self.months as f64 * signed_factor, false);
        out.write_unit(Unit::Week, self.weeks as f64 * signed_factor, false);
        out.write_unit(Unit::Day, self.days as f64 * signed_factor, false);
        out.write_unit(Unit::Hour, self.hours as f64 * signed_factor, false);
        out.write_unit(Unit::Minute, self.minutes as f64 * signed_factor, false);
        out.write_unit(Unit::Second, self.seconds as f64 * signed_factor, false);
        out.write_unit(Unit::Microsecond, self.microseconds as f64 * signed_factor, false);
        out.absorb_sign();
        out.cascade()
    }

    pub fn invert_interval(&self) -> Self {
        let mut out = self.clone();
        if self.total_micros() != 0 {
            out.invert = !self.invert;
        }
        out
    }

    pub fn abs_interval(&self) -> Self {
        let mut out = self.clone();
        out.invert = false;
        if out.years < 0
            || out.months < 0
            || out.weeks < 0
            || out.days < 0
            || out.hours < 0
            || out.minutes < 0
            || out.seconds < 0
            || out.microseconds < 0
        {
            out = out.cascade();
            out.invert = false;
        }
        out
    }

    pub fn spec(&self) -> String {
        let days = self.days + self.weeks * DAYS_PER_WEEK;
        let mut date = String::new();
        if self.years != 0 {
            date.push_str(&format!("{}Y", self.years.abs()));
        }
        if self.months != 0 {
            date.push_str(&format!("{}M", self.months.abs()));
        }
        if days != 0 {
            date.push_str(&format!("{}D", days.abs()));
        }
        let mut time = String::new();
        if self.hours != 0 {
            time.push_str(&format!("{}H", self.hours.abs()));
        }
        if self.minutes != 0 {
            time.push_str(&format!("{}M", self.minutes.abs()));
        }
        if self.seconds != 0 || self.microseconds != 0 {
            let extra = self.microseconds.abs().div_euclid(MICROS_PER_SECOND);
            let frac = self.microseconds.abs().rem_euclid(MICROS_PER_SECOND);
            let secs = self.seconds.abs() + extra;
            if frac == 0 {
                time.push_str(&format!("{secs}S"));
            } else {
                time.push_str(&format!("{secs}.{frac:06}S"));
            }
        }
        if date.is_empty() && time.is_empty() {
            return "PT0S".to_string();
        }
        let mut out = String::new();
        if self.invert {
            out.push('-');
        }
        out.push('P');
        out.push_str(&date);
        if !time.is_empty() {
            out.push('T');
            out.push_str(&time);
        }
        out
    }

    pub fn format_interval(&self, fmt: &str) -> String {
        let days = self.days + self.weeks * DAYS_PER_WEEK;
        let total_days = (self.total(Unit::Day) as i64).abs();
        let sign = if self.invert { "-" } else { "+" };
        let mut out = String::new();
        let chars: Vec<char> = fmt.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '%' && i + 1 < chars.len() {
                let spec = chars[i + 1];
                match spec {
                    'Y' => out.push_str(&pad2(self.years)),
                    'y' => out.push_str(&self.years.abs().to_string()),
                    'M' => out.push_str(&pad2(self.months)),
                    'm' => out.push_str(&self.months.abs().to_string()),
                    'D' => out.push_str(&pad2(days)),
                    'd' => out.push_str(&days.abs().to_string()),
                    'H' => out.push_str(&pad2(self.hours)),
                    'h' => out.push_str(&self.hours.abs().to_string()),
                    'I' => out.push_str(&pad2(self.minutes)),
                    'i' => out.push_str(&self.minutes.abs().to_string()),
                    'S' => out.push_str(&pad2(self.seconds)),
                    's' => out.push_str(&self.seconds.abs().to_string()),
                    'F' | 'f' => {
                        let frac = self.microseconds.abs().rem_euclid(MICROS_PER_SECOND);
                        out.push_str(&format!("{frac:06}"));
                    }
                    'R' => out.push_str(sign),
                    'r' => {
                        if self.invert {
                            out.push('-');
                        }
                    }
                    'a' => out.push_str(&total_days.to_string()),
                    '%' => out.push('%'),
                    other => out.push(other),
                }
                i += 2;
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out
    }

    pub(crate) fn for_humans(&self, opts: &HumanOpts) -> String {
        let mut d = self.cascade();
        fold_skipped(&mut d, &opts.skip);
        if let Some(min) = opts.minimum {
            zero_below(&mut d, min);
        }
        let mut parts: Vec<String> = Vec::new();
        let millis = d.microseconds.div_euclid(1000);
        let micros = d.microseconds.rem_euclid(1000);
        let values = [
            (Unit::Year, d.years),
            (Unit::Month, d.months),
            (Unit::Week, d.weeks),
            (Unit::Day, d.days),
            (Unit::Hour, d.hours),
            (Unit::Minute, d.minutes),
            (Unit::Second, d.seconds),
            (Unit::Millisecond, millis),
            (Unit::Microsecond, micros),
        ];
        for (unit, count) in values {
            if count == 0 || opts.skip.contains(&unit) {
                continue;
            }
            if let Some(min) = opts.minimum {
                if unit_rank(unit) > unit_rank(min) {
                    continue;
                }
            }
            parts.push(human_part(unit, count, opts.short, opts.a_unit));
        }
        if parts.is_empty() {
            let unit = opts.minimum.unwrap_or(Unit::Second);
            parts.push(human_part(unit, 0, opts.short, false));
        }
        if opts.parts > 0 && parts.len() > opts.parts as usize {
            parts.truncate(opts.parts as usize);
        }
        let body = match &opts.join {
            Join::Space => parts.join(" "),
            Join::Natural => join_natural(&parts),
            Join::Sep(sep) => parts.join(sep),
        };
        match opts.syntax {
            DIFF_RELATIVE_TO_NOW => {
                if self.invert {
                    format!("{body} ago")
                } else {
                    format!("{body} from now")
                }
            }
            DIFF_RELATIVE_TO_OTHER => {
                if self.invert {
                    format!("{body} before")
                } else {
                    format!("{body} after")
                }
            }
            _ => body,
        }
    }

    pub fn to_vm_object(&self) -> VmObject {
        let mut obj = VmObject::new("Duration");
        self.sync_properties(&mut obj);
        obj
    }

    pub fn sync_properties(&self, obj: &mut VmObject) {
        let humans = self.for_humans(&HumanOpts::default());
        let spec = self.spec();
        obj.set_property("formatted", PhpValue::string(&humans));
        obj.set_property("iso", PhpValue::string(&spec));
        obj.set_property("spec", PhpValue::string(&spec));
        obj.set_property("years", PhpValue::Int(self.years));
        obj.set_property("months", PhpValue::Int(self.months));
        obj.set_property("weeks", PhpValue::Int(self.weeks));
        obj.set_property("days", PhpValue::Int(self.days));
        obj.set_property("hours", PhpValue::Int(self.hours));
        obj.set_property("minutes", PhpValue::Int(self.minutes));
        obj.set_property("seconds", PhpValue::Int(self.seconds));
        obj.set_property("microseconds", PhpValue::Int(self.microseconds));
        obj.set_property("milliseconds", PhpValue::Int(self.microseconds.div_euclid(1000)));
        obj.set_property("y", PhpValue::Int(self.years));
        obj.set_property("m", PhpValue::Int(self.months));
        obj.set_property("d", PhpValue::Int(self.days + self.weeks * DAYS_PER_WEEK));
        obj.set_property("h", PhpValue::Int(self.hours));
        obj.set_property("i", PhpValue::Int(self.minutes));
        obj.set_property("s", PhpValue::Int(self.seconds));
        obj.set_property(
            "f",
            PhpValue::Float(self.microseconds as f64 / MICROS_PER_SECOND as f64),
        );
        obj.set_property("invert", PhpValue::Bool(self.invert));
    }
}

fn pad2(n: i64) -> String {
    format!("{:02}", n.abs())
}

fn factor_to_next(unit: Unit) -> Option<i64> {
    match unit {
        Unit::Year => Some(MONTHS_PER_YEAR),
        Unit::Month => Some(WEEKS_PER_MONTH),
        Unit::Week => Some(DAYS_PER_WEEK),
        Unit::Day => Some(HOURS_PER_DAY),
        Unit::Hour => Some(MINUTES_PER_HOUR),
        Unit::Minute => Some(SECONDS_PER_MINUTE),
        Unit::Second => Some(MICROS_PER_SECOND),
        Unit::Millisecond => Some(1000),
        Unit::Microsecond => None,
    }
}

fn next_smaller(unit: Unit) -> Option<Unit> {
    match unit {
        Unit::Year => Some(Unit::Month),
        Unit::Month => Some(Unit::Week),
        Unit::Week => Some(Unit::Day),
        Unit::Day => Some(Unit::Hour),
        Unit::Hour => Some(Unit::Minute),
        Unit::Minute => Some(Unit::Second),
        Unit::Second => Some(Unit::Microsecond),
        Unit::Millisecond => Some(Unit::Microsecond),
        Unit::Microsecond => None,
    }
}

fn unit_rank(unit: Unit) -> usize {
    UNIT_ORDER.iter().position(|u| *u == unit).unwrap_or(0)
}

fn fold_skipped(d: &mut DurationInterval, skip: &[Unit]) {
    if skip.is_empty() {
        return;
    }
    if skip.contains(&Unit::Year) {
        d.months += d.years * MONTHS_PER_YEAR;
        d.years = 0;
    }
    if skip.contains(&Unit::Month) {
        d.weeks += d.months * WEEKS_PER_MONTH;
        d.months = 0;
    }
    if skip.contains(&Unit::Week) {
        d.days += d.weeks * DAYS_PER_WEEK;
        d.weeks = 0;
    }
    if skip.contains(&Unit::Day) {
        d.hours += d.days * HOURS_PER_DAY;
        d.days = 0;
    }
    if skip.contains(&Unit::Hour) {
        d.minutes += d.hours * MINUTES_PER_HOUR;
        d.hours = 0;
    }
    if skip.contains(&Unit::Minute) {
        d.seconds += d.minutes * SECONDS_PER_MINUTE;
        d.minutes = 0;
    }
    if skip.contains(&Unit::Second) {
        d.microseconds += d.seconds * MICROS_PER_SECOND;
        d.seconds = 0;
    }
}

fn zero_below(d: &mut DurationInterval, min: Unit) {
    let drop_from = unit_rank(min) + 1;
    if drop_from <= unit_rank(Unit::Month) {
        d.months = 0;
    }
    if drop_from <= unit_rank(Unit::Week) {
        d.weeks = 0;
    }
    if drop_from <= unit_rank(Unit::Day) {
        d.days = 0;
    }
    if drop_from <= unit_rank(Unit::Hour) {
        d.hours = 0;
    }
    if drop_from <= unit_rank(Unit::Minute) {
        d.minutes = 0;
    }
    if drop_from <= unit_rank(Unit::Second) {
        d.seconds = 0;
    }
    if drop_from <= unit_rank(Unit::Millisecond) {
        d.microseconds = 0;
    } else if drop_from <= unit_rank(Unit::Microsecond) {
        d.microseconds = d.microseconds.div_euclid(1000) * 1000;
    }
}

fn human_part(unit: Unit, count: i64, short: bool, a_unit: bool) -> String {
    let n = count.abs();
    if short {
        return match unit {
            Unit::Year => {
                if n == 1 {
                    format!("{n}yr")
                } else {
                    format!("{n}yrs")
                }
            }
            Unit::Month => {
                if n == 1 {
                    format!("{n}mo")
                } else {
                    format!("{n}mos")
                }
            }
            Unit::Week => format!("{n}w"),
            Unit::Day => format!("{n}d"),
            Unit::Hour => format!("{n}h"),
            Unit::Minute => format!("{n}m"),
            Unit::Second => format!("{n}s"),
            Unit::Millisecond => format!("{n}ms"),
            Unit::Microsecond => format!("{n}μs"),
        };
    }
    let name = unit_word(unit, n != 1);
    if a_unit && n == 1 {
        let article = if unit == Unit::Hour { "an" } else { "a" };
        format!("{article} {name}")
    } else {
        format!("{n} {name}")
    }
}

fn unit_word(unit: Unit, plural: bool) -> &'static str {
    match (unit, plural) {
        (Unit::Year, false) => "year",
        (Unit::Year, true) => "years",
        (Unit::Month, false) => "month",
        (Unit::Month, true) => "months",
        (Unit::Week, false) => "week",
        (Unit::Week, true) => "weeks",
        (Unit::Day, false) => "day",
        (Unit::Day, true) => "days",
        (Unit::Hour, false) => "hour",
        (Unit::Hour, true) => "hours",
        (Unit::Minute, false) => "minute",
        (Unit::Minute, true) => "minutes",
        (Unit::Second, false) => "second",
        (Unit::Second, true) => "seconds",
        (Unit::Millisecond, false) => "millisecond",
        (Unit::Millisecond, true) => "milliseconds",
        (Unit::Microsecond, false) => "microsecond",
        (Unit::Microsecond, true) => "microseconds",
    }
}

fn join_natural(parts: &[String]) -> String {
    match parts.len() {
        0 => String::new(),
        1 => parts[0].clone(),
        2 => format!("{} and {}", parts[0], parts[1]),
        n => {
            let head = parts[..n - 1].join(", ");
            format!("{head} and {}", parts[n - 1])
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Join {
    Space,
    Natural,
    Sep(String),
}

#[derive(Debug, Clone)]
pub(crate) struct HumanOpts {
    short: bool,
    parts: i64,
    syntax: i64,
    a_unit: bool,
    join: Join,
    minimum: Option<Unit>,
    skip: Vec<Unit>,
}

impl Default for HumanOpts {
    fn default() -> Self {
        Self {
            short: false,
            parts: -1,
            syntax: DIFF_ABSOLUTE,
            a_unit: false,
            join: Join::Space,
            minimum: None,
            skip: Vec::new(),
        }
    }
}

fn parse_unit_name(raw: &str) -> Option<Unit> {
    let u = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    match u.as_str() {
        "y" | "yr" | "yrs" | "year" | "years" => Some(Unit::Year),
        "mo" | "mon" | "mons" | "month" | "months" => Some(Unit::Month),
        "w" | "wk" | "wks" | "week" | "weeks" => Some(Unit::Week),
        "d" | "day" | "days" | "dayz" => Some(Unit::Day),
        "h" | "hr" | "hrs" | "hour" | "hours" => Some(Unit::Hour),
        "i" | "min" | "mins" | "minute" | "minutes" | "m" => Some(Unit::Minute),
        "s" | "sec" | "secs" | "second" | "seconds" => Some(Unit::Second),
        "ms" | "milli" | "millis" | "millisecond" | "milliseconds" => Some(Unit::Millisecond),
        "us" | "µs" | "μs" | "micro" | "micros" | "microsecond" | "microseconds" => Some(Unit::Microsecond),
        _ => None,
    }
}

fn unit_from_method_tail(tail: &str) -> Option<Unit> {
    match tail {
        "year" | "years" | "Year" | "Years" => Some(Unit::Year),
        "month" | "months" | "Month" | "Months" => Some(Unit::Month),
        "week" | "weeks" | "Week" | "Weeks" => Some(Unit::Week),
        "day" | "days" | "dayz" | "Day" | "Days" | "Dayz" => Some(Unit::Day),
        "hour" | "hours" | "Hour" | "Hours" => Some(Unit::Hour),
        "minute" | "minutes" | "Minute" | "Minutes" => Some(Unit::Minute),
        "second" | "seconds" | "Second" | "Seconds" => Some(Unit::Second),
        "millisecond" | "milliseconds" | "Millisecond" | "Milliseconds" => Some(Unit::Millisecond),
        "microsecond" | "microseconds" | "Microsecond" | "Microseconds" => Some(Unit::Microsecond),
        _ => None,
    }
}

enum MethodKind {
    GetSet(Unit),
    Add(Unit),
    Sub(Unit),
    Total(Unit),
}

fn classify_method(method: &str) -> Option<MethodKind> {
    if let Some(rest) = method.strip_prefix("total") {
        return unit_from_method_tail(rest).map(MethodKind::Total);
    }
    if let Some(rest) = method.strip_prefix("add") {
        return unit_from_method_tail(rest).map(MethodKind::Add);
    }
    if let Some(rest) = method.strip_prefix("sub") {
        return unit_from_method_tail(rest).map(MethodKind::Sub);
    }
    unit_from_method_tail(method).map(MethodKind::GetSet)
}

fn arg_f64(args: &[PhpValue], index: usize, default: f64) -> f64 {
    match args.get(index) {
        None | Some(PhpValue::Null) => default,
        Some(v) => v.to_f64().unwrap_or(default),
    }
}

fn factory(unit: Unit, args: &[PhpValue]) -> DurationInterval {
    let mut d = DurationInterval::default();
    d.write_unit(unit, arg_f64(args, 0, 1.0), true);
    d.absorb_sign();
    d
}

pub fn from_string(input: &str) -> Result<DurationInterval, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("Duration::fromString() expects a non-empty interval string".to_string());
    }
    if raw.starts_with('P') || raw.starts_with('p') || raw.starts_with("-P") || raw.starts_with("-p") {
        return parse_iso(raw);
    }
    parse_words(raw)
}

fn parse_iso(raw: &str) -> Result<DurationInterval, String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"(?i)^(?P<sign>-)?P(?:(?P<y>\d+(?:\.\d+)?)Y)?(?:(?P<mo>\d+(?:\.\d+)?)M)?(?:(?P<w>\d+(?:\.\d+)?)W)?(?:(?P<d>\d+(?:\.\d+)?)D)?(?:T(?:(?P<h>\d+(?:\.\d+)?)H)?(?:(?P<mi>\d+(?:\.\d+)?)M)?(?:(?P<s>\d+(?:\.\d+)?)S)?)?$",
        )
        .unwrap()
    });
    let caps = re
        .captures(raw)
        .ok_or_else(|| format!("Invalid ISO duration '{raw}'"))?;
    let mut d = DurationInterval::default();
    let num = |name: &str| -> f64 {
        caps.name(name)
            .and_then(|m| m.as_str().parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    d.write_unit(Unit::Year, num("y"), false);
    d.write_unit(Unit::Month, num("mo"), false);
    d.write_unit(Unit::Week, num("w"), false);
    d.write_unit(Unit::Day, num("d"), false);
    d.write_unit(Unit::Hour, num("h"), false);
    d.write_unit(Unit::Minute, num("mi"), false);
    d.write_unit(Unit::Second, num("s"), false);
    if caps.name("sign").is_some() {
        d.invert = !d.invert;
        if d.total_micros() == 0 {
            d.invert = false;
        }
    }
    d.absorb_sign();
    Ok(d)
}

fn parse_words(raw: &str) -> Result<DurationInterval, String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"(?i)(?P<sign>-)?(?P<num>\d+(?:\.\d+)?)\s*(?P<unit>[a-zµμ]+)").unwrap()
    });
    let mut d = DurationInterval::default();
    let mut matched = false;
    for caps in re.captures_iter(raw) {
        matched = true;
        let num: f64 = caps["num"].parse().unwrap_or(0.0);
        let signed = if caps.name("sign").is_some() { -num } else { num };
        let unit = parse_unit_name(&caps["unit"])
            .ok_or_else(|| format!("Unknown duration unit '{}'", &caps["unit"]))?;
        d.write_unit(unit, signed, false);
    }
    if !matched {
        return Err(format!("Invalid duration string '{raw}'"));
    }
    d.absorb_sign();
    Ok(d)
}

pub fn make_interval(args: &[PhpValue]) -> Result<PhpValue, String> {
    if args.is_empty() || args[0].is_null() {
        return Ok(PhpValue::Null);
    }
    if let Some(existing) = interval_from_value(&args[0]) {
        return Ok(PhpValue::object(existing.to_vm_object()));
    }
    match &args[0] {
        PhpValue::String(s) => {
            let d = from_string(s)?;
            Ok(PhpValue::object(d.to_vm_object()))
        }
        other => {
            let qty = other.to_f64().ok_or_else(|| {
                "Duration::make() expects a string, a number, or a Duration".to_string()
            })?;
            let unit = if args.len() > 1 {
                parse_unit_name(&args[1].to_php_string())
                    .ok_or_else(|| format!("Unknown duration unit '{}'", args[1].to_php_string()))?
            } else {
                Unit::Second
            };
            let mut d = DurationInterval::default();
            d.write_unit(unit, qty, true);
            d.absorb_sign();
            Ok(PhpValue::object(d.to_vm_object()))
        }
    }
}

pub fn interval_from_value(val: &PhpValue) -> Option<DurationInterval> {
    let PhpValue::Object(rc) = val else {
        return None;
    };
    let obj = rc.borrow();
    if obj.class_name != "Duration" && obj.class_name != "CarbonInterval" {
        return None;
    }
    let read = |name: &str| -> i64 {
        obj.get_property(name).and_then(|v| v.to_i64()).unwrap_or(0)
    };
    Some(DurationInterval {
        years: read("years"),
        months: read("months"),
        weeks: read("weeks"),
        days: read("days"),
        hours: read("hours"),
        minutes: read("minutes"),
        seconds: read("seconds"),
        microseconds: read("microseconds"),
        invert: obj
            .get_property("invert")
            .map(|v| v.is_truthy())
            .unwrap_or(false),
    })
}

fn human_opts_from_args(args: &[PhpValue]) -> HumanOpts {
    if args.is_empty() {
        return HumanOpts::default();
    }
    if let Some(map) = args[0].as_dict() {
        return human_opts_from_map(&*map);
    }
    let mut opts = HumanOpts::default();
    if let Some(n) = args.first().and_then(|v| v.to_i64()) {
        opts.syntax = n;
    }
    if args.len() > 1 {
        opts.short = args[1].is_truthy();
    }
    if args.len() > 2 {
        if let Some(n) = args[2].to_i64() {
            opts.parts = n;
        }
    }
    if args.len() > 3 {
        if let Some(map) = args[3].as_dict() {
            let extra = human_opts_from_map(&*map);
            opts.short = extra.short || opts.short;
            if extra.parts > 0 {
                opts.parts = extra.parts;
            }
            opts.a_unit = extra.a_unit;
            opts.join = extra.join;
            opts.minimum = extra.minimum.or(opts.minimum);
            if !extra.skip.is_empty() {
                opts.skip = extra.skip;
            }
            if extra.syntax != DIFF_ABSOLUTE {
                opts.syntax = extra.syntax;
            }
        }
    }
    opts
}

fn human_opts_from_map(map: &indexmap::IndexMap<String, PhpValue>) -> HumanOpts {
    let mut opts = HumanOpts::default();
    if let Some(v) = map.get("short") {
        opts.short = v.is_truthy();
    }
    if let Some(v) = map.get("parts").and_then(|v| v.to_i64()) {
        opts.parts = v;
    }
    if let Some(v) = map.get("syntax").and_then(|v| v.to_i64()) {
        opts.syntax = v;
    }
    if let Some(v) = map.get("aUnit").or_else(|| map.get("a_unit")) {
        opts.a_unit = v.is_truthy();
    }
    if let Some(v) = map.get("join") {
        opts.join = match v {
            PhpValue::Bool(true) => Join::Natural,
            PhpValue::String(s) => Join::Sep(s.to_string()),
            _ => Join::Space,
        };
    }
    if let Some(v) = map.get("minimumUnit").or_else(|| map.get("minimum_unit")) {
        opts.minimum = parse_unit_name(&v.to_php_string());
    }
    if let Some(v) = map.get("skip") {
        opts.skip = strings_of(v)
            .into_iter()
            .filter_map(|s| parse_unit_name(&s))
            .collect();
    }
    opts
}

fn strings_of(v: &PhpValue) -> Vec<String> {
    match v {
        PhpValue::Array(a) | PhpValue::List(a) => a.borrow().iter().map(|x| x.to_php_string()).collect(),
        PhpValue::Dict(d) => d.borrow().values().map(|x| x.to_php_string()).collect(),
        PhpValue::String(s) => vec![s.to_string()],
        _ => vec![v.to_php_string()],
    }
}

pub fn get_duration_constant(name: &str) -> Option<PhpValue> {
    match name {
        "DAYS_PER_WEEK" => Some(PhpValue::Int(DAYS_PER_WEEK)),
        "HOURS_PER_DAY" => Some(PhpValue::Int(HOURS_PER_DAY)),
        "MINUTES_PER_HOUR" => Some(PhpValue::Int(MINUTES_PER_HOUR)),
        "SECONDS_PER_MINUTE" => Some(PhpValue::Int(SECONDS_PER_MINUTE)),
        "WEEKS_PER_MONTH" => Some(PhpValue::Int(WEEKS_PER_MONTH)),
        "MONTHS_PER_YEAR" => Some(PhpValue::Int(MONTHS_PER_YEAR)),
        "MICROSECONDS_PER_SECOND" => Some(PhpValue::Int(MICROS_PER_SECOND)),
        "MILLISECONDS_PER_SECOND" => Some(PhpValue::Int(1000)),
        "DIFF_ABSOLUTE" => Some(PhpValue::Int(DIFF_ABSOLUTE)),
        "DIFF_RELATIVE_TO_NOW" => Some(PhpValue::Int(DIFF_RELATIVE_TO_NOW)),
        "DIFF_RELATIVE_TO_OTHER" => Some(PhpValue::Int(DIFF_RELATIVE_TO_OTHER)),
        "PERIOD_PREFIX" => Some(PhpValue::string("P")),
        "PERIOD_TIME_PREFIX" => Some(PhpValue::string("T")),
        _ => None,
    }
}

fn is_factory(method: &str) -> bool {
    matches!(
        method,
        "year"
            | "years"
            | "month"
            | "months"
            | "week"
            | "weeks"
            | "day"
            | "days"
            | "dayz"
            | "hour"
            | "hours"
            | "minute"
            | "minutes"
            | "second"
            | "seconds"
            | "millisecond"
            | "milliseconds"
            | "microsecond"
            | "microseconds"
    )
}

pub fn dispatch_duration(method: &str, args: &[PhpValue]) -> FacadeResult {
    if method == "macro" {
        let name = get_arg_str(args, 0, "Duration::macro")?;
        let closure = match args.get(1) {
            Some(PhpValue::Closure(c)) => c,
            _ => {
                return Err(RuntimeErrorKind::Custom {
                    message: format!(
                        "TypeError: Duration::macro() expects argument 2 to be a Closure, got {}",
                        args.get(1).map(|v| v.type_name()).unwrap_or("none")
                    ),
                });
            }
        };
        register_runtime_closure_macro("Duration", &name, (**closure).clone());
        register_runtime_closure_macro("CarbonInterval", &name, (**closure).clone());
        return Ok(PhpValue::Null);
    }

    if method == "create" {
        let mut d = DurationInterval::default();
        let units = [
            Unit::Year,
            Unit::Month,
            Unit::Week,
            Unit::Day,
            Unit::Hour,
            Unit::Minute,
            Unit::Second,
            Unit::Microsecond,
        ];
        for (i, unit) in units.into_iter().enumerate() {
            if i < args.len() {
                d.write_unit(unit, arg_f64(args, i, 0.0), false);
            }
        }
        d.absorb_sign();
        return Ok(PhpValue::object(d.to_vm_object()));
    }

    if matches!(method, "make" | "instance" | "fromString" | "parse" | "createFromFormat") {
        let parse_args: Vec<PhpValue> = if method == "createFromFormat" && args.len() >= 2 {
            vec![args[1].clone()]
        } else {
            args.to_vec()
        };
        return make_interval(&parse_args).map_err(|e| RuntimeErrorKind::Custom { message: e });
    }

    if is_factory(method) {
        let unit = unit_from_method_tail(method).unwrap();
        let d = factory(unit, args);
        return Ok(PhpValue::object(d.to_vm_object()));
    }

    let zero = DurationInterval::default();
    let receiver = PhpValue::object(zero.to_vm_object());
    exec_duration_instance_method(&receiver, method, args, Span::new(0, 0, 0, 1, 1)).map_err(|e| e.kind)
}

pub fn dispatch_carbon_interval(method: &str, args: &[PhpValue]) -> FacadeResult {
    dispatch_duration(method, args)
}

pub fn exec_duration_instance_method(
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
    let mut current = interval_from_value(receiver).ok_or_else(|| {
        RuntimeError::new(
            RuntimeErrorKind::Custom {
                message: "Expected an instance of Duration".to_string(),
            },
            span,
        )
    })?;

    let finish = |current: DurationInterval| {
        current.sync_properties(&mut rc.borrow_mut());
        Ok(receiver.clone())
    };

    if let Some(kind) = classify_method(method) {
        return match kind {
            MethodKind::GetSet(unit) => {
                if args.is_empty() {
                    Ok(PhpValue::Int(current.get_unit(unit)))
                } else {
                    current = current.set_unit(unit, arg_f64(args, 0, 0.0));
                    finish(current)
                }
            }
            MethodKind::Add(unit) => {
                current = current.add_unit(unit, arg_f64(args, 0, 1.0));
                finish(current)
            }
            MethodKind::Sub(unit) => {
                current = current.add_unit(unit, -arg_f64(args, 0, 1.0));
                finish(current)
            }
            MethodKind::Total(unit) => Ok(PhpValue::Float(current.total(unit))),
        };
    }

    match method {
        "cascade" => finish(current.cascade()),
        "spec" | "toIso8601String" | "toDateInterval" | "toString" => Ok(PhpValue::string(current.spec())),
        "format" => {
            let fmt = args
                .first()
                .map(|v| v.to_php_string())
                .unwrap_or_else(|| "%y years %m months %d days %h hours %i minutes %s seconds".to_string());
            Ok(PhpValue::string(current.format_interval(&fmt)))
        }
        "forHumans" => {
            let opts = human_opts_from_args(args);
            Ok(PhpValue::string(current.for_humans(&opts)))
        }
        "total" => {
            let name = get_arg_str(args, 0, "Duration::total").map_err(|kind| RuntimeError::new(kind, span))?;
            let unit = parse_unit_name(&name).ok_or_else(|| {
                RuntimeError::new(
                    RuntimeErrorKind::Custom {
                        message: format!("Unknown duration unit '{name}'"),
                    },
                    span,
                )
            })?;
            Ok(PhpValue::Float(current.total(unit)))
        }
        "add" => {
            current = apply_add(&current, args, false).map_err(|e| RuntimeError::new(RuntimeErrorKind::Custom { message: e }, span))?;
            finish(current)
        }
        "sub" => {
            current = apply_add(&current, args, true).map_err(|e| RuntimeError::new(RuntimeErrorKind::Custom { message: e }, span))?;
            finish(current)
        }
        "invert" => finish(current.invert_interval()),
        "abs" => finish(current.abs_interval()),
        "times" | "multiply" => {
            let factor = arg_f64(args, 0, 1.0);
            finish(current.scale(factor))
        }
        "shares" | "divide" => {
            let divisor = arg_f64(args, 0, 1.0);
            if divisor == 0.0 {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::Custom {
                        message: "Duration::divide() division by zero".to_string(),
                    },
                    span,
                ));
            }
            finish(current.scale(1.0 / divisor))
        }
        "copy" | "clone" => Ok(PhpValue::object(current.to_vm_object())),
        "eq" | "equalTo" => {
            let other = expect_interval(args, 0, span)?;
            Ok(PhpValue::Bool(current.total_micros() == other.total_micros()))
        }
        "ne" | "notEqualTo" => {
            let other = expect_interval(args, 0, span)?;
            Ok(PhpValue::Bool(current.total_micros() != other.total_micros()))
        }
        "gt" | "greaterThan" => {
            let other = expect_interval(args, 0, span)?;
            Ok(PhpValue::Bool(current.total_micros() > other.total_micros()))
        }
        "gte" | "greaterThanOrEqualTo" => {
            let other = expect_interval(args, 0, span)?;
            Ok(PhpValue::Bool(current.total_micros() >= other.total_micros()))
        }
        "lt" | "lessThan" => {
            let other = expect_interval(args, 0, span)?;
            Ok(PhpValue::Bool(current.total_micros() < other.total_micros()))
        }
        "lte" | "lessThanOrEqualTo" => {
            let other = expect_interval(args, 0, span)?;
            Ok(PhpValue::Bool(current.total_micros() <= other.total_micros()))
        }
        "compare" => {
            let other = expect_interval(args, 0, span)?;
            let ord = current.total_micros().cmp(&other.total_micros());
            Ok(PhpValue::Int(match ord {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }))
        }
        "between" | "betweenIncluded" => {
            let a = expect_interval(args, 0, span)?;
            let b = expect_interval(args, 1, span)?;
            Ok(PhpValue::Bool(between(&current, &a, &b, true)))
        }
        "betweenExcluded" => {
            let a = expect_interval(args, 0, span)?;
            let b = expect_interval(args, 1, span)?;
            Ok(PhpValue::Bool(between(&current, &a, &b, false)))
        }
        "isEmpty" => Ok(PhpValue::Bool(current.total_micros() == 0)),
        "isNegative" => Ok(PhpValue::Bool(current.total_micros() < 0)),
        "isPositive" => Ok(PhpValue::Bool(current.total_micros() > 0)),
        "get" => {
            let name = get_arg_str(args, 0, "Duration::get").map_err(|kind| RuntimeError::new(kind, span))?;
            let unit = parse_unit_name(&name).ok_or_else(|| {
                RuntimeError::new(
                    RuntimeErrorKind::Custom {
                        message: format!("Unknown duration unit '{name}'"),
                    },
                    span,
                )
            })?;
            Ok(PhpValue::Int(current.get_unit(unit)))
        }
        "set" => {
            let name = get_arg_str(args, 0, "Duration::set").map_err(|kind| RuntimeError::new(kind, span))?;
            let unit = parse_unit_name(&name).ok_or_else(|| {
                RuntimeError::new(
                    RuntimeErrorKind::Custom {
                        message: format!("Unknown duration unit '{name}'"),
                    },
                    span,
                )
            })?;
            current = current.set_unit(unit, arg_f64(args, 1, 0.0));
            finish(current)
        }
        "toArray" => {
            let mut pairs = Vec::new();
            pairs.push(("years".to_string(), PhpValue::Int(current.years)));
            pairs.push(("months".to_string(), PhpValue::Int(current.months)));
            pairs.push(("weeks".to_string(), PhpValue::Int(current.weeks)));
            pairs.push(("days".to_string(), PhpValue::Int(current.days)));
            pairs.push(("hours".to_string(), PhpValue::Int(current.hours)));
            pairs.push(("minutes".to_string(), PhpValue::Int(current.minutes)));
            pairs.push(("seconds".to_string(), PhpValue::Int(current.seconds)));
            pairs.push(("microseconds".to_string(), PhpValue::Int(current.microseconds)));
            pairs.push(("invert".to_string(), PhpValue::Bool(current.invert)));
            Ok(PhpValue::dict(pairs))
        }
        _ => Err(RuntimeError::new(
            RuntimeErrorKind::UndefinedMethod {
                class: "Duration".to_string(),
                method: method.to_string(),
            },
            span,
        )),
    }
}

fn between(current: &DurationInterval, a: &DurationInterval, b: &DurationInterval, equal: bool) -> bool {
    let v = current.total_micros();
    let x = a.total_micros();
    let y = b.total_micros();
    let (lo, hi) = if x <= y { (x, y) } else { (y, x) };
    if equal {
        v >= lo && v <= hi
    } else {
        v > lo && v < hi
    }
}

fn expect_interval(args: &[PhpValue], index: usize, span: Span) -> Result<DurationInterval, RuntimeError> {
    let arg = args.get(index).ok_or_else(|| {
        RuntimeError::new(
            RuntimeErrorKind::Custom {
                message: format!("Missing Duration argument at index {index}"),
            },
            span,
        )
    })?;
    if let Some(d) = interval_from_value(arg) {
        return Ok(d);
    }
    if let PhpValue::String(s) = arg {
        return from_string(s).map_err(|e| RuntimeError::new(RuntimeErrorKind::Custom { message: e }, span));
    }
    Err(RuntimeError::new(
        RuntimeErrorKind::Custom {
            message: "Expected a Duration instance".to_string(),
        },
        span,
    ))
}

fn apply_add(current: &DurationInterval, args: &[PhpValue], subtract: bool) -> Result<DurationInterval, String> {
    let Some(first) = args.first() else {
        return Ok(current.clone());
    };
    if let Some(mut other) = interval_from_value(first) {
        if subtract {
            other = other.invert_interval();
        }
        return Ok(current.add_interval(&other));
    }
    if let PhpValue::String(s) = first {
        if args.len() == 1 {
            let mut other = from_string(s)?;
            if subtract {
                other = other.invert_interval();
            }
            return Ok(current.add_interval(&other));
        }
        let unit = parse_unit_name(s).ok_or_else(|| format!("Unknown duration unit '{s}'"))?;
        let qty = arg_f64(args, 1, 1.0);
        let qty = if subtract { -qty } else { qty };
        return Ok(current.add_unit(unit, qty));
    }
    Err("Duration::add() expects a Duration, an interval string, or a unit name".to_string())
}
