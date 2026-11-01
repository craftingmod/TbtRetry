#![no_std]

extern crate alloc;

use alloc::{format, string::String};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detection {
    Pass,
    Fail,
    Unknown,
}

#[derive(Default, Debug)]
pub struct SsdtIds {
    pub dtbt: bool,
    pub tbt: bool,
    pub rvp: bool,
}

impl SsdtIds {
    pub fn observe(&mut self, table: &[u8]) -> Result<(), &'static str> {
        validate_sdt(table)?;
        if &table[..4] == b"SSDT" {
            match &table[16..24] {
                b"DTbtSsdt" => self.dtbt = true,
                b"AdlS_Tbt" => self.tbt = true,
                b"AdlS_Rvp" => self.rvp = true,
                _ => {}
            }
        }
        Ok(())
    }

    pub fn detection(&self) -> Detection {
        match (self.dtbt, self.tbt, self.rvp) {
            (true, true, false) => Detection::Pass,
            (false, false, true) => Detection::Fail,
            _ => Detection::Unknown,
        }
    }
}

pub fn checksum_ok(bytes: &[u8]) -> bool {
    bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)) == 0
}

pub fn sdt_length(header: &[u8]) -> Result<usize, &'static str> {
    let bytes = header.get(4..8).ok_or("Truncated ACPI header")?;
    let len = u32::from_le_bytes(bytes.try_into().unwrap()) as usize;
    if !(36..=1024 * 1024).contains(&len) {
        return Err("Invalid ACPI table length (36 bytes .. 1 MiB required)");
    }
    Ok(len)
}

pub fn validate_sdt(table: &[u8]) -> Result<(), &'static str> {
    if sdt_length(table)? != table.len() || !checksum_ok(table) {
        return Err("Invalid ACPI table length/checksum");
    }
    Ok(())
}

/// Returns the XSDT (8-byte entries) or legacy RSDT (4-byte entries) address.
pub fn root_address(rsdp: &[u8]) -> Result<(u64, usize), &'static str> {
    if rsdp.len() < 20 || &rsdp[..8] != b"RSD PTR " || !checksum_ok(&rsdp[..20]) {
        return Err("Invalid RSDP");
    }
    if rsdp[15] >= 2 {
        if rsdp.len() < 36 {
            return Err("Truncated ACPI 2 RSDP");
        }
        let len = u32::from_le_bytes(rsdp[20..24].try_into().unwrap()) as usize;
        if !(36..=4096).contains(&len) || len != rsdp.len() || !checksum_ok(rsdp) {
            return Err("Invalid extended RSDP length/checksum");
        }
        let xsdt = u64::from_le_bytes(rsdp[24..32].try_into().unwrap());
        if xsdt != 0 {
            return Ok((xsdt, 8));
        }
    }
    let rsdt = u32::from_le_bytes(rsdp[16..20].try_into().unwrap());
    if rsdt == 0 {
        return Err("Missing XSDT/RSDT");
    }
    Ok((u64::from(rsdt), 4))
}

pub fn root_entries(table: &[u8], width: usize) -> Result<&[u8], &'static str> {
    validate_sdt(table)?;
    let signature: &[u8] = match width {
        8 => b"XSDT",
        4 => b"RSDT",
        _ => return Err("Invalid ACPI entry width"),
    };
    if &table[..4] != signature || !(table.len() - 36).is_multiple_of(width) {
        return Err("Invalid XSDT/RSDT signature or entries");
    }
    Ok(&table[36..])
}

pub fn parse_count(bytes: &[u8]) -> Result<u8, &'static str> {
    let error = "Invalid tbt-retry.count: expected 1..3 ASCII digits, value 0..255";
    if !(1..=3).contains(&bytes.len()) || !bytes.iter().all(u8::is_ascii_digit) {
        return Err(error);
    }
    core::str::from_utf8(bytes)
        .map_err(|_| error)?
        .parse()
        .map_err(|_| error)
}

pub fn success_attempt_log_line(
    debug: bool,
    detection: Detection,
    previous_failures: Option<u8>,
) -> Option<String> {
    if debug && detection == Detection::Pass {
        previous_failures.map(|count| format!("{}\n", u16::from(count) + 1))
    } else {
        None
    }
}

