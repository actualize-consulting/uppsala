//! XPath 2.0 / XDM atomic type system.
//!
//! This module defines the built-in `xs:*` atomic type hierarchy used by XPath
//! 2.0, the temporal/duration/binary value structs, and the lexical validation
//! and casting rules between atomic types. It intentionally depends only on
//! `std` to preserve the crate's zero-dependency policy.

use std::fmt;

use crate::error::XmlError;
use crate::xsd::XS_NAMESPACE;

use super::value::XPath2AtomicValue;

/// A built-in XPath 2.0 atomic type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AtomicType {
    AnyAtomicType,
    UntypedAtomic,
    // String family.
    String,
    NormalizedString,
    Token,
    Language,
    NmToken,
    Name,
    NCName,
    Id,
    IdRef,
    Entity,
    // Boolean.
    Boolean,
    // Numeric family.
    Decimal,
    Integer,
    NonPositiveInteger,
    NegativeInteger,
    Long,
    Int,
    Short,
    Byte,
    NonNegativeInteger,
    UnsignedLong,
    UnsignedInt,
    UnsignedShort,
    UnsignedByte,
    PositiveInteger,
    Float,
    Double,
    // Duration family.
    Duration,
    YearMonthDuration,
    DayTimeDuration,
    // Date/time family.
    DateTime,
    Date,
    Time,
    GYearMonth,
    GYear,
    GMonthDay,
    GDay,
    GMonth,
    // Binary.
    HexBinary,
    Base64Binary,
    // Other primitives.
    AnyUri,
    QName,
    Notation,
}

impl AtomicType {
    /// Resolve a type name to a built-in atomic type.
    ///
    /// The namespace URI must be the XSD namespace; the local name selects the
    /// type. Returns `None` for unknown or non-XSD types.
    pub fn from_name(namespace: Option<&str>, local: &str) -> Option<Self> {
        if namespace != Some(XS_NAMESPACE) {
            return None;
        }
        Some(match local {
            "anyAtomicType" => AtomicType::AnyAtomicType,
            "untypedAtomic" => AtomicType::UntypedAtomic,
            "string" => AtomicType::String,
            "normalizedString" => AtomicType::NormalizedString,
            "token" => AtomicType::Token,
            "language" => AtomicType::Language,
            "NMTOKEN" => AtomicType::NmToken,
            "Name" => AtomicType::Name,
            "NCName" => AtomicType::NCName,
            "ID" => AtomicType::Id,
            "IDREF" => AtomicType::IdRef,
            "ENTITY" => AtomicType::Entity,
            "boolean" => AtomicType::Boolean,
            "decimal" => AtomicType::Decimal,
            "integer" => AtomicType::Integer,
            "nonPositiveInteger" => AtomicType::NonPositiveInteger,
            "negativeInteger" => AtomicType::NegativeInteger,
            "long" => AtomicType::Long,
            "int" => AtomicType::Int,
            "short" => AtomicType::Short,
            "byte" => AtomicType::Byte,
            "nonNegativeInteger" => AtomicType::NonNegativeInteger,
            "unsignedLong" => AtomicType::UnsignedLong,
            "unsignedInt" => AtomicType::UnsignedInt,
            "unsignedShort" => AtomicType::UnsignedShort,
            "unsignedByte" => AtomicType::UnsignedByte,
            "positiveInteger" => AtomicType::PositiveInteger,
            "float" => AtomicType::Float,
            "double" => AtomicType::Double,
            "duration" => AtomicType::Duration,
            "yearMonthDuration" => AtomicType::YearMonthDuration,
            "dayTimeDuration" => AtomicType::DayTimeDuration,
            "dateTime" => AtomicType::DateTime,
            "date" => AtomicType::Date,
            "time" => AtomicType::Time,
            "gYearMonth" => AtomicType::GYearMonth,
            "gYear" => AtomicType::GYear,
            "gMonthDay" => AtomicType::GMonthDay,
            "gDay" => AtomicType::GDay,
            "gMonth" => AtomicType::GMonth,
            "hexBinary" => AtomicType::HexBinary,
            "base64Binary" => AtomicType::Base64Binary,
            "anyURI" => AtomicType::AnyUri,
            "QName" => AtomicType::QName,
            "NOTATION" => AtomicType::Notation,
            _ => return None,
        })
    }

