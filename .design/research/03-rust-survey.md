# Rust survey — Zephyr 4.4.99/4.5-dev × Baochip 1x

Scope: what Rust support actually exists in this Zephyr tree, whether it can
carry a device driver, and what of xous-core's Rust stack is reusable for the
`hal_bao` / Baochip-1x Zephyr port. Companion to `00-soc-inventory.md`,
`01-boot-delivery.md`, `02-zephyr-integration.md`.

## 1. Rust in Zephyr today (this tree)

**The tree itself contains almost no Rust.** Everything functional lives in a
separate, optional west project, `zephyr-lang-rust`:

- `modules/Kconfig.rust:4-5` — the entire in-tree Kconfig is `config RUST bool`
  (no prompt, no help). It is sourced at `modules/Kconfig:38`.
- `submanifests/optional.yaml:20-26` — `zephyr-lang-rust` is declared as an
  *optional-group* project, pinned to revision `dd73abc2…`, installed at
  `modules/lang/rust`. It is **not checked out in either local workspace**
  (`~/src/zephyr-baochip` or `~/archive/zephyrproject-rtos/zephyr`), so local
  findings below come from the in-tree docs plus the pinned upstream revision.
- `MAINTAINERS.yml:7569-7575` — maintained upstream by d3zd3z, label `area: Rust`.
- No `rust/` directory, no `samples/subsys/rust`, no `lib/rust`, no Rust
  subsystem bindings anywhere in-tree (verified by search).
- `doc/develop/languages/rust/index.rst` is the user-facing story:
  - enable the module via `west config manifest.project-filter +zephyr-lang-rust`
    (lines 24-33), set `CONFIG_RUST` (line 35), call `rust_cargo_application()`
    in the app's `CMakeLists.txt` (line 49);
  - the app is a Cargo **`staticlib` named `rustapp`** depending on the
    `zephyr` crates.io crate (lines 51-69);
  - "Only a few targets currently support Rust" — listed in the module's
    `etc/platforms.txt` (lines 74-76); the doc's own example output names
    `riscv32i-unknown-none-elf` (line 95).

What the pinned module (upstream rev `dd73abc2`, fetched read-only) actually
provides:

- **Linking model**: cargo builds the app as a `staticlib`; a C shim `main.c`
  in the module defines `main()` and calls `#[no_mangle] extern "C" fn
  rust_main()`. Same file provides `rust_log_message()` (funnel into Zephyr
  `LOG_*` macros) and `rust_panic_wrap()` → `k_panic()` for the Rust panic
  handler. So: **applications only, `no_std`, entered from C**.
- **Bindings**: `zephyr-sys` runs bindgen (`build.rs` over `wrapper.h`) at build
  time against the *specific board + Kconfig* headers, exposed as
  `zephyr::raw`. `wrapper.h` pulls in `kernel.h`, `thread_stack.h`,
  `drivers/gpio.h`, `logging/log.h`, `bluetooth/bluetooth.h`, `drivers/flash.h`,
  `irq.h` — i.e. **consumer-side APIs**: GPIO/flash device *usage*, k_* calls,
  irq_lock/unlock wrappers.
- **Devicetree**: `dt-rust.yaml` augments the DT with Rust `Instance`s for
  `gpio-controller`, `gpio-leds`, flash controllers/partitions, and labels —
  again **consumption** of devices from app code, not driver registration.
- **Kconfig**: `zephyr-build` crate exports bool Kconfig into `cfg()` via
  `build.rs`; all settings readable at `zephyr::kconfig::*`.
- **Supported platforms** (`etc/platforms.txt`): `qemu_cortex_m0`,
  `qemu_cortex_m3`, `qemu_riscv32`, `qemu_riscv64`, `m2gl025_miv`. RISC-V is
  exercised in CI (qemu rv32/rv64 + Microsemi MIV rv32im), so a
  riscv32-none-elf Rust toolchain path exists upstream — relevant since
  qemu_riscv32 and Baochip 1x are both rv32imac-class (xous builds
  `riscv32imac-unknown-none-elf`, `.cargo/config.toml:19-22`).
