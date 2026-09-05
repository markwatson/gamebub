# Flashing and recovering the handheld

Field notes from repairing a rev4 unit after a mis-targeted flash. The short
version: **you cannot brick this device by writing flash**, but you can very
easily write to the wrong place, and almost nothing about the device tells you
so.

## Quick reference

Enter the ROM serial bootloader: **hold Home, press Power**, connect USB-C.

```sh
# From firmware/handheld. The runner in .cargo/config.toml passes these already.
cargo run --release --features=rev4        # match the flag to your board

# Equivalent, by hand:
espflash flash --chip esp32s3 \
  --bootloader bootloader.bin \
  --partition-table partitions.csv \
  --target-app-partition factory \
  target/xtensa-esp32s3-espidf/release/handheld
```

**Never pass `--erase-flash` or `--erase-parts`.** `system_data` is a 2 MB FAT
image that a plain flash leaves alone; erasing it means regenerating it with
`flash_system_data.py`.

## The espflash version trap

`.cargo/config.toml` used to carry `runner = "espflash flash --monitor"` with a
comment reading *"Select this runner for espflash v2.x.x"*. On **espflash 4.x**
that is actively dangerous:

* espflash 4 **ignores the `[idf]` section of `espflash.toml`** — both
  `bootloader` and `partition_table` are silently dropped.
* With no `--target-app-partition`, it generates its own default table
  (`nvs` 0x9000, `phy_init` 0xf000, `factory` **0x10000** sized to the rest of
  flash), writes *that* at 0x8000, writes its own stock bootloader at 0x0, and
  puts the app at **0x10000** — on top of the `dfu` partition.

The tell is in espflash's own output:

```
App/part. size:    1,908,192/8,323,072 bytes, 22.93%
[00:00:17] [====]  922/922  0x10000  Verifying... OK!
```

`8,323,072` is exactly 8 MiB − 0x10000, i.e. "0x10000 to end of flash" — not any
partition in `partitions.csv`. A correct flash reports the real partition size:

```
App/part. size:    1,908,192/5,242,880 bytes, 36.40%   # 5 MB = factory
```

A 1.9 MB app written at 0x10000 runs to 0x1E1DE0, destroying `dfu`
(0x10000–0x40000) and `nvs` (0x80000), and spilling into `factory`. It does not
error, because espflash is not using your table's bounds.

`handheld-dfu/.cargo/config.toml` always did this correctly
(`--target-app-partition dfu`); only the main firmware's runner was stale.

## Flash layout, and what is expensive to lose

| Offset | Partition | Notes |
| --- | --- | --- |
| `0x0` | bootloader | Prebuilt QIO-capable `bootloader.bin`, committed to the repo |
| `0x8000` | partition table | 0xC00 long, so it never reaches `nvs_ro` |
| `0x9000` | `nvs_ro` | Factory data, readonly |
| `0x10000` | `dfu` | DFU bootloader app, 192 KB, type `app` / subtype `test` |
| `0x80000` | `nvs` | Settings. Erasing it just forces the first-boot setup screen |
| `0x100000` | `factory` | Main firmware, 5 MB |
| `0x600000` | `system_data` | 2 MB FAT. **The one genuinely expensive thing here** |

**Serial number and hardware revision live in eFuse, not flash**
(`hwinfo.rs` reads `EFUSE_BLK_USER_DATA`). No flash operation can touch them, so
device identity is never at risk. There is no `flash_nvs.py` in this repo
despite what the old `docs/building.md` said — an assembled board already has
its factory data.

Decode the revision from `Settings > About`: `1.4.1.0` is
`product.major.minor.variant`, so **major 4 → `--features=rev4`**. `main.rs`
compares this against the compiled-in revision and bails on a mismatch, so a
wrong-`--features` build fails loudly over serial. A hand-built UF2 carries no
revision tag and the DFU bootloader's check cannot catch it.

## Recovery: what is actually the safety net

**The ROM serial bootloader.** Holding Home (GPIO0, a strapping pin) at power-on
makes the ESP32-S3's mask ROM enter download mode regardless of what is in
flash. It is silicon; it cannot be erased or corrupted. This is the real
guarantee, and it holds even with a completely destroyed bootloader, partition
table and app.

**DFU (Volume− + Power) is a convenience, not the safety net.** It lives in a
flash partition and can be destroyed like anything else.

If DFU is gone, the official release UF2 can still be installed over serial —
its payload is just two regions:

```sh
# A UF2 is 512-byte blocks: 32-byte header (magic, flags, targetAddr,
# payloadSize, blockNo, numBlocks, familyID) + payload + trailing magic.
# gamebub-rev4_v1.0.1.uf2 contains:
#   0x0100000 .. 0x02C9000   1,871,872 bytes   application
#   0x0600000 .. 0x0800000   2,097,152 bytes   system_data
espflash write-bin 0x100000 app_from_uf2.bin
```

The UF2 contains **nothing below 0x80000** — no bootloader, no partition table,
no DFU app. Those come from the repo (`bootloader.bin`, `partitions.csv`) and
from building `handheld-dfu`.

## Reading the device

Almost every indicator is misleading. In order of usefulness:

**USB device identity** is the single most reliable signal. Check with
`ioreg -p IOUSB -w0 -l | grep "USB Product Name"`:

* `Espressif` / `USB JTAG_serial debug unit` (`303a:1001`) — the **ROM**
  USB-serial-JTAG. The firmware is *not* running.
* Anything else — the firmware reached `main.rs:41` and brought up TinyUSB.

The two share GPIO19/20 and only one can drive the pins, so the identity flips
the moment the app takes over.

**LEDs.**

* **Orange** (D201) is the charge LED, driven by the BQ24073's `CHG` pin from
  `VSYS_ALWAYS` — *upstream of the power switch*. It is lit whenever USB is
  connected, powered on or off. **It tells you nothing about device state**, and
  it will convince you a power-off failed when it succeeded.
* **Purple** (GPIO42) is the status LED. Brightness-only PWM — `LedColor` is a
  duty cycle, there is no colour control.
  * *Smooth breathe, ~400 ms* — `LedBehavior::LOADING`, set early in
    `Device::init()`. Firmware is alive.
  * *Solid* — hung. LEDC holds the last duty when the CPU stops.
  * *Two blinks then a ~1 s pause* — the DFU app (`handheld-dfu/src/led.rs`).
    The dark screen is intentional.

**The console is on UART0, not USB.** `sdkconfig.defaults` never sets
`CONFIG_ESP_CONSOLE_*`, so ROM output, second-stage bootloader output, and any
app output before TinyUSB comes up go to physical pins. Over USB you are blind
until the app reaches `configure_usb`. Silence on USB proves nothing.

## Resets, and why only the power button really works

Everything reachable over USB resets the CPU but **not the RTC domain**:

* `espflash monitor` — connects with a flash stub and parks the chip in download
  mode. **`--no-reset` does this too**; it still prints `Using flash stub`.
* `espflash reset` — `--after hard-reset` toggles DTR on the USB-serial-JTAG.
* Driving DTR/RTS by hand with pyserial — same path, same limitation.

All of them produce `rst:0x15 (USB_UART_CHIP_RESET)`, and on this transport they
tend to land in `boot:0x0 (DOWNLOAD(USB/UART0))`.

This matters because **`power_off()` leaves pad holds on GPIO8 (power switch)
and GPIO17 (FPGA program_b)**, both RTC-capable. RTC-domain holds survive every
reset above; only a genuine power-on reset clears them. A device that came back
from a brown-out with those holds still set has its power-switch line latched
low and can hang during init.

**The only true cold boot is collapsing the rail: hold Power for ~10 seconds.**
The power switch circuit is documented on the schematic as *"Normally Vsys (up
to 4.5V) / On push, goes LOW / **Hold LOW for 3 sec to shut off**"*. A short
press will not do it, and the orange LED stays lit throughout, so it looks like
nothing happened. Hold it, watch the *purple* LED go out, then press Power.

If you are debugging a device that will not boot, do this **first** — otherwise
you are resetting in a way that preserves the exact state causing the problem.

## Verifying a flash

Read it back; do not trust "Verifying... OK!" to mean it went to the right place.

```sh
espflash read-flash 0x8000 0xc00 pt.bin && espflash partition-table --to-csv pt.bin
espflash read-flash 0x0 0x3d00 bl.bin      # diff against bootloader.bin
espflash read-flash 0x100000 0x100 app.bin # expect magic 0xE9 + a descriptor
```

The app descriptor near the start of the image carries a version string and
build date, which is how you tell whose build is actually resident. Note it is
populated from the ESP-IDF side of the build and can lag the Rust code.

## Building the DFU bootloader

`handheld-dfu` links with `xtensa-esp32s3-elf-gcc` directly rather than through
`ldproxy`, so the ESP-IDF toolchain must be on `PATH`:

```sh
export PATH="$HOME/.espressif/tools/xtensa-esp-elf/esp-14.2.0_20250730/xtensa-esp-elf/bin:$PATH"
cd firmware/handheld-dfu && cargo build --release
espflash save-image --chip esp32s3 target/xtensa-esp32s3-none-elf/release/handheld-dfu dfu.bin
espflash write-bin 0x10000 dfu.bin
```

The image is ~140 KB against a 192 KB partition. `write-bin` is preferred over
`espflash flash --target-app-partition dfu`, which would also rewrite the
bootloader and partition table — no reason to put more regions at risk than the
one you are fixing. A "LOAD segment with RWX permissions" linker warning is
expected.

## Schematics

The `pcb/` tree was removed from the current branch; it lives at the **`v0.1`
tag**, and covers rev1/rev2 only, so treat component values as stale for rev4
(the fuel gauge changed MAX17048 → BQ27427 and POWER_SW moved GPIO15 → GPIO8).
Circuit topology is still representative.

Fastest way to read a sheet is the rendered PDF rather than the KiCad sources:
`pcb/handheld_rev2/handheld.pdf`, one sheet per page — Power Path is page 2.
Locate a sheet with `pdftotext -f N -l N handheld.pdf - | grep "<sheet name>"`.

Worth knowing from that sheet: the power switch is a discrete soft-latch (Q202
AO3401A P-FET gating `VSYS_ALWAYS` → `VSYS`, latched by Q203), and **there is no
separate MCU power-hold output**. `POWER_SW` is simultaneously the button-sense
input and the shutdown assert, so the firmware cannot distinguish "the user is
holding the button" from "I am holding the button".