    /// The XSD local name of this atomic type.
    pub fn local_name(self) -> &'static str {
        match self {
            AtomicType::AnyAtomicType => "anyAtomicType",
            AtomicType::UntypedAtomic => "untypedAtomic",
            AtomicType::String => "string",
            AtomicType::NormalizedString => "normalizedString",
            AtomicType::Token => "token",
            AtomicType::Language => "language",
            AtomicType::NmToken => "NMTOKEN",
            AtomicType::Name => "Name",
            AtomicType::NCName => "NCName",
            AtomicType::Id => "ID",
            AtomicType::IdRef => "IDREF",
            AtomicType::Entity => "ENTITY",
            AtomicType::Boolean => "boolean",
            AtomicType::Decimal => "decimal",
            AtomicType::Integer => "integer",
            AtomicType::NonPositiveInteger => "nonPositiveInteger",
            AtomicType::NegativeInteger => "negativeInteger",
            AtomicType::Long => "long",
            AtomicType::Int => "int",
            AtomicType::Short => "short",
            AtomicType::Byte => "byte",
            AtomicType::NonNegativeInteger => "nonNegativeInteger",
            AtomicType::UnsignedLong => "unsignedLong",
            AtomicType::UnsignedInt => "unsignedInt",
            AtomicType::UnsignedShort => "unsignedShort",
            AtomicType::UnsignedByte => "unsignedByte",
            AtomicType::PositiveInteger => "positiveInteger",
            AtomicType::Float => "float",
            AtomicType::Double => "double",
            AtomicType::Duration => "duration",
            AtomicType::YearMonthDuration => "yearMonthDuration",
            AtomicType::DayTimeDuration => "dayTimeDuration",
            AtomicType::DateTime => "dateTime",
            AtomicType::Date => "date",
            AtomicType::Time => "time",
            AtomicType::GYearMonth => "gYearMonth",
            AtomicType::GYear => "gYear",
            AtomicType::GMonthDay => "gMonthDay",
            AtomicType::GDay => "gDay",
            AtomicType::GMonth => "gMonth",
            AtomicType::HexBinary => "hexBinary",
            AtomicType::Base64Binary => "base64Binary",
            AtomicType::AnyUri => "anyURI",
            AtomicType::QName => "QName",
            AtomicType::Notation => "NOTATION",
        }
    }

    /// The immediate base type in the XSD derivation hierarchy.
    pub fn parent(self) -> Option<AtomicType> {
        Some(match self {
            AtomicType::AnyAtomicType => return None,
            AtomicType::UntypedAtomic => AtomicType::AnyAtomicType,
            AtomicType::String => AtomicType::AnyAtomicType,
            AtomicType::NormalizedString => AtomicType::String,
            AtomicType::Token => AtomicType::NormalizedString,
            AtomicType::Language => AtomicType::Token,
            AtomicType::NmToken => AtomicType::Token,
            AtomicType::Name => AtomicType::Token,
            AtomicType::NCName => AtomicType::Name,
            AtomicType::Id => AtomicType::NCName,
            AtomicType::IdRef => AtomicType::NCName,
            AtomicType::Entity => AtomicType::NCName,
            AtomicType::Boolean => AtomicType::AnyAtomicType,
            AtomicType::Decimal => AtomicType::AnyAtomicType,
            AtomicType::Integer => AtomicType::Decimal,
            AtomicType::NonPositiveInteger => AtomicType::Integer,
            AtomicType::NegativeInteger => AtomicType::NonPositiveInteger,
            AtomicType::Long => AtomicType::Integer,
            AtomicType::Int => AtomicType::Long,
            AtomicType::Short => AtomicType::Int,
            AtomicType::Byte => AtomicType::Short,
            AtomicType::NonNegativeInteger => AtomicType::Integer,
            AtomicType::UnsignedLong => AtomicType::NonNegativeInteger,
            AtomicType::UnsignedInt => AtomicType::UnsignedLong,
            AtomicType::UnsignedShort => AtomicType::UnsignedInt,
            AtomicType::UnsignedByte => AtomicType::UnsignedShort,
            AtomicType::PositiveInteger => AtomicType::NonNegativeInteger,
            AtomicType::Float => AtomicType::AnyAtomicType,
            AtomicType::Double => AtomicType::AnyAtomicType,
            AtomicType::Duration => AtomicType::AnyAtomicType,
            AtomicType::YearMonthDuration => AtomicType::Duration,
            AtomicType::DayTimeDuration => AtomicType::Duration,
            AtomicType::DateTime => AtomicType::AnyAtomicType,
            AtomicType::Date => AtomicType::AnyAtomicType,
            AtomicType::Time => AtomicType::AnyAtomicType,
            AtomicType::GYearMonth => AtomicType::AnyAtomicType,
            AtomicType::GYear => AtomicType::AnyAtomicType,
            AtomicType::GMonthDay => AtomicType::AnyAtomicType,
            AtomicType::GDay => AtomicType::AnyAtomicType,
            AtomicType::GMonth => AtomicType::AnyAtomicType,
            AtomicType::HexBinary => AtomicType::AnyAtomicType,
            AtomicType::Base64Binary => AtomicType::AnyAtomicType,
            AtomicType::AnyUri => AtomicType::AnyAtomicType,
            AtomicType::QName => AtomicType::AnyAtomicType,
            AtomicType::Notation => AtomicType::AnyAtomicType,
        })
    }

    /// Whether `self` is the same type as, or derived from, `other`.
    pub fn is_subtype_of(self, other: AtomicType) -> bool {
        let mut current = Some(self);
        while let Some(ty) = current {
            if ty == other {
                return true;
            }
            current = ty.parent();
        }
        false
    }

    /// The primitive base type (one of the 19 XDM primitive atomic types).
    pub fn primitive(self) -> AtomicType {
        let mut current = self;
        loop {
            match current {
                AtomicType::AnyAtomicType
                | AtomicType::UntypedAtomic
                | AtomicType::String
                | AtomicType::Boolean
                | AtomicType::Decimal
                | AtomicType::Float
                | AtomicType::Double
                | AtomicType::Duration
                | AtomicType::DateTime
                | AtomicType::Date
                | AtomicType::Time
                | AtomicType::GYearMonth
                | AtomicType::GYear
                | AtomicType::GMonthDay
                | AtomicType::GDay
                | AtomicType::GMonth
                | AtomicType::HexBinary
                | AtomicType::Base64Binary
                | AtomicType::AnyUri
                | AtomicType::QName
                | AtomicType::Notation => return current,
                // integer is treated as its own canonical base for numeric ops.
                AtomicType::Integer => return AtomicType::Integer,
                _ => match current.parent() {
                    Some(parent) => current = parent,
                    None => return current,
                },
            }
        }
    }

    /// Whether this type is in the numeric family (decimal/integer/float/double).
    pub fn is_numeric(self) -> bool {
        self.is_subtype_of(AtomicType::Decimal)
            || self == AtomicType::Float
            || self == AtomicType::Double
    }
}

