---
type: Research
title: Baochip lifecycle and delivery validation
description: Device-state preflight, one-way counter policy, boot1 transport modes, observable Dabao outputs, and runner scope for Zephyr bring-up.
tags: [baochip, dabao, boot1, lifecycle, uf2, zephyr]
status: draft
generated: { by: agent:opencode, at: 2026-08-14 }
sources:
  - id: xous-bootchain
    resource: https://github.com/betrusted-io/xous-core/blob/dev/bao1x-boot/BOOTCHAIN.md
    title: Bao1x Boot Chain
  - id: xous-boot1
    resource: https://github.com/betrusted-io/xous-core/tree/dev/bao1x-boot/boot1
    title: Baochip boot1 source
  - id: dabao-hardware
    resource: https://github.com/baochip/dabao
    title: Dabao hardware design
---

# Baochip lifecycle and delivery validation

This note resolves bring-up questions that became concrete once a signed
Zephyr UF2 existed: what can be observed on Dabao, how to inspect an unknown
device before entering developer mode, which writes consume one-way counters,
and whether a custom Zephyr runner is critical-path work.

The checked-out Xous `dev` revision, its `origin/dev`, and the live remote were
all `5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b` during this review.

## Observable hardware

Dabao has no user LED. Its schematic `D1` is a Schottky diode, and the board is
otherwise intentionally minimal. An external LED with a resistor or an
oscilloscope can eventually observe an exposed IOX GPIO, but no current Zephyr
artifact configures one.

