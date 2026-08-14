---
type: Design # kind of knowledge
title: hal_bao init — why and how Zephyr comes to Baochip
description: The narrative arc of porting Zephyr to the Baochip 1x SoC and Dabao board: the problem, the terrain, the strategy, and the shape of the work.
resource: file:///home/rektide/src/hal_bao/.design/init/init.md
tags: [baochip, dabao, zephyr, riscv, porting]
status: draft
generated: { by: agent:opencode, at: 2026-08-14 }
sources:
  - id: research-corpus
    resource: file:///home/rektide/archive/doc/bao/README.md
    title: Baochip research corpus (docs 00-04)
  - id: baochip-1x
    resource: https://github.com/baochip/baochip-1x
    title: Baochip 1x chip repo (RTL, docs, generated headers)
  - id: xous-core
    resource: https://github.com/betrusted-io/xous-core
    title: Xous OS — the only existing OS and the reference implementation
---

# init: Zephyr on Baochip — the story of the effort

## What is going on

Baochip 1x is a real, taped-out, open-ish RISC-V SoC from the bunnie/betrusted
ecosystem: a VexRiscv RV32IMAC(+Zkn) application core with Sv32, 4 MiB of XIP
RRAM, 2 MiB SRAM, four PicoRV32 "BIO" coprocessors, a serious crypto engine,
and a pile of custom peripherals — bootstrapped by an ed25519 verified boot
chain that ends in a USB mass-storage bootloader speaking UF2. The Dabao board
puts exactly that chip, two buck regulators, a USB-C port, and two buttons on
a PCB. Nothing else.

