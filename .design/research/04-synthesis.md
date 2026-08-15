---
type: Design # kind of knowledge
title: Baochip 1x / Dabao Zephyr port — synthesis & recommended plan
description: Synthesis of the four research docs into a phased bring-up plan for Zephyr on Baochip 1x (Dabao board).
resource: file:///home/rektide/src/hal_bao/.design/research/04-synthesis.md
tags: [baochip, dabao, zephyr, porting, riscv]
status: draft
generated: { by: agent:opencode, at: 2026-08-14 }
sources:
  - id: 00-soc-inventory
    resource: file:///home/rektide/src/hal_bao/.design/research/00-soc-inventory.md
    title: SoC inventory
  - id: 01-boot-delivery
    resource: file:///home/rektide/src/hal_bao/.design/research/01-boot-delivery.md
    resource_note: boot chain, image format, signing, emulation
    title: Boot & delivery
  - id: 02-zephyr-integration
    resource: file:///home/rektide/src/hal_bao/.design/research/02-zephyr-integration.md
    title: Zephyr integration mechanics
  - id: 03-rust-survey
    resource: file:///home/rektide/src/hal_bao/.design/research/03-rust-survey.md
    title: Rust survey
---

# Synthesis: Zephyr on Baochip 1x

Cross-references: [00-soc-inventory](00-soc-inventory.md) ·
[01-boot-delivery](01-boot-delivery.md) · [02-zephyr-integration](02-zephyr-integration.md) ·
[03-rust-survey](03-rust-survey.md)

## The system in one paragraph

Baochip 1x (BAO1X2S4F-WA) is a VexRiscv RV32IMAC(+Zkn) SoC with M/S/U + Sv32,
4 MiB XIP RRAM, 2 MiB ACRAM, custom peripherals behind a LiteX-style CSR space,
4× PicoRV32 "BIO" coprocessors, and a rich SCE crypto block. It boots through an
ed25519 verified chain (boot0 → boot1) that ends in a USB MSC bootloader; images
are UF2 files (family `0xa7d76373`) signed with a public devkey. Dabao is a
minimal SoM (chip + 2 bucks + USB-C + RST/PROG buttons, 48 MHz crystal, no LEDs).
The only existing OS is Xous (Rust); a `baremetal` boot slot exists precisely
for non-Xous images — Zephyr's home.

## Decisions (recommended)

| # | Decision | Choice | Rationale |
|---|----------|--------|-----------|
| D1 | Language | **All C**; Xous Rust as reference | Zephyr 4.5-dev has no Rust driver story; app-level Rust possible later via `zephyr-lang-rust` (03) |
| D2 | hal_bao module | **Defer** — start in-tree only | No vendor C SDK to mirror; only artifacts are generated `bao1x_peri.h` + SVD; litex/neorv32 precedent (02 §3). Create hal_bao later if regen cadence/multi-consumer emerges |
| D3 | Boot slot | **baremetal region**: ROM `0x60060400`, RAM `0x61000000` (2 MiB) | Xous kernel occupies `0x6009FD00+`; baremetal slot is the intended third-party OS slot (01) |
| D4 | SoC port model | **Copy `soc/litex/litex_vexriscv` shape**: no CLINT/PLIC required | Same core; bao's intc is even simpler (MMIO ack + custom CSRs MIM/MIP 0xBC0/0xFC0) (00 §4, 02 §1) |
| D5 | sys_clock | **ticktimer** MMIO driver @ `0xE001_B000` (64-bit, one-shot alarm, IRQ 20) | Machine timer doesn't exist; this is Xous's own kernel clock (00 §3) |
| D6 | Console | **DUART @ `0x40042000` for simulation**, minimal polling **UDMA UART2 @ `0x50103000` for Dabao hardware**, interrupt-driven UDMA second | DUART is TX-only and easy in Verilator, but Dabao leaves its dedicated package pad unconnected. PB14/PB13 are UDMA UART2 at 1 Mbaud (00 §3.2) |
| D7 | Flashing | **UF2 runner** with `CONFIG_BUILD_OUTPUT_UF2_FAMILY_ID=0xa7d76373`; devkey signing via `xous-sign-image --bao1x --with-jump --sig-length 768 --function-code baremetal --loader-key devkey/dev.key` | Runner is family-agnostic; copies to any `INFO_UF2.TXT` MSC mount (02 §4, 01 §4) |
| D8 | CI/emulation | **Verilator RTL sim** (runs arbitrary RV32IMAC, chain bypassed) + later Renode platform if worthwhile | No Renode bao1x platform exists today (01 §8) |
| D9 | Blinky | **UART + button** (no LEDs on Dabao); GPIO toggle on header pads via IOX | 00 §7 |

## Phased bring-up

### M0 — scaffolding (tree hygiene day 1)
- `dts/bindings/vendor-prefixes.txt`: add `baochip` prefix; REUSE/spdx headers.
- Vendor `bao1x_peri.h` (+ SVD) into `soc/baochip/bao1x/include/` (regen path documented).
- Toolchain check: Zephyr SDK `riscv64-zephyr-elf`, ISA `rv32imac_zicsr` (gd32vf103 precedent — 02 §5).

