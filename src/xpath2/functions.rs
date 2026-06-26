//! XPath 2.0 casting rules and value-level Functions & Operators helpers.
//!
//! This module holds the pure, context-light pieces of the XPath 2.0 function
//! library: casting between atomic types, typed atomic comparison with the
//! numeric type-promotion rules, and string/sequence helpers. The evaluator
//! dispatches `fn:*` calls and delegates the value-level work here.

use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::HashMap;

use crate::error::{XmlError, XmlResult};
use crate::xsd_regex::XsdRegex;

use super::types::{self, cast_error, integer_in_range, AtomicType, DateTimeValue, DurationValue};
use super::value::{QNameValue, XPath2AtomicValue};

/// Cast an atomic value to `target`, applying XPath 2.0 casting rules.
///
/// `namespaces` is consulted only when casting a string to `xs:QName` so the
/// prefix can be resolved to a namespace URI.
pub fn cast_to(
    value: &XPath2AtomicValue,
    target: AtomicType,
    namespaces: &HashMap<String, String>,
) -> XmlResult<XPath2AtomicValue> {
    // Casting to a derived string/integer type validates then wraps.
    let source_type = value.type_of();

    // Identity / trivial cases.
    if source_type == target {
        return Ok(value.clone());
    }

    match target {
        AtomicType::AnyAtomicType => Err(XmlError::xpath_code(
            "XPST0080",
            "cannot cast to xs:anyAtomicType",
        )),
        AtomicType::UntypedAtomic => Ok(XPath2AtomicValue::UntypedAtomic(value.to_xpath_string())),
        AtomicType::String => cast_to_string(value),
        AtomicType::NormalizedString
        | AtomicType::Token
        | AtomicType::Language
        | AtomicType::NmToken
        | AtomicType::Name
        | AtomicType::NCName
        | AtomicType::Id
        | AtomicType::IdRef
        | AtomicType::Entity => {
            let s = cast_to_string(value)?.to_xpath_string();
            let validated = types::validate_string_type(&s, target)?;
            Ok(wrap_derived(target, XPath2AtomicValue::String(validated)))
        }
        AtomicType::Boolean => cast_to_boolean(value),
        AtomicType::Decimal => cast_to_decimal(value),
        AtomicType::Integer => cast_to_integer(value, AtomicType::Integer),
        AtomicType::NonPositiveInteger
        | AtomicType::NegativeInteger
        | AtomicType::Long
        | AtomicType::Int
        | AtomicType::Short
        | AtomicType::Byte
        | AtomicType::NonNegativeInteger
        | AtomicType::UnsignedLong
        | AtomicType::UnsignedInt
        | AtomicType::UnsignedShort
        | AtomicType::UnsignedByte
        | AtomicType::PositiveInteger => cast_to_integer(value, target),
        AtomicType::Float => Ok(XPath2AtomicValue::Float(
            cast_to_double_value(value)? as f32 as f64,
        )),
        AtomicType::Double => Ok(XPath2AtomicValue::Double(cast_to_double_value(value)?)),
        AtomicType::AnyUri => Ok(XPath2AtomicValue::AnyUri(value.to_xpath_string())),
        AtomicType::QName => cast_to_qname(value, namespaces),
        AtomicType::Duration | AtomicType::YearMonthDuration | AtomicType::DayTimeDuration => {
            cast_to_duration(value, target)
        }
        AtomicType::DateTime | AtomicType::Date | AtomicType::Time => {
            cast_to_date_time(value, target)
        }
        AtomicType::GYearMonth
        | AtomicType::GYear
        | AtomicType::GMonthDay
        | AtomicType::GDay
        | AtomicType::GMonth => cast_to_gregorian(value, target),
        AtomicType::HexBinary => cast_to_hex_binary(value),
        AtomicType::Base64Binary => cast_to_base64_binary(value),
        AtomicType::Notation => Err(XmlError::xpath_code(
            "XPST0080",
            "cannot cast to xs:NOTATION",
        )),
    }
}

/// Whether a value is castable to `target` (`castable as`), i.e. `cast as`
/// would succeed.
pub fn castable(
    value: &XPath2AtomicValue,
    target: AtomicType,
    namespaces: &HashMap<String, String>,
) -> bool {
    cast_to(value, target, namespaces).is_ok()
}

