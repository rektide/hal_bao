---
type: Research note
title: Zephyr integration plan for Baochip 1x / Dabao (SoC port, board port, hal_bao, runners, toolchain)
description: How a new RV32IMAC VexRiscv SoC + SoM board lands in Zephyr 4.5-dev, and whether a hal_bao module is warranted
tags: [baochip, zephyr, soc-port, board-port, hal-module, uf2, riscv]
status: draft
generated: { by: model:sol-xhigh-gpt, at: 2026-08-14 }
sources:
  - ~/src/zephyr-baochip (Zephyr 4.4.99 / 4.5-dev fork, all line refs below)
  - ~/archive/zephyrproject-rtos (hal_espressif, hal_nxp, hal_stm32 checkouts)
---

# 02 — Zephyr integration: SoC port, board port, hal_bao, runners, toolchain

Companion docs: [00-soc-inventory](00-soc-inventory.md), [01-boot-delivery](01-boot-delivery.md),
[03-rust-survey](03-rust-survey.md). All `path:line` references are relative to
`~/src/zephyr-baochip` unless prefixed with `~/archive/zephyrproject-rtos`.

## Key conclusions (TL;DR)

1. A **M-mode-only SoC with a fully custom IRQ controller and no CLINT/PLIC is a
   supported configuration** — `soc/litex/litex_vexriscv` is the exact precedent
   (VexRiscv core, LiteX timer as sysclock, custom-CSR intc driver overriding
   `arch_irq_*`). Baochip needs: 1 intc driver, 1 timer driver, periph drivers,
   5 SoC files, 1 dtsi, ~4 bindings.
2. **hal_bao is not needed to boot.** `hal_*` modules exist to mirror vendor C
   SDKs/blobs (hal_stm32 = CubeHAL, hal_nxp = MCUX, hal_espressif = ESP-IDF +
   wifi blobs). Baochip's only vendor SDK is Rust; the C deliverables are a
   generated register header + SVD. Everything can live in the zephyr fork
   (litex/neorv32/renode precedent: zero hal module). Defer hal_bao until vendor
   artifacts stabilize; if created, clone the hal_stm32 shape
   (`cmake: .` + `dts_root: .`), pin it in the fork's `west.yml` under
   `groups: [hal]`.
3. **UF2 is nearly free**: generic build-side support (`CONFIG_BUILD_OUTPUT_UF2`
   + `BUILD_OUTPUT_UF2_FAMILY_ID`, custom hex ID allowed, set in SoC
   `Kconfig.defconfig`) + generic `uf2` runner that copies to any mounted
   USB-MSC `INFO_UF2.TXT` volume. No family ID is hardcoded in the runner.
4. **ISA/toolchain is declarative**: `riscv,isa-base` + `riscv,isa-extensions`
   DT props on `cpu@0` flow to `RISCV_ISA_*` Kconfig then `-march`/`-mabi`.
   `rv32imac_zicsr` = exactly what `dts/riscv/gd/gd32vf103.dtsi:27-28` already
   does. Zephyr SDK `riscv64-zephyr-elf` toolchain covers RV32IMAC.