We want to run [Zephyr](https://zephyrproject.org) on it.

## Why this is a whole-SoC port, not a board port

The previous effort in this workspace (`zephyr-crosspoint`, the XTEINK X4) was
a *board* port: an ESP32-C3 SoC already had a mature Zephyr SoC layer, a
vendor HAL, standard CLINT/UART/flash IP, and an established flashing tool.
The work was devicetree, board Kconfig, and panel glue.

Baochip has none of that scaffolding, because the chip's entire software world
is [Xous](https://github.com/betrusted-io/xous-core), a microkernel OS written
in Rust. There is no vendor C SDK, no CMSIS pack, no existing Zephyr anything.
What does exist is unusually good *raw material*:

- The chip RTL is public, and a headergen flow already extracts LiteX-style C
  register headers, an SVD, and per-peripheral documentation from it.
- Xous is a complete, working behavioral specification of every peripheral —
  in Rust, with linker scripts that pin down the memory map.
- The boot chain is documented and developer-friendly: a public devkey lets
  anyone sign images, delivered by copying a UF2 file onto a USB drive.

So the task is to write a Zephyr SoC port from scratch — interrupt controller,
system clock, console, GPIO — using generated headers as the register truth
and Xous as the behavioral reference, then hang a minimal Dabao board
definition off it, and build the image tooling that gets a signed kernel onto
the chip. That is the difference in kind: we are not configuring an SoC, we
are standing one up.

## The terrain (what the research established)

Four research documents ([`.design/research/`](../research/README.md); the
durable copy lives in the shared doc repo, `~/a/doc/bao/`) map the terrain;
`04-synthesis.md` condenses them. The load-bearing facts:

1. **The core is standard-shaped, the system is not.** RV32IMAC with M/S/U and
   Sv32, but no CLINT and no PLIC. Interrupts arrive through a custom
   ExternalInterruptArray (custom CSRs `MIM`/`MIP` at 0xBC0/0xFC0) fronting 20
   event banks, and time comes from a 64-bit MMIO "ticktimer", not the machine
   timer. Zephyr's stock RISC-V timer and IRQ paths do not apply; the
   `litex_vexriscv` SoC port is the precedent that proves this shape works.
2. **The boot slot dictates the linker.** Third-party OSes are meant to live
   in the "baremetal" region: entry at `0x60060400`, with a JAL trampoline
   quirk — the first linked instruction must be the reset code, because boot1
   jumps through fixed offsets in a signature block. RAM is ACRAM at
   `0x61000000`. At handoff the machine is in M-mode, MMU off, IRQs masked,
   at ~350 MHz, with UDMA UART2 already alive on PB14/PB13 at 1 Mbaud.
3. **Tooling is the hidden work.** Images are ed25519-signed
   (`xous-sign-image --function-code baremetal`, devkey in slot 3) and wrapped
   in UF2 with family id `0xa7d76373`. The stock Xous packing tool strips
   `.data` (it pokes ≤40 words into RAM) — useless for Zephyr — so we must
   write a packer that embeds `.data` in ROM and lets the kernel self-copy.
4. **Debug is UART-or-simulation.** JTAG is fused out on production parts.
   The physical 1 Mbaud UDMA UART2 console and the verilator RTL simulation's
   TX-only DUART output — which happily runs arbitrary RV32IMAC images, boot
   chain bypassed — are the entire debug story. Dabao leaves the dedicated
   DUART package pin unconnected.
5. **Rust is out (for now), but not wasted.** Zephyr 4.5-dev's Rust story is
   app-level only. The port is all C. But Xous's driver crates are the
   reference implementation for every peripheral, and its utralib register
   map cross-checks our vendored headers.

## The strategy

- **Two repos, one effort.** Zephyr code goes in the `zephyr-baochip` fork as
  *new files only* (`soc/baochip/`, `drivers/`, `dts/`, `boards/baochip/dabao/`)
  so upstream rebases stay clean. Everything that is not Zephyr code — vendored
  generated headers, image tooling, tickets, design docs — lives here in
  `hal_bao`. This repo is the home of the effort; the fork is where the effort
  ships. A `zephyr/module.yml` stub keeps the door open to moving the port
  fully out-of-tree later if that trade changes.
- **Copy the litex_vexriscv shape.** Same CPU core, same no-CLINT/PLIC
  situation, minimal file set. Bao's interrupt controller is even simpler
  (MMIO ack plus two custom CSRs).
- **Bring-up in milestone order** (M0–M5 below), each gated by something you
  can see: a linked image that fits the slot; a signable/flashable UF2; a
  console printing; a ticking clock; a board tree that builds `hello_world`;
  UDMA serial peripherals.
- **Xous is the spec.** When register semantics are ambiguous (edge vs level
  ack, ticktimer alarm ordering, IOX AFSEL encodings), the answer is "read the
  Xous driver, mirror it, then test on sim or silicon."

## The shape of the work (M0–M5)

- **M0 — scaffolding.** Vendor the generated header/SVD (done at repo birth),
  define the `baochip` DT vendor prefix, toolchain smoke test
  (`riscv64-zephyr-elf`, `rv32imac_zicsr`).
- **M1 — boot to main.** SoC skeleton, the `0x60060400` linker layout with
  reset-first linking, the `.data`-capable image packer + devkey signer in
  `tools/`, first UF2 copied to a `BAOCHIP` volume. Exit: early shell.
- **M2 — a real OS.** DUART simulation console, polling UDMA UART2 hardware
  console, irqarray intc driver, ticktimer sys_clock. Exit: preemptive
  scheduling demonstrable.
- **M3 — a board.** `boards/baochip/dabao` HWMv2 tree, IOX GPIO, CMSDK WDT,
  PL031 RTC, button + UART "blinky" (there are no LEDs on a Dabao).
- **M4 — comms.** UDMA descriptor-DMA subsystem, interrupt-driven UART
  console, optional serial (boot1 REPL) uploader for CI-less flashing.
- **M5 — the long tail.** USB CDC-ACM (Corigine controller — hard, deferred),
  a Rust app sample via `zephyr-lang-rust`, BIO/SCE drivers as custom
  subsystems, Renode platform, and the hal_bao split-out decision.

Risks and open questions live in `04-synthesis.md`; the ticket graph lives in
`.beads/` as `bao-*` issues mirroring these milestones.

## Where this is goingIf M1–M3 land, Baochip becomes a first-class Zephyr target with a story no
other silicon has: an open RTL, an open secure-boot chain, and two operating
systems — one microkernel written in Rust, one RTOS written in C — sharing one
honest set of generated hardware descriptions. Everything after that
(networking over UDMA, crypto services on the SCE, BIO coprocessor offload,
upstreaming) builds on that foundation.

## Cross-references

- [`../research/README.md`](../research/README.md) — the wave-0 research corpus
- [`../research/04-synthesis.md`](../research/04-synthesis.md) — decisions table, risk register, open questions
- [`../research/00-soc-inventory.md`](../research/00-soc-inventory.md) — memory map, IRQ table, peripheral details
- [`../research/01-boot-delivery.md`](../research/01-boot-delivery.md) — signature block, JAL chain, UF2, sim
- [`../research/02-zephyr-integration.md`](../research/02-zephyr-integration.md) — SoC/board file skeletons, module analysis
- [`../research/03-rust-survey.md`](../research/03-rust-survey.md) — Rust boundaries and reuse
- [`../../include/PROVENANCE.md`](../../include/PROVENANCE.md) — vendored header provenance and regen path