fn wrap_derived(target: AtomicType, base: XPath2AtomicValue) -> XPath2AtomicValue {
    XPath2AtomicValue::Derived(target, Box::new(base))
}

fn cast_to_string(value: &XPath2AtomicValue) -> XmlResult<XPath2AtomicValue> {
    Ok(XPath2AtomicValue::String(value.to_xpath_string()))
}

fn cast_to_boolean(value: &XPath2AtomicValue) -> XmlResult<XPath2AtomicValue> {
    let result = match value.base() {
        XPath2AtomicValue::Boolean(b) => *b,
        XPath2AtomicValue::Double(n) | XPath2AtomicValue::Float(n) => *n != 0.0 && !n.is_nan(),
        XPath2AtomicValue::Integer(_) | XPath2AtomicValue::Decimal(_) => value.as_f64()? != 0.0,
        XPath2AtomicValue::String(s) | XPath2AtomicValue::UntypedAtomic(s) => match s.trim() {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => return Err(cast_error(value, AtomicType::Boolean)),
        },
        _ => return Err(cast_error(value, AtomicType::Boolean)),
    };
    Ok(XPath2AtomicValue::Boolean(result))
}

fn cast_to_decimal(value: &XPath2AtomicValue) -> XmlResult<XPath2AtomicValue> {
    match value.base() {
        XPath2AtomicValue::Boolean(b) => Ok(XPath2AtomicValue::Decimal(
            if *b { "1" } else { "0" }.to_string(),
        )),
        XPath2AtomicValue::Double(n) | XPath2AtomicValue::Float(n) => {
            if !n.is_finite() {
                return Err(cast_error(value, AtomicType::Decimal));
            }
            Ok(XPath2AtomicValue::Decimal(format_decimal(*n)))
        }
        XPath2AtomicValue::Integer(s) | XPath2AtomicValue::Decimal(s) => {
            Ok(XPath2AtomicValue::Decimal(normalize_decimal_lexical(s)?))
        }
        XPath2AtomicValue::String(s) | XPath2AtomicValue::UntypedAtomic(s) => {
            Ok(XPath2AtomicValue::Decimal(normalize_decimal_lexical(s)?))
        }
        _ => Err(cast_error(value, AtomicType::Decimal)),
    }
}

fn cast_to_integer(value: &XPath2AtomicValue, target: AtomicType) -> XmlResult<XPath2AtomicValue> {
    let int = match value.base() {
        XPath2AtomicValue::Boolean(b) => {
            if *b {
                1
            } else {
                0
            }
        }
        XPath2AtomicValue::Double(n) | XPath2AtomicValue::Float(n) => {
            if !n.is_finite() {
                return Err(cast_error(value, target));
            }
            n.trunc() as i128
        }
        XPath2AtomicValue::Integer(s) => s
            .trim()
            .parse::<i128>()
            .map_err(|_| cast_error(value, target))?,
        XPath2AtomicValue::Decimal(s)
        | XPath2AtomicValue::String(s)
        | XPath2AtomicValue::UntypedAtomic(s) => {
            parse_decimal_to_integer(s.trim()).ok_or_else(|| cast_error(value, target))?
        }
        _ => return Err(cast_error(value, target)),
    };
    if !integer_in_range(int, target) {
        return Err(XmlError::xpath_code(
            "FORG0001",
            format!("{} out of range for {}", int, target),
        ));
    }
    let base = XPath2AtomicValue::Integer(int.to_string());
    if target == AtomicType::Integer {
        Ok(base)
    } else {
        Ok(wrap_derived(target, base))
    }
}

fn cast_to_double_value(value: &XPath2AtomicValue) -> XmlResult<f64> {
    match value.base() {
        XPath2AtomicValue::Boolean(b) => Ok(if *b { 1.0 } else { 0.0 }),
        other => other.as_f64(),
    }
    .map_err(|_| cast_error(value, AtomicType::Double))
}

