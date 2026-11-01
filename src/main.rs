#![no_std]
#![no_main]

extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::{slice, time::Duration};
use tbt_retry::{
    Action, DEFAULT_CONFIG, Detection, RebootType, SsdtIds, action, configured_loader,
    menu_selection, parse_config, parse_count, resolve_loader_candidates, root_address,
    root_entries, sdt_length, success_attempt_log_line,
};
use uefi::proto::BootPolicy;
use uefi::proto::{
    console::text::{Key, ScanCode},
    device_path::{
        DevicePath,
        build::{self, DevicePathBuilder},
        media::FilePath,
    },
    loaded_image::LoadedImage,
    media::file::{Directory, File, FileAttribute, FileMode, RegularFile},
};
use uefi::table::cfg::ConfigTableEntry;
use uefi::{
    CString16,
    boot::{self, LoadImageSource},
    prelude::*,
    print, println,
};

const DIR: &str = "\\EFI\\BOOT\\";
const CONFIG: &str = "\\tbt-retry.toml";
const COUNT: &str = "\\EFI\\BOOT\\tbt-retry.count";
const ATTEMPT_LOG: &str = "\\EFI\\BOOT\\tbt-attemp.log";
type AppResult<T> = Result<T, String>;

fn ucs2(text: &str) -> AppResult<CString16> {
    CString16::try_from(text).map_err(|e| format!("Invalid UEFI filename: {e:?}"))
}

fn disk_path(name: &str) -> AppResult<CString16> {
    ucs2(&format!("{DIR}{name}"))
}

fn read_optional(root: &mut Directory, name: &str, limit: usize) -> AppResult<Option<Vec<u8>>> {
    let handle = match root.open(&ucs2(name)?, FileMode::Read, FileAttribute::empty()) {
        Ok(file) => file,
        Err(e) if e.status() == Status::NOT_FOUND => return Ok(None),
        Err(e) => return Err(format!("Read {name}: {e:?}")),
    };
    let mut file = handle
        .into_regular_file()
        .ok_or_else(|| format!("{name} is not a file"))?;
    let mut bytes = alloc::vec![0; limit + 1];
    let mut read = 0;
    loop {
        let n = file
            .read(&mut bytes[read..])
            .map_err(|e| format!("Read {name}: {e:?}"))?;
        read += n;
        if read > limit {
            return Err(format!("{name} exceeds {limit} bytes"));
        }
        if n == 0 {
            break;
        }
    }
    bytes.truncate(read);
    Ok(Some(bytes))
}

fn create_default_config(root: &mut Directory) -> AppResult<()> {
    let mut file = root
        .open(
            &ucs2(CONFIG)?,
            FileMode::CreateReadWrite,
            FileAttribute::empty(),
        )
        .map_err(|e| format!("Create {CONFIG}: {e:?}"))?
        .into_regular_file()
        .ok_or("Config is not a file")?;
    let result = file
        .write(DEFAULT_CONFIG.as_bytes())
        .map_err(|e| format!("Write {CONFIG}: {e:?}"))
        .and_then(|_| file.flush().map_err(|e| format!("Flush {CONFIG}: {e:?}")));
    if let Err(error) = result {
        // Remove an incomplete new file so the next boot can create it again.
        if let Err(cleanup) = file.delete() {
            return Err(format!(
                "{error}; could not remove incomplete config: {cleanup:?}"
            ));
        }
        return Err(error);
    }
    println!("Created {CONFIG} with default settings.");
    Ok(())
}

fn save_count(root: &mut Directory, count: u8) -> AppResult<()> {
    let mut file = root
        .open(
            &ucs2(COUNT)?,
            FileMode::CreateReadWrite,
            FileAttribute::empty(),
        )
        .map_err(|e| format!("Open counter: {e:?}"))?
        .into_regular_file()
        .ok_or("Counter is not a file")?;
    // Fixed width avoids trailing digits when clearing a multi-digit count; keep the existing file.
    let bytes = format!("{count:03}");
    file.set_position(0)
        .map_err(|e| format!("Seek counter: {e:?}"))?;
    file.write(bytes.as_bytes())
        .map_err(|e| format!("Write counter: {e:?}"))?;
    file.flush().map_err(|e| format!("Flush counter: {e:?}"))?;
    drop(file);
    if read_optional(root, COUNT, 3)?.as_deref() != Some(bytes.as_bytes()) {
        return Err("Counter read-back failed".into());
    }
    Ok(())
}