pub fn configured_loader(text: &str) -> Result<&str, &'static str> {
    let name = text.trim();
    if name.len() <= 4
        || !name
            .get(name.len().saturating_sub(4)..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".efi"))
        || name
            .chars()
            .any(|c| c.is_control() || c == '\u{feff}' || "\\/:*?\"<>|".contains(c))
    {
        return Err("boot_name must be an EFI filename, without a path");
    }
    Ok(name)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "String")]
pub enum RebootType {
    Cold,
    #[default]
    Warm,
}

impl TryFrom<String> for RebootType {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.eq_ignore_ascii_case("COLD") {
            Ok(Self::Cold)
        } else if value.eq_ignore_ascii_case("WARM") {
            Ok(Self::Warm)
        } else {
            Err("reboot_type must be COLD or WARM (case insensitive)")
        }
    }
}

#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub boot_name: String,
    pub debug_reboot_delay: bool,
    pub reboot_type: RebootType,
    pub max_reboots: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            boot_name: String::new(),
            debug_reboot_delay: false,
            reboot_type: RebootType::default(),
            max_reboots: 4,
        }
    }
}

pub const DEFAULT_CONFIG: &str = include_str!("../tbt-retry.toml.example");

pub fn parse_config(text: &str) -> Result<Config, String> {
    if text.starts_with('\u{feff}') {
        return Err("Config must be UTF-8 without BOM".into());
    }
    let mut config: Config =
        toml::from_str(text).map_err(|e| format!("Invalid tbt-retry.toml: {e}"))?;
    if config.boot_name.trim().is_empty() {
        config.boot_name.clear();
    } else {
        config.boot_name = configured_loader(&config.boot_name)?.into();
    }
    Ok(config)
}

pub fn menu_selection(input: &str, count: usize) -> Result<usize, &'static str> {
    if input.is_empty() || !input.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("Enter a number from the list");
    }
    let number = input.parse::<usize>().map_err(|_| "Number is too large")?;
    if number == 0 || number > count {
        return Err("Number is outside the list");
    }
    Ok(number - 1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Boot,
    Reboot(u8),
    Exhausted,
}