fn cast_to_qname(
    value: &XPath2AtomicValue,
    namespaces: &HashMap<String, String>,
) -> XmlResult<XPath2AtomicValue> {
    match value.base() {
        XPath2AtomicValue::QName(q) => Ok(XPath2AtomicValue::QName(q.clone())),
        XPath2AtomicValue::String(s) | XPath2AtomicValue::UntypedAtomic(s) => {
            let s = s.trim();
            let (prefix, local) = match s.split_once(':') {
                Some((p, l)) => (Some(p.to_string()), l.to_string()),
                None => (None, s.to_string()),
            };
            let uri = match &prefix {
                Some(p) => Some(namespaces.get(p).cloned().ok_or_else(|| {
                    XmlError::xpath_code(
                        "FONS0004",
                        format!("no namespace bound to prefix '{}'", p),
                    )
                })?),
                None => namespaces.get("").cloned(),
            };
            Ok(XPath2AtomicValue::QName(QNameValue { prefix, uri, local }))
        }
        _ => Err(cast_error(value, AtomicType::QName)),
    }
}

fn cast_to_duration(value: &XPath2AtomicValue, target: AtomicType) -> XmlResult<XPath2AtomicValue> {
    match value.base() {
        XPath2AtomicValue::Duration(d, _) => Ok(coerce_duration(*d, target)),
        XPath2AtomicValue::String(s) | XPath2AtomicValue::UntypedAtomic(s) => {
            let d = types::parse_duration(s, target)?;
            Ok(XPath2AtomicValue::Duration(d, target))
        }
        _ => Err(cast_error(value, target)),
    }
}

fn coerce_duration(d: DurationValue, target: AtomicType) -> XPath2AtomicValue {
    let coerced = match target {
        AtomicType::YearMonthDuration => DurationValue {
            months: d.months,
            seconds: 0.0,
        },
        AtomicType::DayTimeDuration => DurationValue {
            months: 0,
            seconds: d.seconds,
        },
        _ => d,
    };
    XPath2AtomicValue::Duration(coerced, target)
}

fn cast_to_date_time(
    value: &XPath2AtomicValue,
    target: AtomicType,
) -> XmlResult<XPath2AtomicValue> {
    // Cross-casts among dateTime/date/time follow XSD truncation rules.
    let derive = |v: DateTimeValue| match target {
        AtomicType::DateTime => XPath2AtomicValue::DateTime(v),
        AtomicType::Date => XPath2AtomicValue::Date(v),
        AtomicType::Time => XPath2AtomicValue::Time(v),
        _ => unreachable!(),
    };
    match value.base() {
        XPath2AtomicValue::DateTime(v) => Ok(derive(*v)),
        XPath2AtomicValue::Date(v) if target == AtomicType::DateTime => Ok(derive(*v)),
        XPath2AtomicValue::String(s) | XPath2AtomicValue::UntypedAtomic(s) => {
            let v = types::parse_date_time(s, target)?;
            Ok(derive(v))
        }
        _ => Err(cast_error(value, target)),
    }
}

fn cast_to_gregorian(
    value: &XPath2AtomicValue,
    target: AtomicType,
) -> XmlResult<XPath2AtomicValue> {
    match value.base() {
        XPath2AtomicValue::String(s) | XPath2AtomicValue::UntypedAtomic(s) => {
            let v = parse_gregorian(s, target)?;
            Ok(XPath2AtomicValue::Gregorian(v, target))
        }
        _ => Err(cast_error(value, target)),
    }
}

fn cast_to_hex_binary(value: &XPath2AtomicValue) -> XmlResult<XPath2AtomicValue> {
    match value.base() {
        XPath2AtomicValue::HexBinary(b) => Ok(XPath2AtomicValue::HexBinary(b.clone())),
        XPath2AtomicValue::Base64Binary(b) => Ok(XPath2AtomicValue::HexBinary(b.clone())),
        XPath2AtomicValue::String(s) | XPath2AtomicValue::UntypedAtomic(s) => {
            Ok(XPath2AtomicValue::HexBinary(types::parse_hex_binary(s)?))
        }
        _ => Err(cast_error(value, AtomicType::HexBinary)),
    }
}

fn cast_to_base64_binary(value: &XPath2AtomicValue) -> XmlResult<XPath2AtomicValue> {
    match value.base() {
        XPath2AtomicValue::Base64Binary(b) => Ok(XPath2AtomicValue::Base64Binary(b.clone())),
        XPath2AtomicValue::HexBinary(b) => Ok(XPath2AtomicValue::Base64Binary(b.clone())),
        XPath2AtomicValue::String(s) | XPath2AtomicValue::UntypedAtomic(s) => Ok(
            XPath2AtomicValue::Base64Binary(types::parse_base64_binary(s)?),
        ),
        _ => Err(cast_error(value, AtomicType::Base64Binary)),
    }
}