5. Biggest risks: (a) USB CDC-ACM console needs a from-scratch USB device
   controller driver (the UF2 bootloader's USB stack is not app-reusable);
   (b) memory map/linker assumptions (`CONFIG_INCLUDE_RESET_VECTOR`, XIP vs
   RAM-exec, where boot1 loads Zephyr); (c) upstreaming later requires
   `baochip` vendor-prefix + REUSE-clean headers, cheap to do right from day 1.

---

## 1. New RISC-V SoC port anatomy (Zephyr 4.5-dev, HWMv2)

### 1.1 Where SoCs live and the required files

SoCs live in `soc/<vendor>/<soc-name>/` (vendor dir mandatory for upstream
contribution and must match a prefix in `dts/bindings/vendor-prefixes.txt` —
`doc/hardware/porting/soc_porting.rst:36-49`). The doc's minimal layout
(`doc/hardware/porting/soc_porting.rst:57-105`):

```
soc/<VENDOR>/<soc-name>/
├── soc.yml          # mandatory: soc (and optional family/series) metadata
├── soc.h            # mandatory: SoC config macros included by drivers
├── CMakeLists.txt   # mandatory: sources, include dirs, SOC_LINKER_SCRIPT
├── Kconfig.soc      # mandatory: config SOC_<NAME>, SOC string
├── Kconfig          # optional: selects arch/peripheral capabilities
└── Kconfig.defconfig# optional: NUM_IRQS, clock defaults
```

Family/series hierarchy is expressed in `soc.yml` at the family root, e.g.
`soc/litex/soc.yml:1-7` (family litex → series litex_vexriscv → soc
litex_vexriscv) with matching `SOC_FAMILY_*`/`SOC_SERIES_*` symbols in
`soc/litex/litex_vexriscv/Kconfig.soc:6-13`. A lone SoC needs only
`socs: [{name: <soc>}]` (`soc/neorv32/soc.yml:1-2`,
`soc/renode/riscv_virtual/soc.yml:1-2`).

### 1.2 Precedent A — `soc/litex/litex_vexriscv` (VexRiscv, closest match)

Total SoC content — **4 files + the family-level `soc.yml`**:

- `soc/litex/litex_vexriscv/CMakeLists.txt:7-14` — sources
  `soc/common/riscv-privileged/soc_irq.S` + `vector.S` verbatim, and sets the
  generic linker script:
  `set(SOC_LINKER_SCRIPT ${ZEPHYR_BASE}/include/zephyr/arch/riscv/common/linker.ld)`
- `soc/litex/litex_vexriscv/Kconfig:6-12` — the entire capability statement:

  ```kconfig
  config SOC_LITEX_VEXRISCV
      select RISCV
      select INCLUDE_RESET_VECTOR
      select CPU_HAS_ICACHE if $(dt_node_int_prop_int,/cpus/cpu@0,i-cache-line-size) > 0
      select CPU_HAS_DCACHE if $(dt_node_int_prop_int,/cpus/cpu@0,d-cache-line-size) > 0
  ```

  Note: it does **not** select `RISCV_PRIVILEGED` (that symbol, defined at
  `arch/riscv/Kconfig:135-139`, is only for SoCs following the privileged spec
  for IRQ management). It reuses two assembly files from the "privileged"
  common dir anyway because VexRiscv does implement `mtvec`/`mstatus`/`mie`.
- `Kconfig.defconfig:6-11` — `NUM_IRQS = 12`.
- `Kconfig.soc:6-16` — family/series/soc symbols + `SOC` string.
- Shared helpers live one level up in `soc/litex/common/` (`soc.h` with LiteX
  CSR sub-register accessors, `reboot.c`), included via
  `soc/litex/CMakeLists.txt:4-5` (`add_subdirectory(common)` +
  `add_subdirectory(${CONFIG_SOC_SERIES})`).

**Timer**: no CLINT/mtime at all. The system clock is the LiteX `timer0`
peripheral; `drivers/timer/Kconfig.litex:6-12` makes `CONFIG_LITEX_TIMER`
`default y` when `DT_HAS_LITEX_TIMER0_ENABLED` — so the board DTS enabling the
timer node is all it takes. `drivers/timer/litex_timer.c` implements
`sys_clock_*` purely from DT regs (`DT_INST_REG_ADDR_BY_NAME(0, load)` etc.,
litex_timer.c:13-20). This proves **any memory-mapped timer can be the Zephyr
sysclock**; the generic CLINT-style driver is just one option
(`drivers/timer/Kconfig.riscv_machine:8-14`, `RISCV_MACHINE_TIMER`, needs a
`riscv,machine-timer` compatible).

**Interrupts — no PLIC, no CLIC, custom mechanism**: VexRiscv's LiteX interrupt
plugin exposes mask/pending through **custom CSRs** whose numbers come from DT
(`dts/riscv/riscv32-litex-vexriscv.dtsi:34-42`: `intc0` with
`reg = <0xbc0 0x4 0xfc0 0x4>`, `reg-names = "irq_mask","irq_pending"`,
`riscv,max-priority = <7>`; binding
`dts/bindings/interrupt-controller/litex,vexriscv-intc0.yaml`).
`drivers/interrupt_controller/intc_vexriscv_litex.c`:

- `intc_vexriscv_litex.c:21-32` — `csrw/csr` to the DT-provided CSR numbers;
- `intc_vexriscv_litex.c:71-84` — **overrides `arch_irq_enable/disable/is_enabled`**
  (this is the supported way to replace the riscv-privileged `mie`-based
  implementation, `soc/common/riscv-privileged/soc_common_irq.c:53-180`);
- `intc_vexriscv_litex.c:86-93` — init enables machine external IRQ 11
  (`RISCV_IRQ_MEXT`, `include/zephyr/arch/riscv/irq.h:45`) in `mie` and
  `IRQ_CONNECT(RISCV_IRQ_MEXT, 0, vexriscv_litex_irq_handler, ...)`;
- `intc_vexriscv_litex.c:52-68` — handler reads pending & mask, then dispatches
  DT-declared device IRQs straight out of `_sw_isr_table`
  (`DT_FOREACH_STATUS_OKAY_NODE`), i.e. all device IRQs are level-1 entries in
  the software table; `NUM_IRQS=12` sizes that table.

**Trap entry / mtvec / exceptions**: `soc/common/riscv-privileged/vector.S`
provides `__start` (reset vector): set `gp`, optional `soc_reset_hook`, then in
direct mode `mtvec = _isr_wrapper` (vector.S:95-102, non-vectored default), CLIC
(vectored.S:45-50) and CLINT-vectored variants included. The generic ISR
wrapper (`arch/riscv/core/isr.S:734-736`) calls weak `__soc_handle_irq(cause)`
so each SoC can clear its own pending bit; the weak common impl writes
`mip` (`soc/common/riscv-privileged/soc_irq.S:21-47`). SOCs override it when
their silicon differs: `soc/neorv32/soc_irq.S:12-15` is just `ret`, and
`soc/espressif/esp32c3/soc_irq.S:13-15` likewise ("status clearing is done at
ISR"). There is also an **even more radical** escape hatch:
`RISCV_SOC_HAS_CUSTOM_IRQ_HANDLING` (`arch/riscv/Kconfig:177-181`) replaces the
whole dispatch with a SoC-provided `__soc_handle_all_irqs`. So the ladder for
baochip is: default wrapper + own intc driver (litex model) → override
`__soc_handle_irq` → full custom handling.

**Syscalls/exceptions**: nothing SoC-specific required — ecall syscall trap and
exception handling are in `arch/riscv/core/` generically.

### 1.3 Precedent B — `soc/common/riscv-privileged/` (what it assumes)

Contents: `vector.S` (reset + mtvec modes), `soc_irq.S` (weak
`__soc_handle_irq`), `soc_common_irq.c` (`arch_irq_*` over `mie`/`sie` CSRs,
plus level-2 dispatch to PLIC `riscv_plic_*` / AIA `riscv_aia_*` / CLIC
drivers), `Kconfig` (only `RISCV_VECTORED_MODE`).
It assumes: standard `mstatus`/`mie`/`mip`/`mtvec`/`mcause` CSRs and `mret`, and
optionally PLIC/CLIC/AIA. **Baochip fits without standard CLINT/PLIC**: like
litex_vexriscv it can reuse `vector.S` + weak `soc_irq.S` (or its own
`reset.S`/`soc_irq.S` like `soc/neorv32/CMakeLists.txt:8-13`) and let its intc
driver own `arch_irq_*`. Baochip's IRQ controller is memory-mapped (per
Xous/utralib) rather than CSR-mapped, which is *simpler* than the vexriscv
case — the driver just uses `sys_write32` on DT regs.

### 1.4 Precedent C — minimal vendor SoCs

- `soc/renode/riscv_virtual/` — 4 files + soc.yml. `Kconfig:6-11`:
  `select RISCV, RISCV_PRIVILEGED, INCLUDE_RESET_VECTOR, RISCV_HAS_PLIC,
  RISCV_SOC_HAS_GP_RELATIVE_ADDRESSING`. `Kconfig.defconfig:6-40` shows the
  multilevel-IRQ plumbing you only need if you use PLIC-style level-2 IRQs
  (`1ST_LEVEL_INTERRUPT_BITS`, `2ND_LVL_ISR_TBL_OFFSET`, `NUM_IRQS 2058`...).
  `CMakeLists.txt` is 5 lines. If baochip uses a flat single-level intc
  (litex-style), none of that is needed.
- `soc/neorv32/` — single-directory SoC with own `reset.S` + `soc_irq.S` +
  `soc.c`, `select RISCV, RISCV_PRIVILEGED, RISCV_SOC_HAS_GP_RELATIVE_ADDRESSING`
  (`Kconfig:6-9`), plus a nice pattern for runtime clock discovery
  (`SOC_NEORV32_READ_FREQUENCY_AT_RUNTIME`, Kconfig:15-27).

### 1.5 Minimum viable SoC port (answer)

Files (baochip, flat single-soc style):

| file | content |
|---|---|
| `soc/baochip/bao1x/soc.yml` | `socs: [{name: bao1x}]` |
| `soc/baochip/bao1x/Kconfig.soc` | `config SOC_BAO1X`, `SOC` default `"bao1x"` |
| `soc/baochip/bao1x/Kconfig` | `select RISCV`, `INCLUDE_RESET_VECTOR`, cache selects from DT like litex, `select RISCV_M_MODE` implicit (default, `arch/riscv/Kconfig:671-677`) |
| `soc/baochip/bao1x/Kconfig.defconfig` | `NUM_IRQS`, `SYS_CLOCK_HW_CYCLES_PER_SEC` (from DT like rp2040, `soc/raspberrypi/rpi_pico/rp2040/Kconfig.defconfig:17-19`) |
| `soc/baochip/bao1x/CMakeLists.txt` | sources common `vector.S`/`soc_irq.S` (litex CMakeLists:7-11) or own `reset.S`; `SOC_LINKER_SCRIPT` = generic `include/zephyr/arch/riscv/common/linker.ld` |
| `soc/baochip/bao1x/soc.h` | include guard, mem-map macros, `sys_reboot` hooks if any |

What arch/riscv requires from the SoC layer: (a) an IRQ strategy — default
`mie` bits, or an intc driver providing `arch_irq_*` (vexriscv/esp32c3 model);
(b) a system-clock driver registered in `drivers/timer/` keyed on a DT
compatible; (c) trap entry — `vector.S` reuse or custom `__reset` (the
`INCLUDE_RESET_VECTOR` help text at `arch/riscv/Kconfig:126-131` documents the
default-jump-to-`__initialize` and the custom override); (d) `NUM_IRQS` sizing
`_sw_isr_table`. Non-privileged-spec, M-only custom-IRQ cores: **supported**,
see §1.2.

## 2. Board port (HWMv2)

Mandatory/optional files per `doc/hardware/porting/board_porting.rst:252-315`:
mandatory = `board.yml`, `<board>_<qualifiers>.dts`, `Kconfig.<board>`;
optional = `Kconfig`, `Kconfig.defconfig`, `<board>[_<qualifiers>]_defconfig`,
`board.cmake`, `CMakeLists.txt`, `doc/`, `<board>.yaml` (twister metadata).
Board qualifiers `<soc>/<cpucluster>/<variant>` map to filenames with `_`.
"Image defs" in HWMv2 = board variants/qualifiers in `board.yml` (e.g. the
`smode` variant at `boards/renode/riscv32_virtual/board.yml:6-9`) plus sysbuild
variants like the rp2040 `*_mcuboot` images
(`boards/raspberrypi/rpi_pico/rpi_pico_rp2040_mcuboot.*`).

Smallest real precedent, `boards/renode/riscv32_virtual/`:

- `board.yml` — name/full_name/vendor/socs (+variants).
- `riscv32_virtual.dts` — `/dts-v1/; #include <renode_riscv32_virt.dtsi>`,
  `chosen {zephyr,console = &uart0; zephyr,flash = &flash0; zephyr,sram = &sram0;}`,
  `&uart0 {status="okay"};`.
- `Kconfig.riscv32_virtual:5-6` — `config BOARD_RISCV32_VIRTUAL; select
  SOC_RISCV_VIRTUAL_RENODE` (that's the whole mandatory Kconfig).
- `riscv32_virtual_defconfig` — 8 lines: `CONFIG_CONSOLE/SERIAL/UART_CONSOLE=y,
  CONFIG_GPIO=n, CONFIG_XIP=y, CONFIG_SYS_CLOCK_TICKS_PER_SEC=100`.
- `board.cmake:3-6` — `SUPPORTED_EMU_PLATFORMS renode`, `RENODE_SCRIPT`,
  `RENODE_UART` (no hardware flasher at all — `board.cmake` is optional).
- `riscv32_virtual.yaml` — twister metadata: `arch: riscv, toolchain: [zephyr],
  ram/flash, simulation: renode, supported: [uart], testing.ignore_tags`.

**USB console for Dabao**: the blessed pattern is the shared fragments
`boards/common/usb/Kconfig.cdc_acm_serial.defconfig:1-46` (sets
`BOARD_REQUIRES_SERIAL_BACKEND_CDC_ACM`, `SERIAL`, `CONSOLE`, `UART_CONSOLE`,
`USB_DEVICE_STACK_NEXT`, CDC-ACM at boot, 4s log delay) and
`boards/common/usb/cdc_acm_serial.dtsi:10-20` (chosen → `board_cdc_acm_uart`
node under `&zephyr_udc0`). Boards include both fragments
(`doc/hardware/porting/board_porting.rst:657-660` explicitly tells USB-only
boards to do this). Hard dependency: a baochip USB device controller driver +
`zephyr_udc0` node — that driver is one of the largest single work items
(see Risks). Buttons are just `gpio-keys` compatible nodes; Dabao's 2 buttons +
GPIO headers are trivial DT (`boards/enjoydigital/litex_vexriscv/litex_vexriscv.dts`
shows per-register UART/timer DT style).

Minimal Dabao set: `boards/baochip/dabao/{board.yml, dabao.dts,
Kconfig.dabao, dabao_defconfig, board.cmake, dabao.yaml, doc/index.rst}`.

## 3. `hal_bao` module repo design

### 3.1 What the existing hal_* modules actually are

- **hal_espressif** (`~/archive/zephyrproject-rtos/hal_espressif/zephyr/module.yml:1-10`):
  `build: {cmake: zephyr, kconfig: zephyr/Kconfig, settings: {dts_root: .}}`,
  pip requirement-files, **dozens of prebuilt RF blobs** registered in the
  `blobs:` section (`west blobs fetch hal_espressif`), plus module-shipped west
  commands (`west/west-commands.yml:1-7`, `espressif` tool). It's a mirror of
  ESP-IDF (a real vendor C SDK).
- **hal_nxp** (`hal_nxp/zephyr/module.yml:1-6`): `cmake-ext: True` +
  `kconfig-ext: True` + `settings: {dts_root: .}` — the *glue* lives in the
  **zephyr tree** at `modules/hal_nxp/` (tracked upstream:
  `modules/hal_nxp/CMakeLists.txt`, `Kconfig`, `imx/`, `mcux/mcux-sdk-ng/*.cmake`
  glue — `git ls-files modules/hal_nxp` in the archive zephyr), while the module
  repo carries the MCUX SDK sources and `dts/nxp/` dtsi files. Blobs for
  wifi/BLE firmware.
- **hal_stm32** (`hal_stm32/zephyr/module.yml:1-5`): `build: {cmake: ., settings:
  {dts_root: .}}`; root `CMakeLists.txt:9-10` does
  `add_subdirectory_ifdef(CONFIG_HAS_STM32CUBE stm32cube)`, so zephyr-tree
  drivers opt into Cube sources via `HAS_STM32CUBE` (symbol declared in
  zephyr-tree glue `modules/Kconfig.stm32:9-14`). Ships `dts/st/<series>/*.dtsi`
  (SoC devicetrees live in the module, bindings stay in zephyr tree), `LICENSES/`
  + `REUSE.toml` at repo root.

West integration: each hal is a pinned project in `zephyr/west.yml` under
`groups: [hal]` (hal_espressif west.yml:174-180, hal_gigadevice:185,
hal_nxp:215-221, hal_rpi_pico:240-246, hal_stm32:260-266, hal_wch:280-286);
`group-filter: [-babblesim, -optional, -testing]` (west.yml:23) keeps
unneeded hals uncloned. Licensing: blobs need a license-path entry; source
trees are expected REUSE-clean (hal_stm32 carries `REUSE.toml` + `LICENSES/`).
Tags: not standardized — hal_nxp uses `release/v4.3`, hal_espressif
`snapshot/1`; zephyr pins raw SHAs, so tags are convenience only.

### 3.2 What `module.yml` can declare (verified from code + docs)

`build.settings`: `board_root`, `dts_root`, `snippet_root`, `soc_root`,
`arch_root`, `module_ext_root`, `sca_root`
(`scripts/zephyr_module.py:74-96`; `doc/develop/modules.rst:923-941`). A module
with `soc_root`/`board_root` can host entire SoC/board definitions out-of-tree
(files under `<root>/soc`, `<root>/boards`, `<root>/dts`). `cmake:`/`kconfig:`
point at the module's own build glue; `cmake-ext/kconfig-ext` put the glue in a
module_ext_root (the zephyr repo itself is always one — `doc/develop/modules.rst:905-913`).

### 3.3 Recommendation: in-tree now, hal_bao later (and small)

When a `hal_` module is warranted: you are mirroring a substantial vendor C SDK
or blobs that (a) change on the vendor's cadence, (b) are shared across Zephyr
versions, (c) carry licenses you want isolated from zephyr history. When it is
**not**: litex, neorv32, renode, sifive, etc. all ship zero hal module —
drivers talk to DT-described registers directly. Baochip has **no vendor C
SDK** (Rust only) — there is nothing to mirror but a generated
`bao1x_peri.h` + SVD. Two defensible options:

- **Recommended now — all in-tree in the fork** (`~/src/zephyr-baochip`): soc/,
  dts/, bindings, drivers, boards per §7. Vendor the generated header(s) under
  the soc dir (e.g. `soc/baochip/bao1x/include/bao1x_peri.h`) or generate
  drivers purely from DT regs (litex style) — minimal moving parts, fastest to
  a blinking LED, matches the litex precedent exactly.
- **hal_bao when** one of: vendor header regen cadence starts polluting zephyr
  history; you need the same vendored artifacts for other consumers (other OS,
  host tools, Rust PAC interop — see [03-rust-survey](03-rust-survey.md)); or
  you start maintaining multiple zephyr versions. Shape: hal_stm32-style
  (`zephyr/module.yml`: `name: hal_bao`, `build: {cmake: ., settings: {dts_root:
  .}}`), containing `include/bao1x_peri.h` (vendored, SPDX-tagged),
  `svd/bao1x.svd`, `dts/baochip/bao1x.dtsi` (optional move), `tools/uf2/`
  assets, `openocd/` configs, `LICENSES/` + `REUSE.toml`; zephyr tree keeps
  soc/Kconfig glue, drivers, bindings, boards. Pin in the fork's `west.yml`:
  `- name: hal_bao, path: modules/hal/bao, revision: <sha>, groups: [hal]`.

Split rule of thumb regardless: **DT bindings + drivers + Kconfig stay in the
zephyr tree** (they are API-coupled to subsystems); **raw vendor artifacts**
(register headers, SVD, bootloader/tool files, linker fragments if vendored)
belong to the hal module; `soc/` definitions can be either (in-tree
recommended for a fork that will be upstreamed).

## 4. Flashing / debug runners

Runner framework: each board's `board.cmake` calls `board_set_flasher_ifnset()`,
`board_runner_args()`, `board_finalize_runner_args()` and includes shared
snippets from `boards/common/*.board.cmake`; runners are python classes in
`scripts/west_commands/runners/`.

**UF2 (the rp204x/rp235x path) is fully generic:**

- Build side: `CONFIG_BUILD_OUTPUT_UF2` (+ `BUILD_OUTPUT_UF2_FAMILY_ID`,
  `_USE_FLASH_BASE`, `_USE_FLASH_OFFSET`) — `Kconfig.zephyr:859-890`. Family ID
  help text: "either a hex, e.g. 0x68ed2b88, or well-known family name string...
  SoC-specific defaults are set in the SoC layer Kconfig.defconfig files."
  Generation is a post-build step running `scripts/build/uf2conv.py -c -f
  ${CONFIG_BUILD_OUTPUT_UF2_FAMILY_ID} ...` (`CMakeLists.txt:1884-1897`), with
  the registry of known names in `scripts/build/uf2families.json` (RP2040 =
  `0xe48bff56`, set at
  `soc/raspberrypi/rpi_pico/rp2040/Kconfig.defconfig:11-12`; rp2350 likewise at
  `rp2350/Kconfig.defconfig:8`).
- Flash side: `scripts/west_commands/runners/uf2.py` — `flash` only. It scans
  `psutil.disk_partitions()` for a FAT mount containing `INFO_UF2.TXT`
  (uf2.py:55-60), optionally filters by `--board-id` matched against the
  `Board-ID:` line (uf2.py:38-39, 69-73), then copies `cfg.uf2_file` (the
  build's `zephyr.uf2`, wired via `cmake/flash/CMakeLists.txt:59-62`) to the
  mount (uf2.py:88-100). **No family IDs are hardcoded in the runner** — a
  custom baochip family ID requires only the SoC Kconfig default (pick a fresh
  32-bit constant; optionally add it to `uf2families.json` + the bootloader
  must check it).
- Board hookup: `boards/common/uf2.board.cmake:1-4`
  (`board_set_flasher_ifnset(uf2)`) and per-board args, e.g.
  `boards/raspberrypi/rpi_pico/board.cmake:31` (`board_runner_args(uf2
  "--board-id=RPI-RP2")`) + include at board.cmake:47.

**Serial-upload runners** available for contrast: `bossac.py` (native USB
SAM-BA), `dfu.py` (dfu-util), `esp32.py` (esptool; needs hal_espressif /
ESP-IDF python env), `gd32isp.py`, `wchisp.py`, `teensy.py`. If the baochip
UF2 bootloader also exposes a serial upload protocol, a `bao_loader` runner
modeled on `bossac.py` could be added later — but UF2-MSC covers the primary
flow (see [01-boot-delivery](01-boot-delivery.md)).

**Renode**: yes — `scripts/west_commands/runners/renode.py` exists
(`capabilities: {'simulate'}`, renode.py:26-27); boards opt in via
`boards/common/renode.board.cmake:1-8` which passes
`--renode-command=$elf=@...` + `include @${RENODE_SCRIPT}`, and the board sets
`SUPPORTED_EMU_PLATFORMS renode`, `RENODE_SCRIPT`, `RENODE_UART`
(`boards/renode/riscv32_virtual/board.cmake:3-6`) plus twister `simulation:`
section (`riscv32_virtual.yaml`). Strongly recommended for baochip CI (Renode
models VexRiscv; Xous already emulates this chip under Renode).

**Recommended for Dabao**: `board.cmake` with
`board_runner_args(uf2 "--board-id=BAO-DABAO")` (or matching the bootloader's
INFO_UF2.TXT), `include(.../uf2.board.cmake)`, `board_set_flasher_ifnset(uf2)`;
debugger support (optional) via `board_runner_args(openocd ...)` + a bao1x
target cfg, following the rpi_pico pattern of pre-init `source [find ...]`
commands (`boards/raspberrypi/rpi_pico/board.cmake:19-23`); Renode simulate
runner for CI.

## 5. Toolchain

- **Zephyr SDK**: one `riscv64-zephyr-elf` GCC toolchain builds both rv32 and
  rv64 targets (`west sdk install --toolchains riscv64-zephyr-elf` —
  `doc/develop/west/zephyr-cmds.rst:558`; toolchain cmake lives in
  `cmake/toolchain/zephyr/`). RV32 boards simply declare `toolchain: [zephyr]`
  in their twister yaml (`boards/enjoydigital/litex_vexriscv/litex_vexriscv.yaml`,
  `boards/espressif/esp32c3_devkitm/esp32c3_devkitm.yaml:7-8`).
- **ISA is devicetree-driven, not hand-written `-march`**: `cpu@0` properties
  `riscv,isa-base` / `riscv,isa-extensions` (+ `riscv,isa-omissions`) are
  consumed by `arch/riscv/Kconfig.isa:4-5` to default every
  `RISCV_ISA_RV32I`/`RISCV_ISA_EXT_{M,A,C,ZICSR,ZIFENCEI,ZBA,ZBB,ZK,...}`
  symbol; `cmake/compiler/gcc/target_riscv.cmake:4-100+` then composes
  `-march` (`rv32` + `i/m/a/c` + `_zicsr`...) and `-mabi` (`ilp32`, `ilp32f`...).
  Exact rv32imac+zicsr precedent: `dts/riscv/gd/gd32vf103.dtsi:27-28`
  (`riscv,isa-base = "rv32i"; riscv,isa-extensions = "i","m","a","c","zicsr","zifencei"`).
  Crypto-ish extensions available today: ZK/ZKS/ZBKB/ZBC/ZBS (Kconfig.isa).
  For *non-standard* vendor extensions the escape hatch is a SoC CMake override
  (precedent: `arch/riscv/custom/openisa/ri5cy/CMakeLists.txt:8`
  `zephyr_compile_options(-march=rv32imcxpulpv2)`).
- rv32e is also supported (`RISCV_ISA_RV32E`, target_riscv.cmake:20-24) —
  irrelevant for baochip's IMAC but shows the range.
- Privilege mode: kernel runs M-mode by default; `RISCV_PRIVILEGE_MODE` choice
  at `arch/riscv/Kconfig:665-687`, keyed off DT `riscv,privilege-modes`.

## 6. Devicetree & bindings for a custom SoC

- Vendor prefix: add `baochip` to `dts/bindings/vendor-prefixes.txt`
  (`grep` confirms no existing `bao`/`baochip` entry today); required before
  upstream contribution (`doc/hardware/porting/soc_porting.rst:46-49`).
- Bindings live under `dts/bindings/<class>/<vendor>,<device>.yaml` in-tree
  (litex examples: `dts/bindings/serial/litex,uart.yaml`,
  `dts/bindings/gpio/litex,gpio.yaml` — a `gpio-controller.yaml` include with
  `gpio-cells: [pin, flags]`, `dts/bindings/timer/litex,timer0.yaml` — minimal
  `base.yaml` include requiring only `reg` + `interrupts`,
  `dts/bindings/interrupt-controller/litex,vexriscv-intc0.yaml` — 2 interrupt
  cells (irq, priority) + custom `riscv,max-priority` property).
- LiteX convention worth copying: one DT reg per named register
  (`reg-names = "rxtx","txfull","rxempty",...`,
  `boards/enjoydigital/litex_vexriscv/litex_vexriscv.dts:47-70`) so drivers use
  `DT_INST_REG_ADDR_BY_NAME()` and the binding stays trivial. Baochip's
  packed register blocks (`bao1x_peri.h`) can instead expose a single `reg`
  base + driver-internal offsets; either works.
- SoC-level dtsi goes to `dts/riscv/<vendor>/*.dtsi` in-tree
  (`dts/riscv/riscv32-litex-vexriscv.dtsi`, `dts/riscv/gd/gd32vf103.dtsi`,
  `dts/riscv/renode_riscv32_virt.dtsi`); the cpu node carries
  `clock-frequency`, `riscv,isa-*` (riscv32-litex-vexriscv.dtsi:16-25). A
  module can supply these instead via `settings: {dts_root: .}` (hal_nxp ships
  `dts/nxp/`, hal_stm32 ships `dts/st/<series>/` — board dts then does
  `#include <st/f4/stm32f407Xg.dtsi>` resolved against module roots). Bindings
  *can* technically ship the same way, but every major hal keeps bindings
  beside the drivers in the zephyr tree — do that.

## 7. Recommended file-by-file skeleton

### Phase 1 — zephyr fork only (`~/src/zephyr-baochip`, no hal_bao)

```
dts/bindings/vendor-prefixes.txt                          # add "baochip" vendor line
dts/riscv/baochip/bao1x.dtsi                              # cpu@0 (rv32i + i,m,a,c,zicsr,zifencei; clock-frequency),
                                                          # intc@<base>, uart/timer/gpio/usb nodes, sram/flash nodes
dts/bindings/interrupt-controller/baochip,intc.yaml       # cells (irq, priority); litex,vexriscv-intc0.yaml shape
dts/bindings/serial/baochip,uart.yaml
dts/bindings/timer/baochip,timer.yaml
dts/bindings/gpio/baochip,gpio.yaml                       # gpio-controller.yaml include, [pin, flags] cells
soc/baochip/Kconfig                                       # if SOC_FAMILY used; else skip (flat: single dir)
soc/baochip/bao1x/soc.yml                                 # socs: [{name: bao1x}]
soc/baochip/bao1x/Kconfig.soc                             # SOC_BAO1X; SOC "bao1x"
soc/baochip/bao1x/Kconfig                                 # select RISCV, INCLUDE_RESET_VECTOR; cache selects from DT
soc/baochip/bao1x/Kconfig.defconfig                       # NUM_IRQS, SYS_CLOCK_HW_CYCLES_PER_SEC from DT,
                                                          # BUILD_OUTPUT_UF2_FAMILY_ID "0x????????" (pick + register)
soc/baochip/bao1x/CMakeLists.txt                          # source common vector.S/soc_irq.S; SOC_LINKER_SCRIPT=arch/riscv/common/linker.ld
soc/baochip/bao1x/soc.h                                   # mem map macros; include vendored bao1x_peri.h here or inline
soc/baochip/bao1x/include/bao1x_peri.h                    # vendored generated register header (SPDX-tagged)
drivers/interrupt_controller/intc_bao1x.c                 # arch_irq_* + IRQ_CONNECT(RISCV_IRQ_MEXT) dispatch (vexriscv model)
drivers/interrupt_controller/Kconfig.baochip              # depends on DT_HAS_BAOCHIP_INTC_ENABLED
drivers/serial/uart_bao1x.c (+ CMakeLists/Kconfig hook)   # minimal polling + IRQ uart
drivers/timer/baochip_timer.c (+ Kconfig.baochip in drivers/timer, default y on DT compat)
drivers/gpio/gpio_bao1x.c                                 # for buttons/headers
drivers/usb/device/bao1x_udc.c                            # later: zephyr_udc0 for CDC-ACM console (biggest item)
boards/baochip/dabao/board.yml                            # name dabao, vendor baochip, socs: [bao1x]
boards/baochip/dabao/dabao.dts                            # include bao1x.dtsi; chosen (uart now, cdc-acm later);
                                                          # gpio-keys (2 buttons), connector nodes for headers
boards/baochip/dabao/Kconfig.dabao                        # config BOARD_DABAO; select SOC_BAO1X
boards/baochip/dabao/dabao_defconfig                      # CONSOLE/SERIAL/UART_CONSOLE (+XIP/boot-address realities)
boards/baochip/dabao/board.cmake                          # uf2 flasher + board-id; optional openocd; renode later
boards/baochip/dabao/dabao.yaml                           # twister: arch riscv, toolchain zephyr, ram/flash, supported
boards/baochip/dabao/doc/index.rst                        # for upstream contribution
boards/renode-style support/bao1x.resc (optional)         # Renode sim platform for CI
```

Rationale per area: SoC = litex_vexriscv minus CSR intc (bao intc is MMIO);
board = renode_riscv32_virtual + uf2.board.cmake + buttons; drivers follow the
DT-first, `DT_INST_REG_ADDR_BY_NAME`/offset style the litex drivers use, so the
generated `bao1x_peri.h` is only consulted for bitfield details.

### Phase 2 (optional) — `~/src/hal_bao` module

```
hal_bao/
├── README.md, LICENSE, LICENSES/Apache-2.0.txt, REUSE.toml
├── zephyr/module.yml          # name: hal_bao; build: {cmake: ., kconfig: zephyr/Kconfig, settings: {dts_root: .}}
├── CMakeLists.txt             # zephyr_include_directories(include); guarded by CONFIG_HAS_BAO_HAL
├── zephyr/Kconfig             # config ZEPHYR_HAL_BAO_MODULE / HAS_BAO_HAL (glue pattern from modules/Kconfig.stm32:6-14)
├── include/bao1x_peri.h       # vendored generated header (moves out of soc/ dir)
├── svd/bao1x.svd              # vendor SVD snapshot, for tooling/reference
├── dts/baochip/bao1x.dtsi     # (optional move from zephyr tree)
├── openocd/bao1x.cfg          # debug adapter config referenced by board.cmake
└── tools/                     # UF2 bootloader assets / bio-loader integration notes
```

Plus one entry in the fork's `west.yml` projects list
(`path: modules/hal/bao`, `groups: [hal]`, pinned revision) — mirroring
west.yml:174-287.

### Risks

1. **USB device driver**: CDC-ACM console and any app USB function depend on a
   from-scratch `zephyr_udc0` driver for baochip's USB peripheral; the
   bootloader's UF2 MSC stack isn't reusable in-app. Until then console = UART
   (or Segger RTT-style fallback: none in-tree for riscv, so keep a UART pad).
2. **Boot/memory layout**: where boot1 jumps, whether Zephyr runs XIP or from
   RAM, and `CONFIG_INCLUDE_RESET_VECTOR`/linker placement need pinning against
   the boot chain (see [01-boot-delivery](01-boot-delivery.md)).
3. **IRQ semantics**: vexriscv's LiteX intc is edge/pending-register based;
   confirm baochip's intc mask/pending semantics (level vs edge ack) before
   writing `intc_bao1x.c`, else subtle dropped-IRQ bugs.
4. **Upstream hygiene**: vendor-prefix, REUSE/SPDX on the generated header,
   `doc/index.rst`, twister metadata — cheap if done from the first commit,
   annoying to retrofit.

## Sources

- `~/src/zephyr-baochip/doc/hardware/porting/soc_porting.rst:36-49,57-105` — SoC dir layout, mandatory files
- `~/src/zephyr-baochip/doc/hardware/porting/board_porting.rst:252-315,500-660` — HWMv2 board files, Kconfig roles, USB console guidance
- `~/src/zephyr-baochip/doc/develop/modules.rst:507-560,905-960` — module.yml cmake/kconfig, build settings roots
- `~/src/zephyr-baochip/doc/develop/west/zephyr-cmds.rst:558` — `riscv64-zephyr-elf` SDK toolchain
- `soc/litex/litex_vexriscv/` — `CMakeLists.txt:7-14`, `Kconfig:6-12`, `Kconfig.defconfig:6-11`, `Kconfig.soc:6-16`; `soc/litex/soc.yml:1-7`; `soc/litex/common/soc.h`, `reboot.c`; `soc/litex/Kconfig:4-27`
- `soc/common/riscv-privileged/` — `vector.S:45-102`, `soc_irq.S:21-47`, `soc_common_irq.c:53-262`, `Kconfig:5-10`
- `soc/renode/riscv_virtual/` — `Kconfig:6-11`, `Kconfig.defconfig:6-40`, `Kconfig.soc:6-10`, `soc.yml`, `CMakeLists.txt`
- `soc/neorv32/` — `Kconfig:6-27`, `soc_irq.S:12-15`, `CMakeLists.txt:8-13`, `soc.yml`
- `soc/espressif/esp32c3/Kconfig:6-13`, `soc_irq.S:13-15` — non-privileged-spec vendor SoC
- `arch/riscv/Kconfig:126-139,177-181,665-687`; `arch/riscv/Kconfig.isa:4-5+`; `arch/riscv/custom/openisa/ri5cy/CMakeLists.txt:8`
- `cmake/compiler/gcc/target_riscv.cmake:4-100`; `include/zephyr/arch/riscv/irq.h:45`
- `drivers/interrupt_controller/intc_vexriscv_litex.c:21-32,52-68,71-93`; `dts/bindings/interrupt-controller/litex,vexriscv-intc0.yaml`
- `drivers/timer/Kconfig.litex:6-12`, `drivers/timer/litex_timer.c:13-20,48-70`; `drivers/timer/Kconfig.riscv_machine:8-14`
- `dts/riscv/riscv32-litex-vexriscv.dtsi:16-42`; `dts/riscv/gd/gd32vf103.dtsi:27-28`; `dts/bindings/{serial/litex,uart.yaml,gpio/litex,gpio.yaml,timer/litex,timer0.yaml}`
- `boards/enjoydigital/litex_vexriscv/` — `board.yml`, `Kconfig.litex_vexriscv`, `litex_vexriscv_defconfig`, `litex_vexriscv.dts:47-70`, `litex_vexriscv.yaml`
- `boards/renode/riscv32_virtual/` — `board.yml:6-9`, `board.cmake:3-6`, `riscv32_virtual_defconfig`, `riscv32_virtual.yaml`
- `boards/common/usb/Kconfig.cdc_acm_serial.defconfig:1-46`, `cdc_acm_serial.dtsi:10-20`
- `boards/common/uf2.board.cmake:1-4`; `boards/raspberrypi/rpi_pico/board.cmake:19-31,47`
- `Kconfig.zephyr:859-890` (BUILD_OUTPUT_UF2); `CMakeLists.txt:1884-1897` (uf2conv post-build); `scripts/build/uf2families.json`
- `scripts/west_commands/runners/uf2.py:26-39,55-73,88-100`; `renode.py:20-27`; `boards/common/renode.board.cmake:1-8`
- `soc/raspberrypi/rpi_pico/rp2040/Kconfig.defconfig:8-19`
- `~/src/zephyr-baochip/west.yml:23,174-287` (hal projects, groups); `scripts/zephyr_module.py:74-96`
- `~/archive/zephyrproject-rtos/hal_espressif/zephyr/module.yml:1-10` (+blobs), `west/west-commands.yml:1-7`
- `~/archive/zephyrproject-rtos/hal_nxp/zephyr/module.yml:1-6`; zephyr-tree glue `~/archive/zephyrproject-rtos/zephyr/modules/hal_nxp/` (git-tracked)
- `~/archive/zephyrproject-rtos/hal_stm32/zephyr/module.yml:1-5`, `CMakeLists.txt:9-10`, `dts/st/`, `LICENSES/`, `REUSE.toml`
