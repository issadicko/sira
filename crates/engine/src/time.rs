//! Dates UTC sans dépendance : conversion entre jours depuis l'époque Unix et date civile (algorithmes de Howard Hinnant).

/// Jour civil (année, mois, jour) à `days` jours du 1er janvier 1970.
pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// Nombre de jours entre le 1er janvier 1970 et le jour civil donné ; l'inverse de [`civil`].
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from((month + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Millisecondes depuis l'époque Unix pour une date et une heure UTC.
pub fn millis(year: i64, month: u32, day: u32, hour: u64, minute: u64, second: u64) -> u64 {
    let days = u64::try_from(days_from_civil(year, month, day)).unwrap_or(0);
    ((days * 24 + hour) * 60 + minute) * 60_000 + second * 1000
}

/// `2026-10-06T12:34:56.789Z` pour un instant exprimé en millisecondes depuis l'époque Unix.
pub fn iso_from_millis(millis: u64) -> String {
    let (days, rest) = ((millis / 86_400_000) as i64, millis % 86_400_000);
    let (year, month, day) = civil(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3_600_000,
        rest / 60_000 % 60,
        rest / 1000 % 60,
        rest % 1000
    )
}

/// `20260306T123456Z`, le format des dates de signature AWS.
pub fn amz_date(millis: u64) -> String {
    let (days, rest) = ((millis / 86_400_000) as i64, millis % 86_400_000);
    let (year, month, day) = civil(days);
    format!("{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z", rest / 3_600_000, rest / 60_000 % 60, rest / 1000 % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip_through_days() {
        for days in [-1, 0, 59, 60, 365, 11_016, 19_000, 20_000, 47_482] {
            let (y, m, d) = civil(days);
            assert_eq!(days_from_civil(y, m, d), days, "{y}-{m}-{d}");
        }
    }

    #[test]
    fn instants_format_as_iso_and_as_amz() {
        assert_eq!(iso_from_millis(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_from_millis(1_700_000_000_123), "2023-11-14T22:13:20.123Z");
        assert_eq!(iso_from_millis(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(amz_date(millis(2015, 8, 30, 12, 36, 0)), "20150830T123600Z");
        assert_eq!(millis(2023, 11, 14, 22, 13, 20), 1_700_000_000_000);
    }
}