impl fmt::Display for AtomicType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "xs:{}", self.local_name())
    }
}

/// A timezone offset in minutes from UTC, with `None` meaning no timezone.
pub type TimeZone = Option<i32>;

/// A normalized date/time value used by `xs:dateTime`, `xs:date`, `xs:time` and
/// the gregorian types. Unused components are zeroed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DateTimeValue {
    pub year: i64,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: f64,
    pub tz: TimeZone,
}

impl DateTimeValue {
    /// Total seconds from an arbitrary epoch, used for ordering. Timezone-naive
    /// values are compared assuming the implicit timezone is UTC.
    pub fn timeline_seconds(&self) -> f64 {
        let days = days_from_civil(self.year, self.month.max(1) as i64, self.day.max(1) as i64);
        let tz_seconds = self.tz.unwrap_or(0) as f64 * 60.0;
        days as f64 * 86400.0 + self.hour as f64 * 3600.0 + self.minute as f64 * 60.0 + self.second
            - tz_seconds
    }
}

/// A normalized duration value, split into a months component (years+months)
/// and a seconds component (days+hours+minutes+seconds), each carrying the
/// overall sign.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DurationValue {
    /// Signed total months (year*12 + month).
    pub months: i64,
    /// Signed total seconds for the day/time component.
    pub seconds: f64,
}

