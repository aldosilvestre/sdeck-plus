//! Reads the two facts the server needs back from its own stored certificate:
//! its validity window and its subject alternative names.
//!
//! A deliberately small DER walker: it only follows the fixed X.509 layout of
//! the certificates `certificate.rs` writes, and any surprise means "not
//! usable", which makes the caller generate a fresh certificate.
use std::{
    collections::BTreeSet,
    net::{Ipv4Addr, Ipv6Addr},
};

const SEQUENCE: u8 = 0x30;
const INTEGER: u8 = 0x02;
const OID: u8 = 0x06;
const BOOLEAN: u8 = 0x01;
const OCTET_STRING: u8 = 0x04;
const UTC_TIME: u8 = 0x17;
const GENERALIZED_TIME: u8 = 0x18;
const VERSION: u8 = 0xA0;
const EXTENSIONS: u8 = 0xA3;
const DNS_NAME: u8 = 0x82;
const IP_ADDRESS: u8 = 0x87;
/// 2.5.29.17, subjectAltName.
const SUBJECT_ALT_NAME: &[u8] = &[0x55, 0x1D, 0x11];

#[derive(Debug, PartialEq, Eq)]
pub(super) struct CertificateFacts {
    /// Seconds since the Unix epoch.
    pub not_before: i64,
    pub not_after: i64,
    /// DNS names in lower case and IP addresses in their usual text form.
    pub names: BTreeSet<String>,
}

pub(super) fn inspect(der: &[u8]) -> Option<CertificateFacts> {
    let mut input = der;
    let mut certificate = expect(&mut input, SEQUENCE)?;
    let mut tbs = expect(&mut certificate, SEQUENCE)?;
    if peek(tbs) == Some(VERSION) {
        next(&mut tbs)?;
    }
    expect(&mut tbs, INTEGER)?; // serial number
    expect(&mut tbs, SEQUENCE)?; // signature algorithm
    expect(&mut tbs, SEQUENCE)?; // issuer
    let mut validity = expect(&mut tbs, SEQUENCE)?;
    let not_before = time(&mut validity)?;
    let not_after = time(&mut validity)?;
    expect(&mut tbs, SEQUENCE)?; // subject
    expect(&mut tbs, SEQUENCE)?; // subject public key info
    let mut names = BTreeSet::new();
    while !tbs.is_empty() {
        let (tag, value) = next(&mut tbs)?;
        if tag == EXTENSIONS {
            names = subject_alt_names(value)?;
        }
    }
    Some(CertificateFacts {
        not_before,
        not_after,
        names,
    })
}

fn subject_alt_names(mut explicit: &[u8]) -> Option<BTreeSet<String>> {
    let mut extensions = expect(&mut explicit, SEQUENCE)?;
    while !extensions.is_empty() {
        let mut extension = expect(&mut extensions, SEQUENCE)?;
        let id = expect(&mut extension, OID)?;
        if peek(extension) == Some(BOOLEAN) {
            next(&mut extension)?;
        }
        let mut value = expect(&mut extension, OCTET_STRING)?;
        if id != SUBJECT_ALT_NAME {
            continue;
        }
        let mut general_names = expect(&mut value, SEQUENCE)?;
        let mut names = BTreeSet::new();
        while !general_names.is_empty() {
            let (tag, name) = next(&mut general_names)?;
            match (tag, name.len()) {
                (DNS_NAME, _) => {
                    names.insert(std::str::from_utf8(name).ok()?.to_ascii_lowercase());
                }
                (IP_ADDRESS, 4) => {
                    let octets: [u8; 4] = name.try_into().ok()?;
                    names.insert(Ipv4Addr::from(octets).to_string());
                }
                (IP_ADDRESS, 16) => {
                    let octets: [u8; 16] = name.try_into().ok()?;
                    names.insert(Ipv6Addr::from(octets).to_string());
                }
                // Other kinds of name are never written by this app.
                _ => return None,
            }
        }
        return Some(names);
    }
    Some(BTreeSet::new())
}