pub fn action(detection: Detection, previous_failures: u8, max_reboots: u8) -> Action {
    if detection != Detection::Fail {
        Action::Boot
    } else if previous_failures >= max_reboots {
        Action::Exhausted
    } else {
        Action::Reboot(previous_failures + 1)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{vec, vec::Vec};

    fn seal(bytes: &mut [u8], offset: usize) {
        bytes[offset] = 0;
        bytes[offset] = 0u8.wrapping_sub(bytes.iter().fold(0u8, |a, b| a.wrapping_add(*b)));
    }

    fn table(signature: &[u8; 4], id: &[u8; 8]) -> Vec<u8> {
        let mut bytes = vec![0; 36];
        bytes[..4].copy_from_slice(signature);
        bytes[4..8].copy_from_slice(&36u32.to_le_bytes());
        bytes[16..24].copy_from_slice(id);
        seal(&mut bytes, 9);
        bytes
    }

    #[test]
    fn all_eight_detection_combinations() {
        for bits in 0..8 {
            let mut ids = SsdtIds::default();
            for (i, id) in [b"DTbtSsdt", b"AdlS_Tbt", b"AdlS_Rvp"].iter().enumerate() {
                if bits & (1 << i) != 0 {
                    ids.observe(&table(b"SSDT", id)).unwrap();
                }
            }
            let expected = match bits {
                3 => Detection::Pass,
                4 => Detection::Fail,
                _ => Detection::Unknown,
            };
            assert_eq!(ids.detection(), expected);
        }
    }

    #[test]
    fn only_valid_ssdt_header_ids_count() {
        let mut ids = SsdtIds::default();
        ids.observe(&table(b"DSDT", b"DTbtSsdt")).unwrap();
        let mut payload = table(b"SSDT", b"OtherId ");
        payload.extend_from_slice(b"DTbtSsdtAdlS_TbtAdlS_Rvp");
        let len = payload.len() as u32;
        payload[4..8].copy_from_slice(&len.to_le_bytes());
        seal(&mut payload, 9);
        ids.observe(&payload).unwrap();
        assert_eq!(ids.detection(), Detection::Unknown);
        payload[9] ^= 1;
        assert!(ids.observe(&payload).is_err());
        assert!(ids.observe(&[0; 12]).is_err());
    }

    #[test]
    fn reboot_limit_then_handoff() {
        for limit in 0..=255 {
            for count in 0..=255 {
                let expected = if count < limit {
                    Action::Reboot(count + 1)
                } else {
                    Action::Exhausted
                };
                assert_eq!(action(Detection::Fail, count, limit), expected);
                assert_eq!(action(Detection::Pass, count, limit), Action::Boot);
                assert_eq!(action(Detection::Unknown, count, limit), Action::Boot);
            }
        }
    }

    #[test]
    fn config_and_counter_validation() {
        assert_eq!(configured_loader("부트.efi"), Ok("부트.efi"));
        assert_eq!(configured_loader(" OpenCore.EFI\r\n"), Ok("OpenCore.EFI"));
        for text in [
            "",
            "../boot.efi",
            "\\boot.efi",
            "a.efi\nb.efi",
            "a.txt",
            ".efi",
            "a\0.efi",
            "가나다",
            "\u{feff}OpenCore.efi",
        ] {
            assert!(configured_loader(text).is_err(), "{text}");
        }
        for bytes in [b"".as_slice(), b"256", b"-1", b"+1", b"0\n", b"0000", b"a"] {
            assert!(parse_count(bytes).is_err());
        }
        for n in 0..=255u8 {
            assert_eq!(parse_count(format!("{n}").as_bytes()), Ok(n));
            assert_eq!(parse_count(format!("{n:03}").as_bytes()), Ok(n));
        }
    }

    #[test]
    fn toml_config_and_auto_selection() {
        assert_eq!(parse_config(""), Ok(Config::default()));
        assert_eq!(
            parse_config("boot_name = \"OpenCore.efi\"\r\n").unwrap(),
            Config {
                boot_name: "OpenCore.efi".into(),
                debug_reboot_delay: false,
                reboot_type: RebootType::Warm,
                max_reboots: 4,
            }
        );
        assert_eq!(
            parse_config("# Debug boot\r\nboot_name = 'OpenCore.efi' # next EFI\r\n\r\ndebug_reboot_delay = true\r\n").unwrap(),
            Config {
                boot_name: "OpenCore.efi".into(),
                debug_reboot_delay: true,
                reboot_type: RebootType::Warm,
                max_reboots: 4,
            }
        );
        assert_eq!(
            parse_config("debug_reboot_delay=true").unwrap(),
            Config {
                boot_name: String::new(),
                debug_reboot_delay: true,
                reboot_type: RebootType::Warm,
                max_reboots: 4,
            }
        );
        assert_eq!(
            parse_config("debug_reboot_delay=false"),
            Ok(Config::default())
        );
        assert_eq!(parse_config("boot_name = ''"), Ok(Config::default()));
        assert_eq!(parse_config("boot_name = '  '"), Ok(Config::default()));
        for text in [
            "debug_reboot_delay=1",
            "debug_reboot_delay=",
            "debug=true",
            "debug_reboot_delay=true\ndebug_reboot_delay=false",
            "a.efi\nb.efi",
            "../a.efi\ndebug_reboot_delay=true",
            "\u{feff}debug_reboot_delay=true",
            "OpenCore.efi",
            "boot_name = OpenCore.efi",
            "boot_name = 123",
            "boot_name = '../OpenCore.efi'",
            "boot_name = 'a.efi'\nboot_name = 'b.efi'",
            "debug_reboot_delay = 'true'",
            "[boot]\nboot_name = 'OpenCore.efi'",
        ] {
            assert!(parse_config(text).is_err(), "{text}");
        }
        assert_eq!(
            parse_config("boot_name = \"부트\\u0023.efi\"")
                .unwrap()
                .boot_name,
            "부트#.efi"
        );
        assert_eq!(
            parse_config(include_str!("../tbt-retry.toml.example"))
                .unwrap()
                .boot_name,
            ""
        );
    }

    #[test]
    fn success_attempt_log_counts_initial_boot_and_requires_valid_pass() {
        for count in 0..=255u8 {
            assert_eq!(
                success_attempt_log_line(true, Detection::Pass, Some(count)),
                Some(format!("{}\n", u16::from(count) + 1))
            );
            assert_eq!(
                success_attempt_log_line(false, Detection::Pass, Some(count)),
                None
            );
            assert_eq!(
                success_attempt_log_line(true, Detection::Fail, Some(count)),
                None
            );
            assert_eq!(
                success_attempt_log_line(true, Detection::Unknown, Some(count)),
                None
            );
        }
        assert_eq!(success_attempt_log_line(true, Detection::Pass, None), None);
        assert_eq!(
            success_attempt_log_line(true, Detection::Pass, Some(0)).as_deref(),
            Some("1\n")
        );
        assert_eq!(
            success_attempt_log_line(true, Detection::Pass, Some(255)).as_deref(),
            Some("256\n")
        );
    }

    #[test]
    fn max_reboots_config_validation() {
        assert_eq!(parse_config("").unwrap().max_reboots, 4);
        for limit in [0, 1, 4, 10, 255] {
            let text = format!("max_reboots = {limit}");
            assert_eq!(parse_config(&text).unwrap().max_reboots, limit);
        }
        for text in [
            "max_reboots = -1",
            "max_reboots = 256",
            "max_reboots = 1.5",
            "max_reboots = '4'",
            "max_reboots = true",
            "max_reboots = 1\nmax_reboots = 2",
        ] {
            assert!(parse_config(text).is_err(), "{text}");
        }
    }

    #[test]
    fn reboot_type_config_validation() {
        assert_eq!(parse_config("").unwrap().reboot_type, RebootType::Warm);
        for (value, expected) in [
            ("COLD", RebootType::Cold),
            ("cold", RebootType::Cold),
            ("CoLd", RebootType::Cold),
            ("WARM", RebootType::Warm),
            ("warm", RebootType::Warm),
            ("WaRm", RebootType::Warm),
        ] {
            let text = format!("reboot_type = '{value}'\ndebug_reboot_delay = true");
            let config = parse_config(&text).unwrap();
            assert_eq!(config.reboot_type, expected);
            assert!(config.debug_reboot_delay);
        }
        for text in [
            "reboot_type = 'COLD_REBOOT'",
            "reboot_type = 'WARM_REBOOT'",
            "reboot_type = 'WARM REBOOT'",
            "reboot_type = ''",
            "reboot_type = true",
            "reboot_type = 1",
            "reboot_type = 'COLD'\nreboot_type = 'WARM'",
        ] {
            assert!(parse_config(text).is_err(), "{text}");
        }
    }

    #[test]
    fn numbered_menu_validates_range_and_multiple_digits() {
        assert_eq!(menu_selection("1", 1), Ok(0));
        assert_eq!(menu_selection("12", 12), Ok(11));
        assert_eq!(menu_selection("10", 12), Ok(9));
        for input in [
            "",
            "0",
            "13",
            "-1",
            "+1",
            "1a",
            "１",
            " 1",
            "99999999999999999999999999999",
        ] {
            assert!(menu_selection(input, 12).is_err(), "{input}");
        }
        assert!(menu_selection("1", 0).is_err());
    }

    #[test]
    fn generated_config_matches_runtime_defaults() {
        assert!(DEFAULT_CONFIG.len() <= 1024);
        assert_eq!(parse_config(DEFAULT_CONFIG), Ok(Config::default()));
    }

    #[test]
    fn rsdp_and_root_validation() {
        let mut rsdp = [0; 36];
        rsdp[..8].copy_from_slice(b"RSD PTR ");
        rsdp[15] = 2;
        rsdp[16..20].copy_from_slice(&0x1000u32.to_le_bytes());
        rsdp[20..24].copy_from_slice(&36u32.to_le_bytes());
        rsdp[24..32].copy_from_slice(&0x2000u64.to_le_bytes());
        seal(&mut rsdp[..20], 8);
        seal(&mut rsdp, 32);
        assert_eq!(root_address(&rsdp), Ok((0x2000, 8)));
        rsdp[24..32].fill(0);
        seal(&mut rsdp, 32);
        assert_eq!(root_address(&rsdp), Ok((0x1000, 4)));
        rsdp[15] = 0;
        seal(&mut rsdp[..20], 8);
        assert_eq!(root_address(&rsdp[..20]), Ok((0x1000, 4)));
        rsdp[8] ^= 1;
        assert!(root_address(&rsdp).is_err());
        assert!(root_address(&[]).is_err());
        assert_eq!(root_entries(&table(b"XSDT", b"        "), 8), Ok(&[][..]));
        assert!(root_entries(&table(b"RSDT", b"        "), 8).is_err());
        assert!(root_entries(&table(b"XSDT", b"        "), 0).is_err());
    }
}