/// Build a `DateTimeValue` from unix seconds in UTC (timezone `Z`).
pub fn datetime_from_unix(unix_seconds: i64) -> DateTimeValue {
    let days = unix_seconds.div_euclid(86400);
    let secs_of_day = unix_seconds.rem_euclid(86400);
    let (year, month, day) = civil_from_days(days);
    DateTimeValue {
        year,
        month: month as u8,
        day: day as u8,
        hour: (secs_of_day / 3600) as u8,
        minute: ((secs_of_day % 3600) / 60) as u8,
        second: (secs_of_day % 60) as f64,
        tz: Some(0),
    }
}

/// Inverse of [`days_from_civil`]: civil `(year, month, day)` from a day count
/// since 1970-01-01. Howard Hinnant's algorithm.
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Number of days since 1970-01-01 (proleptic Gregorian), Howard Hinnant's
/// algorithm. Computed in `i128` and saturated to `i64` so an absurdly large
/// year (accepted as any `i64` lexically) cannot overflow-panic in debug builds;
/// the only consumer feeds the result into `f64` arithmetic where the extreme
/// magnitude is already lossy (see security audit).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i128;
    let m = m as i128;
    let d = d as i128;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    days.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// Parse a leading sign and the calendar/time portions of an `xs:dateTime`,
/// `xs:date`, or `xs:time` lexical value, validating ranges.
pub fn parse_date_time(literal: &str, ty: AtomicType) -> Result<DateTimeValue, XmlError> {
    let s = literal.trim();
    let err = || XmlError::xpath(format!("invalid {} value '{}'", ty, literal));

    match ty {
        AtomicType::DateTime => {
            let (date_part, time_part) = s.split_once('T').ok_or_else(err)?;
            let (year, month, day, _) = parse_date_part(date_part, false).ok_or_else(err)?;
            let (hour, minute, second, tz) = parse_time_part(time_part).ok_or_else(err)?;
            let v = DateTimeValue {
                year,
                month,
                day,
                hour,
                minute,
                second,
                tz,
            };
            validate_ymd(year, month, day).ok_or_else(err)?;
            Ok(v)
        }
        AtomicType::Date => {
            let (year, month, day, tz) = parse_date_part(s, true).ok_or_else(err)?;
            validate_ymd(year, month, day).ok_or_else(err)?;
            Ok(DateTimeValue {
                year,
                month,
                day,
                hour: 0,
                minute: 0,
                second: 0.0,
                tz,
            })
        }
        AtomicType::Time => {
            let (hour, minute, second, tz) = parse_time_part(s).ok_or_else(err)?;
            Ok(DateTimeValue {
                year: 1972,
                month: 12,
                day: 31,
                hour,
                minute,
                second,
                tz,
            })
        }
        _ => Err(err()),
    }
}

fn validate_ymd(_year: i64, month: u8, day: u8) -> Option<()> {
    if !(1..=12).contains(&month) {
        return None;
    }
    let max_day = days_in_month(_year, month);
    if !(1..=max_day).contains(&day) {
        return None;
    }
    Some(())
}