fn append_attempt_log(root: &mut Directory, line: &str) -> AppResult<()> {
    let mut file = root
        .open(
            &ucs2(ATTEMPT_LOG)?,
            FileMode::CreateReadWrite,
            FileAttribute::empty(),
        )
        .map_err(|e| format!("Open attempt log: {e:?}"))?
        .into_regular_file()
        .ok_or("Attempt log is not a file")?;
    file.set_position(RegularFile::END_OF_FILE)
        .map_err(|e| format!("Seek attempt log: {e:?}"))?;
    file.write(line.as_bytes())
        .map_err(|e| format!("Write attempt log: {e:?}"))?;
    file.flush()
        .map_err(|e| format!("Flush attempt log: {e:?}"))
}

fn loader_candidates(
    root: &mut Directory,
    own_name: &str,
    selected: &str,
) -> AppResult<Vec<String>> {
    let mut dir = root
        .open(&ucs2(DIR)?, FileMode::Read, FileAttribute::empty())
        .map_err(|e| format!("Open EFI/BOOT: {e:?}"))?
        .into_directory()
        .ok_or("EFI/BOOT is not a directory")?;
    let mut candidates = Vec::new();
    while let Some(entry) = dir
        .read_entry_boxed()
        .map_err(|e| format!("List EFI/BOOT: {e:?}"))?
    {
        let name = entry.file_name().to_string();
        if !entry.is_directory()
            && configured_loader(&name).is_ok()
            && !name.eq_ignore_ascii_case(own_name)
        {
            candidates.push(name);
        }
    }
    resolve_loader_candidates(candidates, selected)
}

fn select_loader(mut candidates: Vec<String>) -> AppResult<String> {
    if candidates.len() == 1 {
        return Ok(candidates.remove(0));
    }
    println!("Choose the next EFI:");
    for (index, name) in candidates.iter().enumerate() {
        println!("{}. {name}", index + 1);
    }
    let index = prompt_selection(candidates.len())?;
    Ok(candidates.remove(index))
}

fn prompt_selection(count: usize) -> AppResult<usize> {
    let max_digits = usize::MAX.to_string().len();
    'prompt: loop {
        print!("Select 1-{count}, then Enter (Esc cancels): ");
        let mut input = String::new();
        loop {
            let key = uefi::system::with_stdin(|stdin| -> AppResult<_> {
                let event = stdin
                    .wait_for_key_event()
                    .map_err(|e| format!("Keyboard event: {e:?}"))?;
                boot::wait_for_event(&[event]).map_err(|e| format!("Wait for keyboard: {e:?}"))?;
                stdin
                    .read_key()
                    .map_err(|e| format!("Read keyboard: {e:?}"))
            })?;
            match key {
                Some(Key::Printable(c)) => match char::from(c) {
                    '\r' => {
                        println!();
                        break;
                    }
                    '\u{8}' if !input.is_empty() => {
                        input.pop();
                        print!("\u{8} \u{8}");
                    }
                    digit @ '0'..='9' => {
                        if input.len() == max_digits {
                            println!("\nNumber is too long. Try again.");
                            continue 'prompt;
                        }
                        input.push(digit);
                        print!("{digit}");
                    }
                    _ => {}
                },
                Some(Key::Special(ScanCode::ESCAPE)) => {
                    return Err("EFI selection cancelled".into());
                }
                _ => {}
            }
        }
        match menu_selection(&input, count) {
            Ok(index) => return Ok(index),
            Err(error) => println!("{error}. Enter 1-{count}."),
        }
    }
}

fn image_location() -> AppResult<(Handle, String)> {
    let loaded = boot::open_protocol_exclusive::<LoadedImage>(boot::image_handle())
        .map_err(|e| format!("LoadedImage: {e:?}"))?;
    let device = loaded.device().ok_or("No boot device")?;
    let path = loaded.file_path().ok_or("No image file path")?;
    let mut filename = String::new();
    for node in path.node_iter() {
        if let Ok(file) = <&FilePath>::try_from(node) {
            let units = file.path_name().to_vec();
            let text =
                uefi::CStr16::from_u16_with_nul(&units).map_err(|_| "Invalid image file path")?;
            filename.push_str(&text.to_string());
        }
    }
    let (parent, name) = filename
        .rsplit_once('\\')
        .ok_or("Image must be in EFI/BOOT")?;
    if !parent.eq_ignore_ascii_case("\\EFI\\BOOT") || configured_loader(name).is_err() {
        return Err("Run this app from \\EFI\\BOOT\\ (normally BOOTX64.EFI)".into());
    }
    Ok((device, name.to_string()))
}

