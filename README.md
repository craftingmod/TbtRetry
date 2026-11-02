# TbtRetry

[English](./README.md) | [한국어](./README.KO.md)

![Gacha](./docs/gacha.webp)
> Above  image is from Blue archive, copyright by Nexon Games.

**A UEFI-based workaround for intermittent Thunderbolt initialization failures.**

> If Thunderbolt fails to initialize, reboot until it does.

TbtRetry is a lightweight UEFI application designed to work around intermittent Thunderbolt initialization failures on certain motherboards.

Instead of attempting to repair a failed PCIe link, TbtRetry checks the ACPI tables installed by the firmware to determine whether Thunderbolt initialization succeeded.

If initialization fails, the application automatically reboots the system and tries again.

Once initialization succeeds, it can continue to the next EFI application or operating system bootloader.

## Background

This project originated from an intermittent Thunderbolt initialization issue observed with the following hardware:

| Component | Configuration |
|---|---|
| Motherboard | ASUS ROG STRIX Z690-A GAMING WIFI D4 |
| Thunderbolt Controller | ASUS ThunderboltEX 4 |
| BIOS Version | 3101 |
| Thunderbolt NVM | 36 |

The issue exhibits several unusual characteristics:

- Thunderbolt initialization succeeds or fails unpredictably between boots.
- The same hardware and BIOS configuration can produce different results.
- When initialization fails, the Thunderbolt controller is missing from PCIe enumeration.
- PCIe link retraining and Link Disable toggling do not recover the device.
- Neither cold boots nor warm reboots consistently guarantee successful initialization.
- Repeated warm reboots can eventually result in successful initialization.

### ACPI Investigation

Comparing ACPI table dumps from successful and failed boots revealed a consistent difference in the Thunderbolt-related SSDTs.

| ACPI Table | Successful Boot | Failed Boot |
|---|---|---|
| `DTbtSsdt` | Present | Missing |
| `AdlS_Tbt` | Present | Missing |
| `AdlS_Rvp` | Missing | Present |
| `TbtTypeC` | Present | Present |

PCIe link status also differed significantly:

| Property | Successful Boot | Failed Boot |
|---|---|---|
| Link Speed | 8 GT/s | 2.5 GT/s |
| Link Width | x4 | x0 |
| Link Training | Completed | In Progress |
| Data Link Active | Yes | No |

These observations indicate that the firmware follows different Thunderbolt initialization paths depending on the outcome of an earlier initialization stage.

Although the exact underlying cause has not been conclusively identified, the presence of specific ACPI tables provides a practical indicator of successful initialization on the tested system.

**Important:** These ACPI differences are indicators of initialization state, not proof that ACPI table generation itself causes the failure.

## How It Works

```text
             UEFI Firmware
                   |
                   v
              TbtRetry.efi
                   |
                   v
            Scan ACPI Tables
                   |
                   v
         Thunderbolt Initialized?
                   |
             +-----+-----+
             |           |
            YES          NO
             |           |
             v           v
       Chainload EFI   Reboot
             |           |
             v           |
       Windows/Linux <---+
       (after success)
```

TbtRetry uses a simple strategy:

1. Execute before the operating system starts.
2. Enumerate the ACPI tables installed by the firmware.
3. Check for the expected Thunderbolt initialization tables.
4. If initialization appears successful, continue the boot process.
5. Otherwise, request a UEFI warm reboot.
6. Repeat until initialization succeeds or the configured retry limit is reached.

TbtRetry does not modify the BIOS, Thunderbolt NVM firmware, or PCIe configuration.

It simply gives the firmware another opportunity to initialize the hardware correctly.

## Build

Install Rust 1.91 or later (including Cargo and rustup) and Bun, then run the following from the repository root:

```sh
bun run setup
bun run build
```

`setup` installs the `x86_64-unknown-uefi` target. `build` compiles the release binary using `Cargo.lock`, checks that it is an x86_64 EFI application, and creates:

- `dist/EFI/BOOT/BOOTX64.EFI`: the UEFI application.
- `dist/tbt-retry.toml`: a copy of `tbt-retry.toml.example`.

Each build overwrites these files. Edit the generated configuration after building, or keep your custom configuration separately.

To run the library tests:

```sh
bun run test
```

## Standalone EFI

If no other EFI exists in `\EFI\BOOT\` on the same partition, TbtRetry performs detection and retries regardless of `boot_name`, then returns `EFI_ABORTED` so firmware can try the next boot option. Retry count cleanup and debug success logging still run. If another EFI exists, chainloading works as before.

For standalone use, set the firmware boot order to **TbtRetry → OS bootloader**. This assumes TbtRetry is launched directly as a firmware boot option.

Set `use_boot_order = true` in `tbt-retry.toml` to skip EFI discovery, selection, and chainloading even when other EFI files exist. It defaults to `false`, including for existing configurations without this option. It overrides `boot_name` selection; configuration syntax validation still applies.

Both paths first perform reboot retries on initialization failure. Handoff happens after success or the retry limit; unknown detection or counter errors continue booting as before. `EFI_ABORTED` is returned only at the final handoff to request the next boot option, not to skip retries.

## Compatibility

Currently, the detection method is based on observations from the ASUS ROG STRIX Z690-A GAMING WIFI D4 running BIOS 3101 with a ThunderboltEX 4 expansion card.

Other motherboards, firmware versions, and Thunderbolt controllers may expose different ACPI tables or use different initialization paths.

Compatibility beyond the tested configuration has not been established.

## Limitations

- **Not a firmware fix:** TbtRetry does not address the underlying initialization defect.
- **Probabilistic recovery:** Success depends on the firmware producing a different initialization outcome on a subsequent boot.
- **Hardware-specific detection:** The current ACPI signatures are derived from one tested hardware configuration.
- **No guaranteed recovery:** Some failures may persist across all reboot attempts.
- **Limited validation:** ACPI table presence does not guarantee that every Thunderbolt or USB function is operational.

## Why Does This Exist?

After investigating PCIe enumeration, root port link states, firmware settings, ACPI table differences, and BIOS initialization behavior, one surprisingly effective workaround remained:

**Reboot until the firmware initializes Thunderbolt correctly.**

While this approach may seem unconventional, it avoids invasive firmware modifications and can recover from failures that cannot be corrected after initialization has already failed.

The key is knowing when a reboot is necessary.

TbtRetry turns an unpredictable firmware initialization outcome into a detectable condition with an automated retry mechanism.

---

**A deterministic check for a nondeterministic firmware initialization failure.**

*Experimental project. Not affiliated with or endorsed by ASUS or Intel.*