fn days_in_month(year: i64, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Parse `[-]YYYY-MM-DD` optionally followed by a timezone when `allow_tz`.
fn parse_date_part(s: &str, allow_tz: bool) -> Option<(i64, u8, u8, TimeZone)> {
    let (body, tz) = if allow_tz {
        split_timezone(s)?
    } else {
        (s, None)
    };
    let negative = body.starts_with('-');
    let digits = if negative { &body[1..] } else { body };
    let mut parts = digits.splitn(3, '-');
    let year_str = parts.next()?;
    let month_str = parts.next()?;
    let day_str = parts.next()?;
    if year_str.len() < 4 || !year_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if month_str.len() != 2 || day_str.len() != 2 {
        return None;
    }
    let year: i64 = year_str.parse().ok()?;
    let month: u8 = month_str.parse().ok()?;
    let day: u8 = day_str.parse().ok()?;
    Some((if negative { -year } else { year }, month, day, tz))
}

/// Parse `hh:mm:ss[.sss]` with an optional timezone.
fn parse_time_part(s: &str) -> Option<(u8, u8, f64, TimeZone)> {
    let (body, tz) = split_timezone(s)?;
    let mut parts = body.splitn(3, ':');
    let hour_str = parts.next()?;
    let minute_str = parts.next()?;
    let second_str = parts.next()?;
    if hour_str.len() != 2 || minute_str.len() != 2 {
        return None;
    }
    let hour: u8 = hour_str.parse().ok()?;
    let minute: u8 = minute_str.parse().ok()?;
    let second: f64 = second_str.parse().ok()?;
    if minute > 59 || !(0.0..60.0).contains(&second) {
        return None;
    }
    // 24:00:00 is permitted lexically and normalized by callers.
    if hour > 24 || (hour == 24 && (minute != 0 || second != 0.0)) {
        return None;
    }
    Some((hour, minute, second, tz))
}

/// Split an optional trailing timezone (`Z`, `+hh:mm`, `-hh:mm`) from the body.
fn split_timezone(s: &str) -> Option<(&str, TimeZone)> {
    if let Some(body) = s.strip_suffix('Z') {
        return Some((body, Some(0)));
    }
    // A timezone offset is the last 6 chars: ±hh:mm. Guard the char boundary:
    // `split_at` panics if `len - 6` lands inside a multibyte character, which an
    // attacker can force with a trailing non-ASCII byte (see security audit).
    if s.len() >= 6 && s.is_char_boundary(s.len() - 6) {
        let (body, tz) = s.split_at(s.len() - 6);
        let bytes = tz.as_bytes();
        if (bytes[0] == b'+' || bytes[0] == b'-') && bytes[3] == b':' {
            let hh: i32 = tz[1..3].parse().ok()?;
            let mm: i32 = tz[4..6].parse().ok()?;
            if hh > 14 || mm > 59 || (hh == 14 && mm != 0) {
                return None;
            }
            let total = hh * 60 + mm;
            let signed = if bytes[0] == b'-' { -total } else { total };
            return Some((body, Some(signed)));
        }
    }
    Some((s, None))
}

/// Parse an `xs:duration`, `xs:yearMonthDuration`, or `xs:dayTimeDuration`.
pub fn parse_duration(literal: &str, ty: AtomicType) -> Result<DurationValue, XmlError> {
    let s = literal.trim();
    let err = || XmlError::xpath(format!("invalid {} value '{}'", ty, literal));
    let (negative, rest) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let rest = rest.strip_prefix('P').ok_or_else(err)?;
    if rest.is_empty() {
        return Err(err());
    }

    let (date_part, time_part) = match rest.split_once('T') {
        Some((d, t)) => {
            if t.is_empty() {
                return Err(err());
            }
            (d, Some(t))
        }
        None => (rest, None),
    };

    let mut months: i64 = 0;
    let mut seconds: f64 = 0.0;
    let mut saw_field = false;

    // Date portion: nY nM nD
    let mut cursor = date_part;
    for (unit, scale) in [('Y', 12i64), ('M', 1), ('D', 0)] {
        if let Some((num, after)) = take_number(cursor, unit) {
            saw_field = true;
            match unit {
                // Use checked arithmetic: an attacker-supplied component such as
                // `P800000000000000000Y` parses as a valid `i64` but overflows on
                // the `* 12` scale (debug panic / release wrap) — see security audit.
                'Y' => {
                    let scaled = num
                        .parse::<i64>()
                        .map_err(|_| err())?
                        .checked_mul(scale)
                        .ok_or_else(err)?;
                    months = months.checked_add(scaled).ok_or_else(err)?;
                }
                'M' => {
                    months = months
                        .checked_add(num.parse::<i64>().map_err(|_| err())?)
                        .ok_or_else(err)?;
                }
                'D' => seconds += num.parse::<f64>().map_err(|_| err())? * 86400.0,
                _ => unreachable!(),
            }
            cursor = after;
        }
    }
    if !cursor.is_empty() {
        return Err(err());
    }

    if let Some(time_part) = time_part {
        let mut cursor = time_part;
        for (unit, scale) in [('H', 3600.0f64), ('M', 60.0), ('S', 1.0)] {
            if let Some((num, after)) = take_number(cursor, unit) {
                saw_field = true;
                seconds += num.parse::<f64>().map_err(|_| err())? * scale;
                cursor = after;
            }
        }
        if !cursor.is_empty() {
            return Err(err());
        }
    }

    if !saw_field {
        return Err(err());
    }

    // Type-specific restrictions.
    match ty {
        AtomicType::YearMonthDuration if seconds != 0.0 => return Err(err()),
        AtomicType::DayTimeDuration if months != 0 => return Err(err()),
        _ => {}
    }

    let sign = if negative { -1.0 } else { 1.0 };
    Ok(DurationValue {
        months: if negative { -months } else { months },
        seconds: seconds * sign,
    })
}

/// Consume a numeric field terminated by `unit` from the start of `s`.
fn take_number(s: &str, unit: char) -> Option<(&str, &str)> {
    let pos = s.find(unit)?;
    let num = &s[..pos];
    if num.is_empty() {
        return None;
    }
    // The number must be all digits (and possibly a decimal point for seconds).
    if !num.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    Some((num, &s[pos + unit.len_utf8()..]))
}

/// Canonicalize a duration to the `xs:duration` lexical form.
pub fn duration_canonical(value: &DurationValue, ty: AtomicType) -> String {
    let mut out = String::new();
    let negative = value.months < 0 || value.seconds < 0.0;
    if negative {
        out.push('-');
    }
    out.push('P');
    let months = value.months.unsigned_abs();
    let years = months / 12;
    let rem_months = months % 12;
    let seconds_abs = value.seconds.abs();

    if ty != AtomicType::DayTimeDuration {
        if years != 0 {
            out.push_str(&format!("{}Y", years));
        }
        if rem_months != 0 {
            out.push_str(&format!("{}M", rem_months));
        }
    }

    if ty != AtomicType::YearMonthDuration {
        let total = seconds_abs as i64;
        let days = total / 86400;
        let hours = (total % 86400) / 3600;
        let minutes = (total % 3600) / 60;
        let secs = seconds_abs - (days * 86400 + hours * 3600 + minutes * 60) as f64;
        if days != 0 {
            out.push_str(&format!("{}D", days));
        }
        if hours != 0 || minutes != 0 || secs != 0.0 {
            out.push('T');
            if hours != 0 {
                out.push_str(&format!("{}H", hours));
            }
            if minutes != 0 {
                out.push_str(&format!("{}M", minutes));
            }
            if secs != 0.0 {
                out.push_str(&format!("{}S", format_seconds(secs)));
            }
        }
    }

    // Canonical zero forms.
    if ty == AtomicType::YearMonthDuration && value.months == 0 {
        return "P0M".to_string();
    }
    if ty == AtomicType::DayTimeDuration && value.seconds == 0.0 {
        return "PT0S".to_string();
    }
    if out == "P" || out == "-P" {
        out.push_str("T0S");
    }
    out
}

fn format_seconds(secs: f64) -> String {
    if secs.fract() == 0.0 {
        format!("{}", secs as i64)
    } else {
        // Trim trailing zeros from the fractional representation.
        let s = format!("{}", secs);
        s
    }
}

/// Validate the lexical form of a string-derived type, returning the canonical
/// value (with whitespace processing applied) on success.
pub fn validate_string_type(value: &str, ty: AtomicType) -> Result<String, XmlError> {
    let processed = match ty {
        AtomicType::String | AtomicType::NormalizedString | AtomicType::UntypedAtomic => {
            // normalizedString replaces tab/newline/cr with space.
            if ty == AtomicType::NormalizedString {
                value
                    .chars()
                    .map(|c| {
                        if matches!(c, '\t' | '\n' | '\r') {
                            ' '
                        } else {
                            c
                        }
                    })
                    .collect()
            } else {
                value.to_string()
            }
        }
        _ => {
            // token and below collapse whitespace.
            collapse_whitespace(value)
        }
    };

    let ok = match ty {
        AtomicType::String
        | AtomicType::NormalizedString
        | AtomicType::Token
        | AtomicType::UntypedAtomic => true,
        AtomicType::Language => is_language(&processed),
        AtomicType::NmToken => is_nmtoken(&processed),
        AtomicType::Name => is_name(&processed),
        AtomicType::NCName | AtomicType::Id | AtomicType::IdRef | AtomicType::Entity => {
            is_ncname(&processed)
        }
        _ => false,
    };
    if ok {
        Ok(processed)
    } else {
        Err(XmlError::xpath(format!(
            "'{}' is not a valid {}",
            value, ty
        )))
    }
}

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_language(s: &str) -> bool {
    let mut parts = s.split('-');
    let Some(first) = parts.next() else {
        return false;
    };
    if first.is_empty() || first.len() > 8 || !first.bytes().all(|b| b.is_ascii_alphabetic()) {
        return false;
    }
    for part in parts {
        if part.is_empty() || part.len() > 8 || !part.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return false;
        }
    }
    true
}