/// Parse a gregorian lexical value (`gYear`, `gYearMonth`, etc.).
fn parse_gregorian(s: &str, ty: AtomicType) -> XmlResult<DateTimeValue> {
    let s = s.trim();
    let err = || XmlError::xpath_code("FORG0001", format!("invalid {} value '{}'", ty, s));
    // Reuse a synthetic full date and validate the relevant parts.
    let mut v = DateTimeValue {
        year: 1,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0.0,
        tz: None,
    };
    let (body, tz) = split_tz_public(s);
    v.tz = tz;
    match ty {
        AtomicType::GYear => {
            v.year = body.parse().map_err(|_| err())?;
        }
        AtomicType::GYearMonth => {
            let (y, m) = body.rsplit_once('-').ok_or_else(err)?;
            v.year = y.parse().map_err(|_| err())?;
            v.month = m.parse().map_err(|_| err())?;
        }
        AtomicType::GMonth => {
            let digits = body.strip_prefix("--").ok_or_else(err)?;
            v.month = digits.parse().map_err(|_| err())?;
        }
        AtomicType::GMonthDay => {
            let digits = body.strip_prefix("--").ok_or_else(err)?;
            let (m, d) = digits.split_once('-').ok_or_else(err)?;
            v.month = m.parse().map_err(|_| err())?;
            v.day = d.parse().map_err(|_| err())?;
        }
        AtomicType::GDay => {
            let digits = body.strip_prefix("---").ok_or_else(err)?;
            v.day = digits.parse().map_err(|_| err())?;
        }
        _ => return Err(err()),
    }
    if v.month > 12 || v.day > 31 {
        return Err(err());
    }
    Ok(v)
}

fn split_tz_public(s: &str) -> (&str, Option<i32>) {
    if let Some(body) = s.strip_suffix('Z') {
        return (body, Some(0));
    }
    // Guard the char boundary: `split_at` panics if `len - 6` lands inside a
    // multibyte character (see security audit).
    if s.len() >= 6 && s.is_char_boundary(s.len() - 6) {
        let (body, tz) = s.split_at(s.len() - 6);
        let bytes = tz.as_bytes();
        if (bytes[0] == b'+' || bytes[0] == b'-') && bytes[3] == b':' {
            if let (Ok(hh), Ok(mm)) = (tz[1..3].parse::<i32>(), tz[4..6].parse::<i32>()) {
                let total = hh * 60 + mm;
                let signed = if bytes[0] == b'-' { -total } else { total };
                return (body, Some(signed));
            }
        }
    }
    (s, None)
}

fn format_decimal(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i128)
    } else {
        // Render without exponent and trim trailing zeros.
        let mut s = format!("{:.18}", n);
        while s.contains('.') && s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
        s
    }
}

fn normalize_decimal_lexical(s: &str) -> XmlResult<String> {
    let t = s.trim();
    let (sign, body) = match t.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", t.strip_prefix('+').unwrap_or(t)),
    };
    if body.is_empty() {
        return Err(XmlError::xpath_code(
            "FORG0001",
            format!("invalid decimal '{}'", s),
        ));
    }
    let valid = body.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && body.bytes().filter(|&b| b == b'.').count() <= 1;
    if !valid {
        return Err(XmlError::xpath_code(
            "FORG0001",
            format!("invalid decimal '{}'", s),
        ));
    }
    // Strip insignificant leading/trailing zeros, keep canonical form.
    let (int_part, frac_part) = match body.split_once('.') {
        Some((i, f)) => (i, f),
        None => (body, ""),
    };
    let int_trimmed = int_part.trim_start_matches('0');
    let int_canon = if int_trimmed.is_empty() {
        "0"
    } else {
        int_trimmed
    };
    let frac_trimmed = frac_part.trim_end_matches('0');
    let mut out = String::new();
    if sign == "-" && !(int_canon == "0" && frac_trimmed.is_empty()) {
        out.push('-');
    }
    out.push_str(int_canon);
    if !frac_trimmed.is_empty() {
        out.push('.');
        out.push_str(frac_trimmed);
    }
    Ok(out)
}