- **Toolchain**: rustup-managed Rust + per-target `core` (e.g.
  `rustup target add thumbv7m-none-eabi`), separate from the Zephyr SDK.
  Cargo is invoked from CMake into the app dir, artifacts land in the Zephyr
  build dir.

In-tree board `boards/espressif/esp32c3_rust/` is *not* Rust integration — it
is the ESP32-C3-DevKit-**RUST** training board; `board.yml`/`Kconfig`/`board.cmake`
are ordinary (selects `SOC_ESP32C3_MINI_N4`, openocd flashing). No Rust build
involvement.

**Version caveat**: nothing Rust-related appears in
`doc/releases/release-notes-4.5.rst` or the 4.5 migration guide; the feature is
unchanged-in-tree between 4.4.99 and 4.5-dev (both trees' `modules/Kconfig.rust`
are byte-identical). Treat Rust support as an out-of-tree, app-level,
explicitly-experimental module as of this tree.

## 2. Could a Zephyr device driver be written in Rust today?

**Not with what exists.** Concrete gaps, all verifiable in-tree:

1. **Entry point**: Rust is reached only via `rust_main()` from the module's C
   `main.c`. There is no path from Zephyr's boot/init machinery (init entries,
   `SYS_INIT`, thread entry) into a Rust staticlib, and no sample does so.
2. **Driver registration**: `DEVICE_DT_DEFINE` (`include/zephyr/device.h:259`)
   expands `struct device` instances into linker-managed iterable sections
   (`Z_DEVICE_SECTION_NAME`, `device.h:1257,1285`; `sys/iterable_sections.h`).
   The `api` pointer is a C vtable (`device.h:1379`, `*_driver_api`). A Rust
   static *could* in principle emit `#[no_mangle] extern "C"` vtable functions
   and a hand-built `struct device` in the right `link_section` — but nothing
   generates the DT-derived `config`/`data` structures for Rust, the module's
   `dt-rust.yaml` only models the consumer side, and no one upstream does this.
   You would be inventing and maintaining a private ABI bridge.