// SAFETY: Called only for ACPI addresses supplied by firmware, before ExitBootServices.
// Firmware owns readable ACPI tables; length caps/checksums detect malformed contents.
unsafe fn firmware_bytes(address: u64, len: usize) -> AppResult<&'static [u8]> {
    let address = usize::try_from(address).map_err(|_| "ACPI address exceeds address space")?;
    if address == 0 || address.checked_add(len).is_none() || len > isize::MAX as usize {
        return Err("Invalid ACPI address".into());
    }
    Ok(unsafe { slice::from_raw_parts(address as *const u8, len) })
}

unsafe fn firmware_table(address: u64) -> AppResult<&'static [u8]> {
    let header = unsafe { firmware_bytes(address, 36)? };
    let len = sdt_length(header)?;
    unsafe { firmware_bytes(address, len) }
}

fn detect() -> AppResult<Detection> {
    let address = uefi::system::with_config_table(|entries| {
        entries
            .iter()
            .find(|e| e.guid == ConfigTableEntry::ACPI2_GUID)
            .or_else(|| {
                entries
                    .iter()
                    .find(|e| e.guid == ConfigTableEntry::ACPI_GUID)
            })
            .map(|e| e.address as u64)
    })
    .ok_or("No ACPI RSDP in UEFI configuration table")?;
    // SAFETY: Pointers originate from the firmware ACPI configuration table and root entries.
    unsafe {
        let prefix = firmware_bytes(address, 20)?;
        let rsdp = if prefix[15] >= 2 {
            let header = firmware_bytes(address, 36)?;
            let len = u32::from_le_bytes(header[20..24].try_into().unwrap()) as usize;
            if !(36..=4096).contains(&len) {
                return Err("Invalid RSDP length".into());
            }
            firmware_bytes(address, len)?
        } else {
            prefix
        };
        let (root, width) = root_address(rsdp)?;
        let entries = root_entries(firmware_table(root)?, width)?;
        let mut ids = SsdtIds::default();
        for entry in entries.chunks_exact(width) {
            let address = if width == 8 {
                u64::from_le_bytes(entry.try_into().unwrap())
            } else {
                u64::from(u32::from_le_bytes(entry.try_into().unwrap()))
            };
            let table = firmware_table(address)?;
            ids.observe(table)?;
        }
        println!(
            "SSDT IDs: DTbtSsdt={}, AdlS_Tbt={}, AdlS_Rvp={}",
            ids.dtbt, ids.tbt, ids.rvp
        );
        Ok(ids.detection())
    }
}

