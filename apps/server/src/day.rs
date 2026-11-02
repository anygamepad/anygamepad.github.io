//! Calendar days (UTC) from the clock: the server keeps the day a document
//! arrived, never the time, like the reports themselves.

use anygamepad_report::Date;

const EPOCH: Date = match Date::new(1970, 1, 1) {
    Some(date) => date,
    None => unreachable!(),
};

/// The UTC day of a time in milliseconds since 1970-01-01; 1970-01-01 for
/// a time before it.
pub fn from_unix_ms(ms: f64) -> Date {
    Date::from_unix_seconds((ms / 1000.0).floor() as i64).unwrap_or(EPOCH)
}

/// Days from 1970-01-01 to `date`, from Howard Hinnant's
/// `days_from_civil`.
pub fn unix_days(date: Date) -> i64 {
    let (month, day) = (i64::from(date.month()), i64::from(date.day()));
    let year = i64::from(date.year()) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The day `days` before `date`; 1970-01-01 at the earliest.
pub fn days_before(date: Date, days: u32) -> Date {
    let seconds = (unix_days(date) - i64::from(days)) * 86_400;
    Date::from_unix_seconds(seconds).unwrap_or(EPOCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_from_the_clock() {
        assert_eq!(from_unix_ms(0.0).to_string(), "1970-01-01");
        assert_eq!(from_unix_ms(86_399_999.0).to_string(), "1970-01-01");
        assert_eq!(from_unix_ms(-1.0).to_string(), "1970-01-01");
        // 2026-11-02T23:59:59Z, 2024-02-29T12:00:00Z.
        assert_eq!(from_unix_ms(1_793_663_999_000.0).to_string(), "2026-11-02");
        assert_eq!(from_unix_ms(1_709_208_000_000.0).to_string(), "2024-02-29");
    }

    #[test]
    fn days_back() {
        for days in [0, 59, 10_957, 19_782, 20_759, 2_932_896] {
            let date = Date::from_unix_seconds(days * 86_400).unwrap();
            assert_eq!(unix_days(date), days, "{date}");
        }
        let day: Date = "2026-03-01".parse().unwrap();
        assert_eq!(days_before(day, 1).to_string(), "2026-02-28");
        assert_eq!(days_before(day, 365).to_string(), "2025-03-01");
        assert_eq!(days_before(day, 100_000).to_string(), "1970-01-01");
    }
}
