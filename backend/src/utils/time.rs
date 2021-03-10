use chrono::{DateTime, Utc};

/// Return ISO 8601 date-time string with UTC timezone using microsecond resolution.
///
/// eg.
/// ```
/// "2021-02-12T13:30:41.791054"
/// ```
pub fn date_time_iso_str(date_time: &DateTime<Utc>) -> String {
    date_time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