fn is_nmtoken(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_name_char)
}

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if is_name_start(c) || c == ':' => {}
        _ => return false,
    }
    chars.all(|c| is_name_char(c) || c == ':')
}

fn is_ncname(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if is_name_start(c) => {}
        _ => return false,
    }
    chars.all(is_name_char)
}

fn is_name_start(c: char) -> bool {
    c == '_' || c.is_alphabetic() || (c as u32) >= 0x80
}

fn is_name_char(c: char) -> bool {
    c == '_' || c == '-' || c == '.' || c.is_alphanumeric() || (c as u32) >= 0x80
}

/// Decode a hexBinary lexical value into bytes.
pub fn parse_hex_binary(s: &str) -> Result<Vec<u8>, XmlError> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(XmlError::xpath(format!(
            "invalid xs:hexBinary value '{}'",
            s
        )));
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    for chunk in bytes.chunks(2) {
        let hi = (chunk[0] as char).to_digit(16).unwrap() as u8;
        let lo = (chunk[1] as char).to_digit(16).unwrap() as u8;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

/// Encode bytes as an uppercase hexBinary lexical value.
pub fn hex_binary_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{:02X}", b));
    }
    out
}

const BASE64_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Decode a base64Binary lexical value into bytes.
pub fn parse_base64_binary(s: &str) -> Result<Vec<u8>, XmlError> {
    let cleaned: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if !cleaned.len().is_multiple_of(4) {
        return Err(XmlError::xpath("invalid xs:base64Binary length"));
    }
    let decode = |b: u8| -> Option<u8> {
        BASE64_ALPHABET
            .iter()
            .position(|&c| c == b)
            .map(|p| p as u8)
    };
    let mut out = Vec::new();
    for chunk in cleaned.chunks(4) {
        let pad = chunk.iter().filter(|&&b| b == b'=').count();
        let c0 = decode(chunk[0]).ok_or_else(|| XmlError::xpath("invalid base64 char"))?;
        let c1 = decode(chunk[1]).ok_or_else(|| XmlError::xpath("invalid base64 char"))?;
        let n = ((c0 as u32) << 18) | ((c1 as u32) << 12);
        out.push((n >> 16) as u8);
        if pad < 2 {
            let c2 = decode(chunk[2]).ok_or_else(|| XmlError::xpath("invalid base64 char"))?;
            let n = n | ((c2 as u32) << 6);
            out.push((n >> 8) as u8);
            if pad < 1 {
                let c3 = decode(chunk[3]).ok_or_else(|| XmlError::xpath("invalid base64 char"))?;
                let n = n | (c3 as u32);
                out.push(n as u8);
            }
        }
    }
    Ok(out)
}