fn parse_decimal_to_integer(s: &str) -> Option<i128> {
    let normalized = normalize_decimal_lexical(s).ok()?;
    match normalized.split_once('.') {
        Some((whole, _frac)) => {
            // Truncate toward zero.
            if whole.is_empty() || whole == "-" {
                Some(0)
            } else {
                whole.parse::<i128>().ok()
            }
        }
        None => normalized.parse::<i128>().ok(),
    }
}

// ---------------------------------------------------------------------------
// Collation
// ---------------------------------------------------------------------------

/// The default Unicode codepoint collation URI.
pub const CODEPOINT_COLLATION: &str = "http://www.w3.org/2005/xpath-functions/collation/codepoint";

/// Compare two strings under the default codepoint collation.
pub fn codepoint_compare(a: &str, b: &str) -> Ordering {
    a.chars().cmp(b.chars())
}

// ---------------------------------------------------------------------------
// Regular expression helpers (built on the anchored XSD regex engine)
// ---------------------------------------------------------------------------

/// Compile an XPath regex pattern. The `i` and `x` flags are honored by a light
/// preprocessing pass; other flags are accepted but ignored. The underlying
/// engine is the XSD regex matcher, which matches the whole input, so callers
/// drive substring semantics by testing slices.
fn compile_regex(pattern: &str, flags: &str) -> XmlResult<XsdRegex> {
    for f in flags.chars() {
        if !matches!(f, 's' | 'm' | 'i' | 'x' | 'q') {
            return Err(XmlError::xpath_code(
                "FORX0001",
                format!("invalid regex flag '{}'", f),
            ));
        }
    }
    let mut effective = pattern.to_string();
    if flags.contains('x') {
        // Free-spacing: remove unescaped whitespace.
        effective = strip_freespacing(&effective);
    }
    if flags.contains('q') {
        // Quote the whole pattern as a literal.
        effective = quote_regex(&effective);
    }
    if flags.contains('i') {
        // Case-insensitivity is approximated by ASCII-lower-casing the pattern's
        // literal letters and the tested input; this is a best-effort subset.
        // ASCII folding is length-preserving, so byte offsets into the original
        // input stay valid and `fn:replace`/`fn:tokenize` return the original
        // (not lower-cased) text (see security audit, F8).
        effective = effective.to_ascii_lowercase();
    }
    XsdRegex::compile(&effective).map_err(|e| {
        XmlError::xpath_code("FORX0002", format!("invalid regex '{}': {}", pattern, e))
    })
}

fn strip_freespacing(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut escaped = false;
    let mut in_class = false;
    for c in pattern.chars() {
        if escaped {
            out.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' => {
                out.push(c);
                escaped = true;
            }
            '[' => {
                in_class = true;
                out.push(c);
            }
            ']' => {
                in_class = false;
                out.push(c);
            }
            c if c.is_whitespace() && !in_class => {}
            c => out.push(c),
        }
    }
    out
}