3. **Bindings coverage**: `zephyr-sys/wrapper.h` includes gpio/flash *header*
   APIs for calling, but nothing for `device.h`, `device_tree.h`, init macros,
   or arch ISR registration. Bindings are also config/board-specific
   (`docs/bindings.rst`: "the bindings needed will be very specialized to a
   given board, and even a given configuration").
4. **ISRs**: no `irq_connect` binding. Zephyr's runtime
   `irq_connect_dynamic()` (`include/zephyr/irq.h:65`) is architecturally
   optional and not wired to anything Rust. Direct-mode ISRs need
   assembly/trampoline per arch.
5. **Runtime services**: no allocator wiring (`alloc` crate) is provided —
   strictly `no_std`; panics funnel to `k_panic()`; logging is a formatted
   string across the FFI boundary (file/line lost, per the shim's comments).

**Verdict**: driver-*model* Rust (a `struct uart_bao1x_driver_api` living in
Rust) is technically imaginable but unsupported, undocumented, and would fight
the build system (cargo builds an app-shaped `rustapp` staticlib, not an
arbitrary library linked into `libzephyr.a`). A more honest framing: Rust today
is an application language on Zephyr, full stop.

## 3. Baochip Rust ecosystem — reusable as reference/asset

### 3.1 `utralib/src/generated/bao1x.rs` (21,179 lines)

Generated by svd2utra from the chip SVDs; regenerated automatically by
`utralib/build.rs:125-132` (`bao1x/core.svd` + `bao1x/bao1x_peri.svd` →
`src/generated/bao1x.rs`, via `svd2utra::generate`). Contents:

- Zero-dep `Register`/`Field` value types and the `CSR<T>`/`AtomicCsr<T>`
  register-block accessors (`bao1x.rs:11-25,25-49,52,146`) with
  read/modify/write helpers (`r`, `rf`, `wo`, `wfo`, …) — this is the "UTRA"
  style: each driver names registers symbolically instead of raw offsets.
- `HW_*_MEM` base-address + length constants for every peripheral window
  (`bao1x.rs:247+`, e.g. `HW_DUART_BASE: usize = 0x40042000` at line 2617).
- `pub mod utra` with one module per peripheral — ~100 blocks: core CSRs
  (`d11ctime`, `susres`, `coreuser`, `irqarray0-19`, `mailbox`, `ticktimer`,
  `timer0`), security engine (`alu`, `aes`, `combohash`, `pke`, `trng`,
  `scedma`, `sce_glbsfr`), system (`sysctrl`, `evc`, `rrc`, `mbox_apb`,
  `gluechain`, `mesh`, `qfc`, `mdma`), I/O (`duart`, `iox`, `pwm`, `sddc`,
  `wdg_intf`), BIO (`bio_bdma`, `bio_fifo0-3`), and the PULP-platform UDMA
  peripherals (`udma_uart_0-3`, `udma_spim_0-3`, `udma_i2c_0-3`, `udma_i2s`,
  `udma_sdio`, `udma_camera`, `udma_adc`, `udma_spis_0/1`, `udma_filter`,
  `udma_scif`, `dkpc`) (`bao1x.rs:437-5446`). Each module: `NUMREGS`, per-register
  `Register` consts, per-field `Field` consts, `HW_<NAME>_BASE`.
- The file is data, not logic: it is a **direct, complete register map of the
  SoC** — an excellent cross-check reference for hand-written C drivers.

**License**: `utralib/Cargo.toml:1-9` declares `MIT OR Apache-2.0`
(sv c2utra crate likewise, `svd2utra/Cargo.toml:1-10`); the generated file
itself carries no SPDX header. xous-core root `LICENSE` is Apache-2.0 with
`LICENSES/Apache-2.0.txt`. Vendoring into an Apache-2.0 `hal_bao` is license-clean.

Reuse options: **(a) vendor as reference** — trivially fine, but the Rust
idioms (CSR<T> pointers) don't map to C; the *names, offsets, masks* do.
**(b) compile from Zephyr** — impossible today: no Rust in the Zephyr kernel/
HAL build (see §2); would require the lang-rust module, which builds apps, not
hal code. **(c) regenerate into C** — the right move: the generator input is
the SVD, and a C header **already exists** (§3.2).

### 3.2 The SVD→code path, and what Zephyr could consume

Pipeline on the chip side (`~/archive/baochip/baochip-1x/rtl/scripts/headergen/`):

- `README.md`: run `python3 ./rtl_to_svd.py --path ../../` to regenerate.
- Output (`output/`): `bao1x_peri.svd` (CMSIS-SVD 1.1, `vendor baochip`,
  `name SOC` — `bao1x_peri.svd:2-4`), **`bao1x_peri.h`** (27,888 lines —
  LiteX-style C header, "Auto-generated by daric_to_svd (derived from LiteX)"
  at `bao1x_peri.h:1-3`), `bao1x_peri.rs` (svd2utra-format, byte-compatible
  prologue with utralib's generated file), `apb_check.rs`, `doc/`.
- The C header is already exactly what a HAL wants: `CSR_BASE 0x4002f000`
  (`bao1x_peri.h:13`), per-peripheral `CSR_<NAME>_BASE` and
  `<periph>_<reg>_read()/write()` static inlines plus field extract/replace
  helpers (e.g. `CSR_DUART_BASE = CSR_BASE + 0x13000` → 0x40042000, matching
  `utra::duart::HW_DUART_BASE`, `bao1x_peri.h:3158-3169` ↔ `bao1x.rs:2617`).
  Note it `#include <generated/soc.h>`, `system.h`, `hw/common.h` — LiteX
  scaffolding `hal_bao` would replace with its own `csr_read_simple` shims.

Zephyr-side consumption: **Zephyr has no in-tree SVD tooling** — devicetree
bindings (`dts/bindings/*.yaml`) are the Zephyr-native peripheral description,
and DT generation runs from those, not from SVD (no SVD consumers under
`scripts/` or `dts/`; only coincidental string matches). So the SVD is useful
to *us* (single source of truth to diff against) but the deliverable for
`hal_bao` is: vendor/regenerate `bao1x_peri.h`-style C headers (option c) and
write `dts/bindings/bao,bao1x-*.yaml` describing the same peripherals for DT.
A `check` script diffing `bao1x_peri.h` bases vs the `.dtsi` reg entries vs
`bao1x.rs` constants would catch transcription drift cheaply.

### 3.3 xous-core driver crates (behavioral references for C drivers)

All Apache-2.0 (`libs/bao1x-api/Cargo.toml:6`; repo root `LICENSE`), all
`#![no_std]`, all built on `utralib` CSRs (`libs/bao1x-hal/src/lib.rs:1-28`).

| Crate / module | Path | What it is |
|---|---|---|
| `bao1x-hal` | `libs/bao1x-hal/src/` | Board+SoC HAL: `udma/` (uart, spim, i2c, adc — the PULP UDMA DMA-peripheral drivers), `iox.rs` (GPIO), `clocks.rs` (806 lines), `acram/buram/ifram/rram.rs` (memory init/trim), `sce/`+`sce.rs` (secure engine), `sigcheck.rs` (1015 lines, signature/boot verify), `wdt.rs`, `mbox.rs`, `rtc.rs`, `coreuser.rs`, `hardening.rs` |
| `bao1x-hal` board devices | `libs/bao1x-hal/src/{axp2101,bmp180,lis2dh12,sh1107,gc2145,ov2640}.rs`, `usb/`, `kpc_aoint.rs` | Dabao peripherals: AXP2101 PMIC, BMP180 baro, LIS2DH12 accel, SH1107 OLED, GC2145/OV2640 cameras, USB, keyboard controller |
| `bao1x-api` | `libs/bao1x-api/src/` | SoC-level API surface for kernel+services: `i2c.rs`, `iox.rs`, `keyboard.rs`, `clocks.rs`, `udma.rs`, `sce/`, `signatures.rs`, `bio.rs`+`bio_resources.rs` (BIO slot map) |
| `xous-bio` | `libs/xous-bio/src/` | BIO (I/O expansion) bus driver + tests, I2C side |
| `xous-bio-bdma` | `libs/xous-bio-bdma/` | BIO DMA engine driver (HW at 0x50124000) |
| `xous-pio` | `libs/xous-pio/src/` | Baochip PIO (RP2040-style programmable I/O) driver + tests |
| `xous-pl230` | `libs/xous-pl230/src/` | PL230 debug UART driver (HW at 0x40010000 region) |
| `bao1x-checks` | `libs/bao1x-checks/` | Host-side diagnostics for the BIO one-way counter slot map |
| `baremetal` | `baremetal/src/platform/bao1x/` | Non-Xous bare-iron target — **closest analog to a Zephyr port**: `debug.rs` (duart console via `utra::duart` CSRs, lines 1-40), `irq.rs`, `bao1x.rs`, `avtrng.rs`, `usb/`, `dabao_selftest.rs`; deps show the runtime recipe: `riscv 0.14` (critical-section-single-hart), `xous-riscv` (VexRiscv CSRs), `linked_list_allocator` (`baremetal/Cargo.toml:10-21`) |
| `bao1x-boot` | `bao1x-boot/` | boot0/boot1 secure boot chain (see `01-boot-delivery.md`), `uf2send.py`, patched `ed25519-dalek`/`sha2`/`slh-dsa` crates for the SCE |

These are **behavioral references, not portable code**: they assume Xous idioms
(critical-section single-hart, kernel messaging in `bao1x-api`) or bare-metal
single-threaded execution. The register-level sequences (UDMA channel setup,
clock tree, SCE provisioning, AXP2101/I2C bringup, USB init) transliterate well
to C; see `baremetal/src/platform/bao1x/debug.rs:29-38` for the canonical
UTRA-style RMW pattern to mirror.

## 4. Bottom line — ranked options

1. **All C, Rust ecosystem as documentation (recommended baseline).** Write
   `hal_bao` + drivers in C against vendored `bao1x_peri.h`-style headers;
   keep `bao1x.rs`, `bao1x-hal`, `baremetal` open as behavioral references and
   as a machine-checkable register-map cross-check (diff bases vs DTS vs
   utralib). Zero toolchain risk, matches Zephyr norms, all of xous-core's
   knowledge is still exploited. Effort: none beyond the port itself.
2. **Option 1 + a Rust *application* sample on baochip (stretch).** After the
   port boots, wire the optional `zephyr-lang-rust` module and stand up a
   `rustapp` hello-world for the dabao board (RISC-V is already an exercised
   Rust platform via qemu_riscv32/m2gl025_miv; target triple
   `riscv32imac-unknown-none-elf` as in xous). Requires: rustup + target in
   CI, adding the board to the module's `platforms.txt`, and accepting
   app-level-only scope. Effort: small but touches an out-of-tree module we
   don't control — fine as a demo, wrong as a dependency.
3. **Rust leaf drivers behind C shims (not recommended now).** E.g. UART or
   GPIO vtable functions as `extern "C"` in a staticlib with a C-side
   `DEVICE_DT_DEFINE`. Blocked on §2 items 2-5 (DT struct generation, ISR
   binding, no `alloc`, private-ABI maintenance). Only worth revisiting if
   upstream grows driver-model Rust support; watch the module, don't fork it.
4. **Rust kernel/HAL (rejected).** Zephyr's kernel, arch, and driver model are
   C with macro/linker machinery that has no Rust story; xous-core already
   exists for people who want a Rust OS on this chip.

## Sources

- `~/src/zephyr-baochip/modules/Kconfig.rust:4-5`, `modules/Kconfig:38`,
  `submanifests/optional.yaml:20-26`, `MAINTAINERS.yml:7569-7575`
- `~/src/zephyr-baochip/doc/develop/languages/rust/index.rst:24-95`
- `~/src/zephyr-baochip/boards/espressif/esp32c3_rust/{board.yml,Kconfig.esp32c3_rust,board.cmake}`
- `~/src/zephyr-baochip/include/zephyr/device.h:259,1257,1285,1379`,
  `include/zephyr/irq.h:65`
- [zephyr-lang-rust @ dd73abc2](https://github.com/zephyrproject-rtos/zephyr-lang-rust/tree/dd73abc242e995784da62352fe8c70d9a6c7ac2e):
  `main.c`, `README.rst`, `docs/bindings.rst`, `dt-rust.yaml`,
  `zephyr-sys/wrapper.h`, `etc/platforms.txt`
- `~/archive/betrusted-io/xous-core/utralib/src/generated/bao1x.rs:11-146,247,437,2600-2617,5446`;
  `utralib/build.rs:125-160`; `utralib/Cargo.toml:1-20`;
  `svd2utra/Cargo.toml:1-10`; `LICENSES/Apache-2.0.txt`
- `~/archive/baochip/baochip-1x/rtl/scripts/headergen/README.md`;
  `output/bao1x_peri.h:1-13,3158-3169`; `output/bao1x_peri.svd:2-4`;
  `output/bao1x_peri.rs:1-14`
- `~/archive/betrusted-io/xous-core/libs/` (`bao1x-hal`, `bao1x-api`,
  `bao1x-checks`, `xous-bio`, `xous-bio-bdma`, `xous-pio`, `xous-pl230`);
  `libs/bao1x-api/Cargo.toml:6`
- `~/archive/betrusted-io/xous-core/baremetal/Cargo.toml:10-21`;
  `baremetal/src/platform/bao1x/debug.rs:1-40`;
  `.cargo/config.toml:19-22`; `bao1x-boot/` (BOOTCHAIN.md, uf2send.py)