Baochip's TX-only DUART drives a dedicated `PAD_DUART`; Dabao leaves that
package pin unconnected. DUART remains valuable in Verilator, where its RTL
prints completed lines. Physical Dabao serial uses **UDMA UART2** on PB14 TX and
PB13 RX at 1,000,000 baud, 8-N-1. This distinction is visible in
[`pad_frame_arm.sv`](https://github.com/baochip/baochip-1x/blob/main/rtl/asic_top/rtl/pad_frame_arm.sv#L218-L225),
the Dabao PCB's unconnected `U1B-DUART-PadD3`, and Xous's
[`README-consoles.md`](https://github.com/betrusted-io/xous-core/blob/dev/README-consoles.md#L5-L16).

The first decisive hardware artifact therefore needs a minimal UDMA UART2
console. The current silent image can prove only that boot1 accepted the image
and attempted a handoff.

## Boot1 modes

Boot1 owns three usable delivery/control paths before handoff:

| Path | Behavior |
|---|---|
| USB MSC | Synthetic FAT volume labeled `BAOCHIP`; addressed UF2 sectors are written to RRAM |
| USB CDC-ACM | Boot1 REPL, including `audit` and per-block `uf2 <base64>` upload |
| Physical UDMA UART2 | The same REPL on PB13/PB14 when USB does not own the console |

USB CDC and physical UART are transports for one REPL, not independent
protocols. Boot1 prefers USB after a valid connection. The update modes are
documented in [`BOOTCHAIN.md`](https://github.com/betrusted-io/xous-core/blob/dev/bao1x-boot/BOOTCHAIN.md#L52-L69).

Xous already supplies
[`uf2send.py`](https://github.com/betrusted-io/xous-core/blob/dev/bao1x-boot/uf2send.py).
It sends one base64-encoded 512-byte UF2 block at a time, waits for a matching
size/address acknowledgment, and retries failed blocks. Zephyr integration
should adopt or thinly wrap this implementation rather than invent another
serial protocol.

That was the upstream precedent when this note was written. The recommendation
has since been implemented locally as two Rust packages: the reusable
`bao-boot1-protocol` library and the
[`bao-uf2send`](/tools/uf2send/README.md) serial binary. The library preserves
the upstream REPL behavior while adding complete canonical-image preflight and
`has-crc` negotiation; the binary opens either USB CDC-ACM or physical UART2.
The split keeps serial-port policy out of the protocol library.

After handoff, none of these boot1 services remain available unless Zephyr
implements the corresponding USB or UART driver.

## Inspect before changing the device

Enter boot wait with the physical PROG button, connect to boot1 USB CDC, run
`audit`, and retain the complete output before writing an image. `audit` reports:

- board type and primary/alternate boot selection;
- boot1 revision, silicon stepping, serial, and UUID;
- both paranoid and require-PQ counter values;
- primary key revocations for boot0, boot1, and the next stage;
- signature/key/PQ validation of boot0, boot1, and the current next stage;
- developer-mode, provisioning, fuse, collateral, and receipt warnings; and
- boot1's anti-rollback match.

The implementation is in
[`audit.rs`](https://github.com/betrusted-io/xous-core/blob/dev/bao1x-boot/boot1/src/audit.rs#L66-L170).
Its compact revocation table explicitly omits duplicate revocation counters, so
host tooling must not claim complete device acceptance from this output alone.

For the current classical developer artifact, require:

- `Board type reads as: Dabao`;
- `PQ required: 0/0`; and
- next-stage key slot 3 shown as enabled.

Absence of `== IN DEVELOPER MODE ==` means the next valid developer-key boot
will irreversibly erase factory secrets and enter developer mode. Presence
means that transition already happened.

Do not use inspection as an excuse to run lifecycle-changing commands.
`lockdown`, `require-pq confirm`, `altboot`, `boardtype`, `baosec-init`, and
`self_destruct` are not part of Zephyr provisioning.

## Developer mode and provisioning

A valid developer-key match automatically erases writable factory secret slots
and advances sticky `DEVELOPER_MODE`. The first transition may require a reboot
and cannot be undone
([`BOOTCHAIN.md`](https://github.com/betrusted-io/xous-core/blob/dev/bao1x-boot/BOOTCHAIN.md#L18-L23)).
The implementation avoids rewriting already-erased slots and caps repeated
developer-state advancement at 15.

No manual provisioning is required to run a developer image on a normal SKU
whose developer key remains enabled. Factory CP setup is automatic and visible
in `audit`. `baosec-init` is product initialization that can erase external
storage; it is not a Dabao development prerequisite.

`lockdown` is much broader than disabling developer mode. After validating that
boot1 is not developer-signed, confirmation revokes classical and PQ developer
keys for all boot stages and enables paranoid and require-PQ policy. Boot1
implements a separate uppercase `YES` confirmation
([`repl.rs`](https://github.com/betrusted-io/xous-core/blob/dev/bao1x-boot/boot1/src/repl.rs#L97-L131)).
Do not invoke it during bring-up.

## One-way counters and repeated testing

There is no general firmware-install attempt counter. Invalid or unsigned
images cannot consume anti-rollback endurance because signature verification
precedes counter handling
([`sigcheck.rs`](https://github.com/betrusted-io/xous-core/blob/dev/libs/bao1x-hal/src/sigcheck.rs#L264-L303)).

Each firmware function has a separate anti-rollback counter. `bao-image`
currently signs baremetal images at anti-rollback value 1. The first accepted
image can advance the baremetal counter from 0 to 1; repeated builds at value 1
do not advance it. Do not use build numbers as anti-rollback values.

Counter allocation is fixed: each counter owns a 32-byte RRAM line. There is no
provisioning option to give one counter more space. Source comments describe a
conservative 10,000-increment wear limit, while `ONEWAY_MAX_VALUE` is currently
spelled `10_0000` (100,000); development should depend on neither. One signed
image may advance anti-rollback by at most 511.

Use the physical PROG button rather than repeatedly toggling `bootwait`.
Board-type, alternate-boot, boot-wait, PQ requirement, revocation, and paranoid
settings each consume their own one-way state only when explicitly changed.

## Runner scope

A small Baochip-specific runner has real precedent, but is not the immediate
bring-up bottleneck:

- Zephyr's generic UF2 runner discovers `INFO_UF2.TXT`; boot1 exposes a volume
  label but no such file, so the generic runner cannot find Dabao.
- Rejecting `ALTCHIP`, ambiguous targets, wrong family/address, and unsigned
  Zephyr output is concrete safety work.
- Zephyr runners normally delegate format/device validation to vendor tools.
  Default-dry-run behavior and mandatory custom acknowledgments are policy
  inventions, not established runner conventions.
- Host validation cannot predict device-local revocation, anti-rollback, and PQ
  policy. Boot1 `audit` remains the preflight authority.

Therefore canonical UF2 inspection and a thin explicit-target MSC runner were
identified as P2 conveniences, while hardware-visible UDMA UART2 output
remained P1. The serial-upload recommendation was to reuse Xous `uf2send.py`;
the subsequent `bao-boot1-protocol` and `bao-uf2send` implementation now does
so at the protocol level rather than leaving serial upload unintegrated.

The current sender preflights the entire image before opening a port, probes
`has-crc`, requires exact size/address acknowledgments (and CRC when supported),
and bounds attempts per block. These host guarantees do not supersede boot1
`audit`, which remains necessary for device-local key, anti-rollback, and PQ
policy. The sender now fails on a reported `Write error` even when affected
boot1 versions subsequently print `Wrote`. Acknowledgments still do not
independently prove RRAM persistence against power loss or silent corruption,
so installation needs independent verification.

## Cross-references

- [`/doc/bringup/manual-validation.md`](/doc/bringup/manual-validation.md) - operator procedure derived from this lifecycle model.
- [`/tools/uf2send/README.md`](/tools/uf2send/README.md) - current USB CDC and physical-UART CLI, protocol guarantees, and persistence limitation.
- [`/tools/README.md`](/tools/README.md) - image-tool and two-package serial-delivery architecture.
- [`/.design/research/01-boot-delivery.md`](/.design/research/01-boot-delivery.md) - image format, slot layout, signing, and handoff mechanics.
- [`/.design/research/04-synthesis.md`](/.design/research/04-synthesis.md) - overall milestone plan and risk register.
- [`/include/PROVENANCE.md`](/include/PROVENANCE.md) - generated register source and revision.