fn time(input: &mut &[u8]) -> Option<i64> {
    let (tag, value) = next(input)?;
    let text = std::str::from_utf8(value).ok()?;
    let digits = text.strip_suffix('Z')?;
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let (year, rest) = match (tag, digits.len()) {
        (UTC_TIME, 12) => {
            let short: i64 = digits[..2].parse().ok()?;
            (
                if short < 50 {
                    2000 + short
                } else {
                    1900 + short
                },
                &digits[2..],
            )
        }
        (GENERALIZED_TIME, 14) => (digits[..4].parse().ok()?, &digits[4..]),
        _ => return None,
    };
    let field = |at: usize| -> Option<i64> { rest.get(at..at + 2)?.parse().ok() };
    let (month, day) = (field(0)?, field(2)?);
    let (hour, minute, second) = (field(4)?, field(6)?, field(8)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    let days = days_from_civil(year, month as u32, day as u32);
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

fn peek(input: &[u8]) -> Option<u8> {
    input.first().copied()
}

fn expect<'a>(input: &mut &'a [u8], tag: u8) -> Option<&'a [u8]> {
    let (found, value) = next(input)?;
    (found == tag).then_some(value)
}

/// One tag-length-value; only single-byte tags and definite lengths (DER).
fn next<'a>(input: &mut &'a [u8]) -> Option<(u8, &'a [u8])> {
    let (&tag, rest) = input.split_first()?;
    if tag & 0x1F == 0x1F {
        return None;
    }
    let (&first, mut rest) = rest.split_first()?;
    let length = if first < 0x80 {
        usize::from(first)
    } else {
        let count = usize::from(first & 0x7F);
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let (bytes, after) = rest.split_at(count);
        rest = after;
        bytes
            .iter()
            .fold(0usize, |length, &byte| (length << 8) | usize::from(byte))
    };
    if rest.len() < length {
        return None;
    }
    let (value, after) = rest.split_at(length);
    *input = after;
    Some((tag, value))
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
pub(super) fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year =
        (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The date of a day number since 1970-01-01: (year, month 1-12, day 1-31).
pub(super) fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month as u8, day as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_back_names_and_validity_of_an_rcgen_certificate() {
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let mut params = rcgen::CertificateParams::new(vec![
            "LocalHost".to_string(),
            "127.0.0.1".to_string(),
            "192.168.1.20".to_string(),
            "::1".to_string(),
        ])
        .unwrap();
        params.not_before = rcgen::date_time_ymd(2026, 11, 20);
        params.not_after = rcgen::date_time_ymd(2051, 2, 28);
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        let certificate = params.self_signed(&key).unwrap();

        let facts = inspect(certificate.der()).unwrap();
        let names: Vec<&str> = facts.names.iter().map(String::as_str).collect();
        assert_eq!(names, ["127.0.0.1", "192.168.1.20", "::1", "localhost"]);
        assert_eq!(facts.not_before, days_from_civil(2026, 11, 20) * 86_400);
        assert_eq!(facts.not_after, days_from_civil(2051, 2, 28) * 86_400);
    }

    #[test]
    fn rejects_anything_that_is_not_a_certificate() {
        assert_eq!(inspect(&[]), None);
        assert_eq!(inspect(&[0x30, 0x82, 0xFF]), None);
        assert_eq!(inspect(b"-----BEGIN CERTIFICATE-----"), None);
    }

    #[test]
    fn civil_dates_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        for days in [-1, 0, 59, 11_016, 11_017, 20_000, 29_586, 2_932_896] {
            let (year, month, day) = civil_from_days(days);
            assert_eq!(
                days_from_civil(year, u32::from(month), u32::from(day)),
                days
            );
        }
        assert_eq!(civil_from_days(20_412), (2025, 11, 20));
    }
}