### M1 — boot to main (the JAL-chain hump)
- `soc/baochip/bao1x/`: soc.yml, Kconfig(.soc/.defconfig w/ UF2 family id), CMakeLists, soc.h, **custom linker fragment** — ROM `ORIGIN 0x60060400` `LENGTH ≈ 0x3F800`, RAM ACRAM; reset code must be the **first linked byte** (JAL chain: sigblock `jal +768`, presign[0] `0x1000006f`).
- Handoff assumptions from boot1: M-mode, MMU off, IRQs masked, a0=KERNEL_START/a1=0, ~350 MHz, DUART live, mtvec/sp = leftovers (must reset both) (01 §3).
- **Custom image packer**: stock `xous-copy-object` strips `.data` (≤40-word poke table) — build a packer that embeds `.data` LMA in ROM and Zephyr self-copies on start (01 §6). This is the single most plan-changing tooling item.
- Sign + UF2 + copy to `BAOCHIP` volume → PROG. Target: `zephyr-entry` / early shell.

### M2 — console + clock (a real OS)
- DUART poll console for Verilator plus a minimal polling UDMA UART2 console for observable Dabao output; full interrupt-driven UDMA remains M4.
- intc driver: irqarray banks + MIM/MIP custom CSRs; validate edge-vs-level ack semantics against Xous kernel usage (risk R3).
- ticktimer sys_clock driver (one-shot alarm model per `xous-ticktimer` impl).

### M3 — board completeness
- `boards/baochip/dabao/` HWMv2: board.yml, DTS (IOX gpio@0x5012_F000, duart, ticktimer, wdt, rtc, buttons on RST_N/PC13), Kconfig, board.cmake w/ uf2 runner.
- IOX GPIO driver (6 banks × 16, 2-bit AFSEL, pull-up only, INTCR/INTFR → PIOIRQ0-3).
- CMSDK WDT (@0x4004_1000, unlock 0x1ACCE551 / feed 0x5A) + PL031 RTC (@0x4006_1000) — both standard IP, cheap wins.
- hello_world + `samples/basic/button` analog.

### M4 — comms
- UDMA subsystem driver (descriptor DMA): UART2 interrupt console, then SPI/I2C/I2S channels as bindings demand.
- Second serial console via boot1-style REPL `uf2` base64 upload — optional Python flasher (bio-loader pattern) for CI-less flashing.

### M5 — optional / deferred
- USB CDC-ACM device driver for Corigine xHCI-style controller (hard, defer).
- Rust app sample via `zephyr-lang-rust` (03).
- Renode platform model.
- BIO coprocessor + SCE crypto drivers (Zephyr has no framework slot; would be custom subsystems).
- `hal_bao` module split-out if/when warranted.

## Risk register

| ID | Risk | Severity | Mitigation |
|----|------|----------|------------|
| R1 | `.data` packer gap (xous-copy-object strips it) | High | Write custom packer in M1; verify with `readelf` + sim |
| R2 | ROM budget: baremetal slot ≈ 254 KiB before Xous kernel @`0x6009FD00` | Medium | Watch size; if tight, negotiate slot layout w/ boot1 config or trim features; RRAM XIP so `.data`-in-ROM costs real space |
| R3 | irqarray edge-vs-level ack semantics undocumented | Medium | Mirror Xous kernel ack sequence exactly; test each bank in sim |
| R4 | No JTAG on production parts | Medium | Physical UDMA UART2 at 1 Mbaud plus Verilator DUART output are the debug story; early-print + sim-first workflow |
| R5 | Dev-signed image trips DEVELOPER_MODE irreversibly | Low (dev boards) | Use dedicated dev units; note in board docs |
| R6 | UF2 family id must exactly match boot1 checks (`0xa7d76373`) | Low | Encode in SoC Kconfig.defconfig; smoke test |
| R7 | Upstream hygiene (vendor prefix, REUSE, checkpatch) | Low | Do it in M0, not retrofit |

## Open questions

1. Does the 4 MiB RRAM slot layout allow a bigger baremetal region (or relocating Xous) if Zephyr outgrows 254 KiB? (Ask baochip / read boot1 slot config.)
2. Can we upstream into zephyrproject-rtos/zephyr eventually (vendor-prefix + maintainer story), or fork long-term like this tree assumes?
3. Is a Renode platform worth building for CI speed, or is verilator + hardware enough?
4. Zkn AES extension: expose via `riscv,isa-extensions` now or defer?

## Cross-references

- Memory map, IRQ table, per-peripheral register detail: [00-soc-inventory](00-soc-inventory.md)
- Signature block layout, JAL chain, UF2 constants, handoff state, sim invocation: [01-boot-delivery](01-boot-delivery.md)
- SoC/board file skeletons, hal_* module analysis, runner/toolchain mechanics: [02-zephyr-integration](02-zephyr-integration.md)
- Rust support boundaries + utralib/svd2utra reuse assessment: [03-rust-survey](03-rust-survey.md)
- Device audit, one-way counters, delivery modes, and runner scope: [05-lifecycle-delivery-validation](05-lifecycle-delivery-validation.md)
