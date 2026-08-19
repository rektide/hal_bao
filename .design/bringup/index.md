---
type: Index
title: Baochip Dabao bring-up
description: Current evidence, procedures, and architecture for bringing Zephyr up on Baochip 1x hardware.
resource: /.design/bringup/index.md
tags: [baochip, dabao, zephyr, bringup]
status: draft
generated: { by: agent:opencode-gpt56, at: 2026-08-19 }
sources:
  - id: bringup-session
    resource: /.design/bringup/s2nm5b-baseline.md
    title: Dabao S2NM5B baseline
  - id: research
    resource: /.design/research/README.md
    title: Baochip Zephyr research corpus
---

# Dabao bring-up

This directory is the current public entry point for physical Baochip 1x and
Dabao bring-up. It separates stable architecture, repeatable operations, and
device-specific evidence. Historical research explains how decisions were
reached; scratch bundles preserve exact build artifacts but are not the source
of current procedure.

## Start here

- [`architecture.md`](architecture.md) — boot chain, memory ownership, image
  delivery, USB/UART handoff, recovery boundaries, and Zephyr's current
  implementation.
- [`procedure.md`](procedure.md) — guarded, version-aware procedure for audit,
  artifact validation, installation, observation, and recovery.
- [`s2nm5b-baseline.md`](s2nm5b-baseline.md) — read-only evidence captured from
  the first connected Dabao on 2026-08-19.
- [`../research/README.md`](../research/README.md) — detailed source research
  for the SoC, boot flow, Zephyr integration, interrupts, timer, and device
  ownership.

## Evidence ledger

Claims are intentionally narrow. A successful host build or simulated run is
not evidence that a physical Dabao executed the same image.

| Gate | Claim | Status | Evidence |
|---|---|---|---|
| H0 | RV32IMAC toolchain compiles and links a Baochip ELF | Proven | Local `.test-agent/toolchain` bundle; conclusion retained here |
| H1 | Image pack/sign/inspect and UF2 protocol tests pass | Proven | `cargo test --workspace`, 45 tests on 2026-08-19 |
| S0 | Zephyr reaches `main()` and prints through DUART in Verilator | Proven for preserved M2 binary | Local `.test-agent/m2-console-validation` bundle; conclusion retained here |
| S1 | Banked irqarray software events dispatch correctly in Verilator | Proven for documented scenarios | Zephyr fork `tests/soc/baochip/irqarray/README.md` |
| S2 | Accepted current ticktimer implementation runs in RTL simulation | Not proven | Current representative runs time out during takeover; native logic and build matrices pass |
| P0 | A physical Dabao enters boot1 and exposes CDC, MSC, and audit | Proven on `S2NM5B` | [`s2nm5b-baseline.md`](s2nm5b-baseline.md) |
| P1 | A signed Zephyr UF2 persists and validates on physical hardware | Not attempted | Requires pre/post-write audit |
| P2 | Zephyr reaches `main()` and emits physical UART output | Not attempted | Current board console is UDMA UART2 on PB14/PB13 |
| P3 | Zephyr reinitializes USB and enumerates CDC-ACM after boot1 | Not implemented | Baochip Zephyr UDC driver required |
| P4 | Timer-driven sleep and preemption pass on physical hardware | Not attempted | Run only after a decisive console exists |
| R0 | A compatible Xous image set restores the board | Not demonstrated | Same-batch boards preserve factory reference state |

## Current priorities

1. Implement the Baochip Corigine USB device controller as a proper Zephyr UDC
   and expose stock CDC-ACM on Dabao.
2. Use USB re-enumeration and console output as the decisive no-extra-hardware
   proof that Zephyr reached `main()`.
3. Validate signed-image persistence with before/after boot1 audits.
4. Resolve current ticktimer runtime evidence, then run the kernel sleep and
   preemption validation on silicon.

## Source-of-truth policy

- This directory owns current architecture, status, and operator procedure.
- [`/doc/bringup/manual-validation.md`](/doc/bringup/manual-validation.md) is a
  historical M1 artifact procedure; its silent-image expectations are not a
  statement of current platform capability.
- `.test-agent/` records are local evidence and exact artifact provenance.
  Important conclusions must be restated here because that directory is
  gitignored.
- [`../research/`](../research/) remains the detailed rationale and source
  analysis. Where old research conflicts with observed hardware or newer
  adjudication, this directory records the current conclusion.