fn run() -> AppResult<Status> {
    boot::set_watchdog_timer(0, 0, None).map_err(|e| format!("Disable watchdog: {e:?}"))?;
    let (device, own_name) = image_location()?;
    let mut fs = boot::get_image_file_system(boot::image_handle())
        .map_err(|e| format!("Boot filesystem: {e:?}"))?;
    let mut root = fs
        .open_volume()
        .map_err(|e| format!("Open volume: {e:?}"))?;
    let config_bytes = match read_optional(&mut root, CONFIG, 1024)? {
        Some(bytes) => bytes,
        None => {
            if let Err(error) = create_default_config(&mut root) {
                println!("Warning: {error}. Continuing with default settings.");
                boot::stall(Duration::from_secs(10));
            }
            DEFAULT_CONFIG.as_bytes().to_vec()
        }
    };
    let config_text =
        core::str::from_utf8(&config_bytes).map_err(|_| "Config must be UTF-8 without BOM")?;
    let config = parse_config(config_text)?;
    let candidates = if config.use_boot_order {
        Vec::new()
    } else {
        loader_candidates(&mut root, &own_name, &config.boot_name)?
    };
    let detection = detect().unwrap_or_else(|error| {
        println!("UNKNOWN: {error}");
        Detection::Unknown
    });
    println!("Detection: {detection:?}");
    let count = read_optional(&mut root, COUNT, 3)
        .and_then(|bytes| bytes.map_or(Ok(0), |bytes| parse_count(&bytes).map_err(String::from)));
    let mut counter_valid = count.is_ok();
    let attempt_log = success_attempt_log_line(
        config.debug_reboot_delay,
        detection,
        count.as_ref().ok().copied(),
    );
    match count {
        Ok(previous) => match action(detection, previous, config.max_reboots) {
            Action::Reboot(next) => {
                if let Err(error) = save_count(&mut root, next) {
                    counter_valid = false;
                    println!("Cannot persist retry count; continuing boot: {error}");
                    boot::stall(Duration::from_secs(10));
                } else {
                    let (reset_type, label) = match config.reboot_type {
                        RebootType::Cold => (uefi::runtime::ResetType::COLD, "Cold"),
                        RebootType::Warm => (uefi::runtime::ResetType::WARM, "Warm"),
                    };
                    println!(
                        "FAIL. Reboot {next}/{}: {label} reboot...",
                        config.max_reboots
                    );
                    if config.debug_reboot_delay {
                        println!(
                            "Debug: showing current state for 10 seconds before {label} reboot."
                        );
                        boot::stall(Duration::from_secs(10));
                    }
                    drop(root);
                    drop(fs);
                    uefi::runtime::reset(reset_type, Status::SUCCESS, None);
                }
            }
            Action::Exhausted => {
                println!(
                    "Thunderbolt detection failed; reboot limit ({}) reached. Continuing boot in 10 seconds...",
                    config.max_reboots
                );
                boot::stall(Duration::from_secs(10));
            }
            Action::Boot => {}
        },
        Err(error) => {
            println!("Retry disabled; continuing boot: {error}");
            boot::stall(Duration::from_secs(10));
        }
    }

    let next_image = if candidates.is_empty() {
        None
    } else {
        let loader = select_loader(candidates)?;
        println!("Next EFI: {loader}");
        Some((load_loader(device, &loader)?, loader))
    };
    if let Some(line) = attempt_log
        && let Err(error) = append_attempt_log(&mut root, &line)
    {
        println!("Warning: could not append attempt log: {error}");
        boot::stall(Duration::from_secs(10));
    }
    if counter_valid && let Err(error) = save_count(&mut root, 0) {
        println!("Warning: could not clear retry count: {error}");
        boot::stall(Duration::from_secs(10));
    }
    drop(root);
    drop(fs);
    let Some((image, loader)) = next_image else {
        println!("Continuing via firmware boot order (override enabled or no other EFI).");
        // A non-success status tells the firmware to try the next BootOrder entry.
        return Ok(Status::ABORTED);
    };
    println!("Starting {loader}");
    let result = boot::start_image(image);
    let _ = boot::unload_image(image);
    result
        .map(|()| Status::SUCCESS)
        .map_err(|e| format!("Start {loader}: {e:?}"))
}

fn load_loader(device: Handle, loader: &str) -> AppResult<Handle> {
    let target = disk_path(loader)?;
    let device_path = boot::open_protocol_exclusive::<DevicePath>(device)
        .map_err(|e| format!("Boot device path: {e:?}"))?;
    let mut storage = Vec::new();
    let mut builder = DevicePathBuilder::with_vec(&mut storage);
    for node in device_path.node_iter() {
        builder = builder
            .push(&node)
            .map_err(|e| format!("Copy device path: {e:?}"))?;
    }
    let path = builder
        .push(&build::media::FilePath { path_name: &target })
        .and_then(|b| b.finalize())
        .map_err(|e| format!("Build loader path: {e:?}"))?;
    drop(device_path);
    boot::load_image(
        boot::image_handle(),
        LoadImageSource::FromDevicePath {
            device_path: path,
            boot_policy: BootPolicy::ExactMatch,
        },
    )
    .map_err(|e| format!("Load {loader}: {e:?}"))
}

#[entry]
fn main() -> Status {
    if let Err(e) = uefi::helpers::init() {
        return e.status();
    }
    println!("TbtRetry");
    match run() {
        Ok(status) => status,
        Err(error) => {
            println!("TbtRetry error: {error}");
            boot::stall(Duration::from_secs(10));
            Status::ABORTED
        }
    }
}