fn quote_regex(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len() * 2);
    for c in pattern.chars() {
        if matches!(
            c,
            '\\' | '.'
                | '*'
                | '+'
                | '?'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '|'
                | '^'
                | '$'
                | '-'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn apply_case_fold(s: &str, flags: &str) -> String {
    if flags.contains('i') {
        // ASCII-only fold keeps the byte length identical to `s`, preserving the
        // offset mapping used by `input_slice` (see security audit, F8).
        s.to_ascii_lowercase()
    } else {
        s.to_string()
    }
}

/// `fn:matches`: whether `input` contains a substring matching `pattern`.
///
/// The XSD engine matches whole strings, so this scans char-boundary slices.
/// Anchored semantics (`^`/`$`) of the XPath flavor are not modeled.
pub fn regex_matches(input: &str, pattern: &str, flags: &str) -> XmlResult<bool> {
    let regex = compile_regex(pattern, flags)?;
    let folded = apply_case_fold(input, flags);
    let work = Cell::new(REGEX_WORK_BUDGET);
    if find_match_from(&regex, &folded, 0, &work)?.is_some() {
        return Ok(true);
    }
    // A pattern that accepts the zero-length string matches at every position.
    Ok(regex.is_match(""))
}

/// Total work budget shared across a single `fn:matches`/`replace`/`tokenize`
/// call. Because the underlying engine only matches whole strings, finding a
/// substring match requires probing `O(n^2)` `(begin, end)` slices; without a
/// cap a large input or a catastrophic-backtracking pattern would hang the
/// thread (see security audit, F4). The budget bounds total work to a finite,
/// sub-second amount and fails closed (`FORX0002`) when exceeded.
const REGEX_WORK_BUDGET: usize = 500_000_000;

/// Find the leftmost-longest match starting at or after `start`. Returns the
/// `(begin, end)` byte offsets of the match, or `None`. Charges each match
/// attempt against the shared `work` budget and caps the per-attempt step count,
/// so the total cost across the whole operation is bounded regardless of input
/// length or pattern (see security audit, F4).
fn find_match_from(
    regex: &XsdRegex,
    s: &str,
    start: usize,
    work: &Cell<usize>,
) -> XmlResult<Option<(usize, usize)>> {
    let boundaries: Vec<usize> = (start..=s.len())
        .filter(|i| s.is_char_boundary(*i))
        .collect();
    for (bi, &begin) in boundaries.iter().enumerate() {
        // Prefer the longest match at this start (greedy leftmost-longest).
        for &end in boundaries[bi..].iter().rev() {
            if end <= begin {
                continue;
            }
            // Cost is proportional to the slice length; it both charges the
            // shared budget and bounds the per-attempt step count so a
            // pathological pattern is cut off (treated as non-match) instead of
            // backtracking exponentially.
            let cost = (end - begin).saturating_mul(4).saturating_add(256);
            let remaining = work.get();
            if remaining < cost {
                return Err(XmlError::xpath_code(
                    "FORX0002",
                    "regular expression evaluation exceeded its work budget",
                ));
            }
            work.set(remaining - cost);
            if regex.is_match_with_max_steps(&s[begin..end], cost) {
                return Ok(Some((begin, end)));
            }
        }
    }
    Ok(None)
}

/// `fn:tokenize`: split `input` on matches of `pattern`.
pub fn regex_tokenize(input: &str, pattern: &str, flags: &str) -> XmlResult<Vec<String>> {
    let regex = compile_regex(pattern, flags)?;
    // A pattern matching the empty string is an error for tokenize.
    if regex.is_match("") {
        return Err(XmlError::xpath_code(
            "FORX0003",
            "tokenize pattern matches the zero-length string",
        ));
    }
    let folded = apply_case_fold(input, flags);
    let work = Cell::new(REGEX_WORK_BUDGET);
    let mut tokens = Vec::new();
    let mut pos = 0;
    while pos <= folded.len() {
        match find_match_from(&regex, &folded, pos, &work)? {
            Some((begin, end)) => {
                tokens.push(input_slice(input, &folded, pos, begin));
                pos = end;
            }
            None => {
                tokens.push(input_slice(input, &folded, pos, folded.len()));
                break;
            }
        }
    }
    Ok(tokens)
}

/// `fn:replace`: replace each match of `pattern` with `replacement`.
///
/// Capturing-group references (`$1`..`$9`) are not supported because the
/// underlying engine does not expose captures; `$0` and literal text are.
pub fn regex_replace(
    input: &str,
    pattern: &str,
    replacement: &str,
    flags: &str,
) -> XmlResult<String> {
    let regex = compile_regex(pattern, flags)?;
    if regex.is_match("") {
        return Err(XmlError::xpath_code(
            "FORX0003",
            "replace pattern matches the zero-length string",
        ));
    }
    let folded = apply_case_fold(input, flags);
    let work = Cell::new(REGEX_WORK_BUDGET);
    let mut out = String::new();
    let mut pos = 0;
    while pos <= folded.len() {
        match find_match_from(&regex, &folded, pos, &work)? {
            Some((begin, end)) => {
                out.push_str(&input_slice(input, &folded, pos, begin));
                let matched = input_slice(input, &folded, begin, end);
                out.push_str(&expand_replacement(replacement, &matched)?);
                pos = end;
            }
            None => {
                out.push_str(&input_slice(input, &folded, pos, folded.len()));
                break;
            }
        }
    }
    Ok(out)
}

/// Slice the original `input` over the byte range that `folded` (same length
/// under case folding, which preserves ASCII byte offsets for the common case)
/// identifies. When case folding changed the length, fall back to the folded
/// slice so offsets stay valid.
fn input_slice(input: &str, folded: &str, start: usize, end: usize) -> String {
    if input.len() == folded.len() && input.is_char_boundary(start) && input.is_char_boundary(end) {
        input[start..end].to_string()
    } else {
        folded[start..end].to_string()
    }
}

fn expand_replacement(replacement: &str, matched: &str) -> XmlResult<String> {
    let mut out = String::new();
    let mut chars = replacement.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('\\') => out.push('\\'),
                Some('$') => out.push('$'),
                Some(other) => {
                    return Err(XmlError::xpath_code(
                        "FORX0004",
                        format!("invalid replacement escape '\\{}'", other),
                    ))
                }
                None => {
                    return Err(XmlError::xpath_code(
                        "FORX0004",
                        "dangling backslash in replacement string",
                    ))
                }
            },
            '$' => {
                // Only $0 (the whole match) is supported; other group refs
                // expand to the empty string.
                match chars.next() {
                    Some('0') => out.push_str(matched),
                    Some(d) if d.is_ascii_digit() => {}
                    Some(other) => {
                        out.push('$');
                        out.push(other);
                    }
                    None => out.push('$'),
                }
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Sequence helpers
// ---------------------------------------------------------------------------

/// `fn:deep-equal` for atomic values (node comparison is handled in the
/// evaluator where the document is available).
pub fn atomic_deep_equal(a: &XPath2AtomicValue, b: &XPath2AtomicValue) -> bool {
    use XPath2AtomicValue::*;
    // NaN is deep-equal to NaN under fn:deep-equal.
    match (a.base(), b.base()) {
        (Double(x) | Float(x), Double(y) | Float(y)) => (x.is_nan() && y.is_nan()) || x == y,
        _ => {
            let an = a.type_of();
            let bn = b.type_of();
            if an.is_numeric() && bn.is_numeric() {
                return a.as_f64().ok() == b.as_f64().ok();
            }
            a.to_xpath_string() == b.to_xpath_string() && comparable_kind(an) == comparable_kind(bn)
        }
    }
}

fn comparable_kind(ty: AtomicType) -> u8 {
    if ty.is_numeric() {
        0
    } else if ty == AtomicType::Boolean {
        1
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns() -> HashMap<String, String> {
        HashMap::new()
    }

    #[test]
    fn regex_matches_substring() {
        assert!(regex_matches("hello world", "wor", "").unwrap());
        assert!(!regex_matches("hello", "xyz", "").unwrap());
        assert!(regex_matches("abc123", "[0-9]+", "").unwrap());
    }

    #[test]
    fn regex_tokenize_splits() {
        assert_eq!(
            regex_tokenize("a,b,c", ",", "").unwrap(),
            vec!["a", "b", "c"]
        );
        assert_eq!(
            regex_tokenize("1  2   3", " +", "").unwrap(),
            vec!["1", "2", "3"]
        );
    }

    #[test]
    fn regex_replace_basic() {
        assert_eq!(regex_replace("a1b2", "[0-9]", "#", "").unwrap(), "a#b#");
    }

    #[test]
    fn casts_string_to_integer() {
        let v = cast_to(
            &XPath2AtomicValue::String("42".into()),
            AtomicType::Integer,
            &ns(),
        )
        .unwrap();
        assert_eq!(v, XPath2AtomicValue::Integer("42".into()));
    }

    #[test]
    fn casts_integer_to_long_wraps_derived() {
        let v = cast_to(
            &XPath2AtomicValue::Integer("5".into()),
            AtomicType::Long,
            &ns(),
        )
        .unwrap();
        assert_eq!(v.type_of(), AtomicType::Long);
        assert_eq!(v.to_xpath_string(), "5");
    }

    #[test]
    fn rejects_out_of_range_byte() {
        assert!(cast_to(
            &XPath2AtomicValue::Integer("999".into()),
            AtomicType::Byte,
            &ns()
        )
        .is_err());
    }

    #[test]
    fn casts_string_to_boolean() {
        assert_eq!(
            cast_to(
                &XPath2AtomicValue::String("true".into()),
                AtomicType::Boolean,
                &ns()
            )
            .unwrap(),
            XPath2AtomicValue::Boolean(true)
        );
        assert!(cast_to(
            &XPath2AtomicValue::String("maybe".into()),
            AtomicType::Boolean,
            &ns()
        )
        .is_err());
    }
}
