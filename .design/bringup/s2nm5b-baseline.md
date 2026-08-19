---
type: Evidence
title: Dabao S2NM5B read-only baseline
description: Device identity, USB modes, boot1 audit, and source-backed interpretation observed before any firmware write.
resource: /.design/bringup/s2nm5b-baseline.md
tags: [baochip, dabao, evidence, boot1, s2nm5b]
status: stable
generated: { by: agent:opencode-gpt56, at: 2026-08-19 }
verified: { by: agent:opencode-gpt56, at: 2026-08-19 }
sources:
  - id: installed-boot1
    resource: https://github.com/betrusted-io/xous-core/tree/5397e1b488c081566cef2c0e597e05426f67c1c3/bao1x-boot/boot1
    title: Installed boot1 source revision
  - id: local-capture
    resource: file:///home/rektide/src/hal_bao/.test-agent/bringup0/
    title: Local raw bring-up evidence
---

# Dabao `S2NM5B` baseline

This document promotes the important facts from the gitignored raw capture in
`.test-agent/bringup0/`. It records only read-only observations made on
2026-08-19. No UF2 was copied, no firmware was written, and no lifecycle or
boot-policy command was run.

## Device identity

| Field | Observed value |
|---|---|
| Board type | Dabao |
| Public serial | `S2NM5B` |
| UUID | `69ed86ae-74a80239-956b2be5-1415a219` |
| Device serializer | `e12dead1-1009b08e-f91ca52c-2b63d850` |
| Silicon stepping | A0 |

The public serial remained stable across normal Xous and boot1 USB modes.

## USB modes

Before the physical PROG sequence, the running factory system exposed:

```text
1d50:6197 Baochip Dabao
/dev/serial/by-id/usb-Baochip_Dabao_S2NM5B-if02 -> ../../ttyACM0
```

It did not expose a `BAOCHIP` block device.

After disconnecting USB, holding `PROG`, reconnecting USB, and releasing
`PROG`, boot1 exposed:

```text
1d50:6196 Baochip-1x
/dev/serial/by-id/usb-Baochip_Baochip-1x_S2NM5B-if00 -> ../../ttyACM0
/dev/sdb1: FAT32, label BAOCHIP, UUID 46DC-CFCD
```

The boot1 descriptor contained CDC control, CDC data, and mass-storage
interfaces. The block device identified itself as `USB update vdisk`, with a
128 MiB device and 119 MiB FAT32 partition. It was not mounted or written.

## Audit transcript summary

The only command issued was `audit`. Relevant output was:

```text
Board type reads as: Dabao
Boot partition is: Ok(PrimaryPartition)
Semver is: v0.10.0-61-g5397e1b48
Description is: bao2-0
Stepping is: A0
Public serial number: S2NM5B
Paranoid mode: 0/0
Possible attack attempts: 0
Stage       key0     key1     key2     key3
boot0       enabled  enabled  enabled  enabled
boot1       enabled  enabled  enabled  enabled
next stage  enabled  enabled  enabled  enabled
Boot0: key 0/0 (bao1) -> 60000000
Boot1: key 0/0 (bao1) -> 60020000
Next stage: key 2/2 (beta) -> 60060000
Erase proof: uninit or access denied
In-system keys have been generated
CM7 & debug confirmed fused off
Collateral erased
Boot1 receipts OK
Boot1 anti-rollback OK
```

No minimum-security warning or `== IN DEVELOPER MODE ==` line appeared.

## Source-backed interpretation

The reported hash resolves to Xous commit
`5397e1b488c081566cef2c0e597e05426f67c1c3`, the v0.10.0 boot1-era source.
Inspection of that exact source establishes:

- the missing developer-mode line means the `DEVELOPER_MODE` counter was zero;
  that audit prints the warning whenever the counter is nonzero;
- `Collateral erased` is a separate collateral-slot check and does not mean
  developer mode;
- `Erase proof: uninit or access denied` is intentionally ambiguous;
- the revocation table reports primary counters, not hardened duplicate
  counters; and
- this boot1 predates PQ validation and the `PQ required: A/B` audit field, so
  absence of that line is a version difference rather than a measured `0/0`.

The board was therefore still in its factory security mode, with a valid
beta-key Xous loader in the shared next-stage slot and primary developer key3
enabled. Booting a valid developer-key Zephyr image would be its first
irreversible developer-mode transition.

## USB handoff finding

The installed boot1 source also resolved an important architecture question.
Boot1 stops the USB controller and forces PC13/SE0 low before jumping so that a
next stage can initialize fresh controller state and re-enumerate. Xous's
USB-enabled baremetal target implements this receiving side.

PB13/PB14 are separate UDMA UART2 pins. They are not multiplexed USB data pins.
The same boot1 REPL is routed over either USB CDC or UART2, which had made the
transport relationship easy to misinterpret.

Current Zephyr lacks the Baochip UDC driver needed to receive the USB handoff.
That is a software gap, not a board wiring or boot1 limitation.

## Recovery observation

Boot1 offers no production command to read back the installed loader slot. Its
MSC device is a synthetic update disk, and JTAG is fused off. Exact backup of
the beta loader is therefore unavailable through the observed interfaces.

Other boards from the same batch preserve the factory reference state, so this
board can be dedicated to developer-mode bring-up. A locally built dev-signed
matched Xous loader/kernel/apps set is the practical post-transition recovery
path; restoring it would not reverse developer mode.

## Local evidence manifest

The raw local capture includes the CRLF and normalized audit, USB descriptors,
kernel enumeration log, `lsblk`, udev properties, stable serial links, and
SHA-256 manifest under `.test-agent/bringup0/`. All manifest entries passed
`sha256sum -c` immediately after capture.

## Cross-references

- [`architecture.md`](architecture.md) generalizes the boot, transport, and
  recovery model established by this device.
- [`procedure.md`](procedure.md) turns this diagnostic sequence into a reusable
  guarded runbook.
- [Installed boot1 `audit.rs`](https://github.com/betrusted-io/xous-core/blob/5397e1b488c081566cef2c0e597e05426f67c1c3/bao1x-boot/boot1/src/audit.rs)
  supports the lifecycle interpretation.
- [Installed boot1 handoff](https://github.com/betrusted-io/xous-core/blob/5397e1b488c081566cef2c0e597e05426f67c1c3/bao1x-boot/boot1/src/main.rs#L377-L422)
  supports the USB conclusion.