/// Encode bytes as a base64Binary lexical value.
pub fn base64_binary_string(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(BASE64_ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(BASE64_ALPHABET[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(BASE64_ALPHABET[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(BASE64_ALPHABET[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Validate that a decimal lexical form fits one of the integer subtypes by
/// range. Returns `Ok` if the value (already known to be an integer) satisfies
/// the bounds of `ty`.
pub fn integer_in_range(value: i128, ty: AtomicType) -> bool {
    match ty {
        AtomicType::Integer | AtomicType::Decimal => true,
        AtomicType::NonPositiveInteger => value <= 0,
        AtomicType::NegativeInteger => value < 0,
        AtomicType::Long => i64::try_from(value).is_ok(),
        AtomicType::Int => i32::try_from(value).is_ok(),
        AtomicType::Short => i16::try_from(value).is_ok(),
        AtomicType::Byte => i8::try_from(value).is_ok(),
        AtomicType::NonNegativeInteger => value >= 0,
        AtomicType::UnsignedLong => (0..=u64::MAX as i128).contains(&value),
        AtomicType::UnsignedInt => (0..=u32::MAX as i128).contains(&value),
        AtomicType::UnsignedShort => (0..=u16::MAX as i128).contains(&value),
        AtomicType::UnsignedByte => (0..=u8::MAX as i128).contains(&value),
        AtomicType::PositiveInteger => value > 0,
        _ => false,
    }
}

/// Convenience used by the evaluator to report cast/type failures with the
/// FORG0001 / XPTY style classification baked into the message.
pub fn cast_error(value: &XPath2AtomicValue, target: AtomicType) -> XmlError {
    XmlError::xpath_code(
        "FORG0001",
        format!(
            "cannot cast value '{}' to {}",
            value.to_xpath_string(),
            target
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtype_relationships() {
        assert!(AtomicType::Int.is_subtype_of(AtomicType::Integer));
        assert!(AtomicType::Int.is_subtype_of(AtomicType::Decimal));
        assert!(AtomicType::NCName.is_subtype_of(AtomicType::String));
        assert!(!AtomicType::String.is_subtype_of(AtomicType::NCName));
        assert!(AtomicType::YearMonthDuration.is_subtype_of(AtomicType::Duration));
    }

    #[test]
    fn parses_datetime() {
        let v = parse_date_time("2004-04-12T13:20:00Z", AtomicType::DateTime).unwrap();
        assert_eq!(v.year, 2004);
        assert_eq!(v.month, 4);
        assert_eq!(v.tz, Some(0));
    }

    #[test]
    fn parses_duration() {
        let d = parse_duration("P1Y2M3DT4H5M6S", AtomicType::Duration).unwrap();
        assert_eq!(d.months, 14);
        assert_eq!(d.seconds, 3.0 * 86400.0 + 4.0 * 3600.0 + 5.0 * 60.0 + 6.0);
    }

    #[test]
    fn hex_binary_roundtrip() {
        let bytes = parse_hex_binary("0FB7").unwrap();
        assert_eq!(bytes, vec![0x0F, 0xB7]);
        assert_eq!(hex_binary_string(&bytes), "0FB7");
    }

    #[test]
    fn base64_roundtrip() {
        let bytes = parse_base64_binary("SGVsbG8=").unwrap();
        assert_eq!(&bytes, b"Hello");
        assert_eq!(base64_binary_string(&bytes), "SGVsbG8=");
    }
}
