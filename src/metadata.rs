use crate::{Precision, Size, Timestamp};
pub(crate) fn size(raw: &str) -> Size {
    let s = raw.trim();
    let digits = s.strip_suffix(" bytes").unwrap_or(s);
    if let Ok(n) = digits.parse::<u64>() {
        return Size::Exact(n);
    }
    let split = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    let unit = s[split..].trim().to_ascii_uppercase();
    if s[..split]
        .parse::<f64>()
        .is_ok_and(|v| v.is_finite() && v >= 0.0)
        && matches!(
            unit.as_str(),
            "K" | "M"
                | "G"
                | "T"
                | "P"
                | "KB"
                | "MB"
                | "GB"
                | "TB"
                | "KIB"
                | "MIB"
                | "GIB"
                | "TIB"
                | "B"
        )
    {
        Size::Approximate(s.into())
    } else {
        Size::Invalid(raw.into())
    }
}
pub(crate) fn timestamp(raw: &str) -> Timestamp {
    let s = raw.trim();
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
        return Timestamp::Offset {
            raw: raw.into(),
            offset_seconds: t.offset().local_minus_utc(),
            precision: if s.contains('.') {
                Precision::Fractional
            } else {
                Precision::Second
            },
        };
    }
    if let Ok(t) = chrono::DateTime::parse_from_rfc2822(s) {
        return Timestamp::Offset {
            raw: raw.into(),
            offset_seconds: t.offset().local_minus_utc(),
            precision: if s
                .split_whitespace()
                .any(|part| part.matches(':').count() == 1)
            {
                Precision::Minute
            } else {
                Precision::Second
            },
        };
    }
    for (fmt, precision) in [
        ("%Y-%m-%d %H:%M", Precision::Minute),
        ("%d-%b-%Y %H:%M", Precision::Minute),
        ("%Y-%m-%d %H:%M:%S", Precision::Second),
    ] {
        if chrono::NaiveDateTime::parse_from_str(s, fmt).is_ok() {
            return Timestamp::Unspecified {
                raw: raw.into(),
                precision,
            };
        }
    }
    if chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
        Timestamp::Date(raw.into())
    } else {
        Timestamp::Invalid(raw.into())
    }
}
