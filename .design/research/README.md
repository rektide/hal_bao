# Research

The wave-0 research corpus behind the port. Written independently by four
agents, synthesized in `04-synthesis.md`. The durable home for these is the
shared doc repo (`~/a/doc/bao/`); this copy keeps the effort self-contained.

Read `04-synthesis.md` first; the rest are its sources.

- [`00-soc-inventory.md`](00-soc-inventory.md) — CPU (VexRiscv RV32IMAC+Zkn,
  Sv32), memory map, peripheral table with base addresses, custom interrupt
  model, clocks, Dabao board.
- [`01-boot-delivery.md`](01-boot-delivery.md) — boot0/boot1 chain, baremetal
  slot at `0x60060400`, JAL-chain entry, signature block and devkey signing,
  UF2 constants, serial upload, verilator sim, JTAG status.
- [`02-zephyr-integration.md`](02-zephyr-integration.md) — SoC-port anatomy
  (litex_vexriscv precedent), HWMv2 board port, hal_* module analysis, UF2 and
  Renode runners, rv32imac toolchain/ISA configuration.
- [`03-rust-survey.md`](03-rust-survey.md) — Rust support in Zephyr 4.5-dev
  (app-level only), why Rust drivers aren't supportable, utralib/svd2utra
  reuse.
- [`04-synthesis.md`](04-synthesis.md) — decision table, phased bring-up plan
  (M0–M5), risk register, open questions.
- [`05-lifecycle-delivery-validation.md`](05-lifecycle-delivery-validation.md)
  — device audit preflight, developer mode and one-way counters, boot1 transport
  modes, serial uploader progression, observable Dabao outputs, and runner
  scope.
- [`06-irq-ack-semantics.md`](06-irq-ack-semantics.md) — RTL-proven irqarray
  edge/level behavior, positive enable polarity, W1C ordering, lost-event
  constraints, Xous sequences, and the concrete Zephyr dispatch algorithm.

Synced from `~/a/doc/bao/` 2026-08-14.
