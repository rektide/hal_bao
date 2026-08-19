---
type: Runbook
title: Guarded Dabao Zephyr bring-up
description: Repeatable procedure for identifying, auditing, programming, observing, and recovering a Dabao board.
resource: /.design/bringup/procedure.md
tags: [baochip, dabao, zephyr, runbook, uf2]
status: draft
generated: { by: agent:opencode-gpt56, at: 2026-08-19 }
sources:
  - id: architecture
    resource: /.design/bringup/architecture.md
    title: Baochip Dabao bring-up architecture
  - id: lifecycle
    resource: /.design/research/05-lifecycle-delivery-validation.md
    title: Lifecycle and delivery validation
---

# Guarded bring-up procedure

This procedure is artifact-independent. Exact filenames, source revisions,
hashes, and expected application markers belong in each validation bundle.

## Safety boundary

Stop before writing unless all of the following are accepted:

- the target is dedicated to development, or its factory security state is
  known to be expendable;
- the first developer-key boot will irreversibly erase protected secrets and
  enter developer mode;
- installing Zephyr replaces the Xous loader in the shared next-stage slot;
- a compatible recovery image set is retained, or equivalent same-batch
  hardware preserves the factory reference state; and
- no part of this procedure uses `ALTCHIP` or updates boot1.

Never run lifecycle or policy commands such as `lockdown`, `altboot`,
`boardtype`, `idmode toggle`, `paranoid`, `require-pq`, `baosec-init`, or
`self_destruct` during routine bring-up.

## 1. Identify the running board

Before pressing buttons, record:

```sh
lsusb
find /dev/serial/by-id -maxdepth 1 -type l -printf '%f -> %l\n'
lsblk -o NAME,PATH,TRAN,LABEL,FSTYPE,MOUNTPOINTS,SIZE,MODEL,SERIAL
```

Prefer `/dev/serial/by-id/...` over `/dev/ttyACM0` in logs and scripts. If the
factory Xous revision matters, boot it normally and capture its `ver` output
before replacing the loader; boot1 cannot read the loader slot back later.

## 2. Enter boot1 read-only

1. Disconnect USB power.
2. Hold `PROG`.
3. Reconnect USB, then release `PROG`.
4. Require the boot1 USB identity and a FAT volume labeled exactly `BAOCHIP`.
5. Stop if the volume is `ALTCHIP`.
6. Locate boot1's CDC-ACM device and connect at 1,000,000 baud, 8N1.
7. Run only `audit` and save the complete output.

On Linux, a simple terminal is:

```sh
python -m serial.tools.miniterm /dev/serial/by-id/<boot1-device> 1000000
```

Audit output varies by boot1 revision. Record at least:

- board type, stepping, public serial, and boot partition;
- boot1 semantic version and description;
- primary key revocations and current next-stage key/tag/target;
- developer-mode warning, if present;
- paranoid mode and attack counter;
- provisioning, fuse, receipt, and anti-rollback warnings; and
- PQ policy only if that boot1 actually prints it.

Absence of `PQ required` on an older boot1 is unknown policy capability, not
evidence of `0/0`. Absence of `== IN DEVELOPER MODE ==` is meaningful only
after checking the exact installed audit source prints that warning whenever
its developer-mode counter is nonzero.

Stop on a revoked required key, unexpected board type, alternate boot
partition, signature-validation error, fuse/setup warning, or unexplained
lifecycle state.

## 3. Validate the artifact on the host

Verify the bundle's recorded hashes, then inspect the only signed UF2 intended
for the board:

```sh
sha256sum -c SHA256SUMS

cargo run -q -p bao-image -- inspect /absolute/path/to/image.uf2 --json
```

Require:

- canonical Baochip baremetal UF2 structure and family ID;
- first address `0x60060000` and image entirely inside the baremetal slot;
- compatible signed-image header and trampolines;
- baremetal function code;
- successful classical signature verification; and
- artifact policy compatible with the connected boot1.

`bao-image inspect` proves a signature against the public key embedded in the
image; that is not by itself an external trust anchor. Also require the
bundle's independently recorded Xous image-checker result or known developer
key fingerprint/tag before writing, and confirm device-local key acceptance
through boot1 audit.

Do not substitute Zephyr's generic `zephyr.uf2`, an ELF, a presign binary, or a
raw signed image.

## 4. Prepare decisive observation

Choose the marker before writing:

- **USB CDC Zephyr image:** monitor kernel USB events and stable serial links.
  Expect boot1 disconnect followed by a new Zephyr USB enumeration. This path
  requires the Baochip Zephyr UDC driver.
- **UART Zephyr image:** connect a 3.3 V adapter to PB14 board TX, PB13 board
  RX, and ground at 1,000,000 baud 8N1. Never use 5 V signaling.
- **Silent image:** can prove persistence and attempted handoff only. It cannot
  prove that Zephyr reached `main()`.

Dabao has no user LED, and its dedicated DUART package pad is unconnected.

## 5. Transfer the signed UF2

Prefer the guarded MSC copy. Mount the exact `BAOCHIP` partition, then preview:

```sh
cargo run -q -p bao-image -- copy /absolute/path/to/image.uf2 \
  --target /absolute/path/to/BAOCHIP --dry-run
```

Review the selected mount identity and artifact report. Perform the irreversible
write only after explicit approval:

```sh
cargo run -q -p bao-image -- copy /absolute/path/to/image.uf2 \
  --target /absolute/path/to/BAOCHIP
```

Flush and cleanly unmount the volume. The tool refuses `ALTCHIP`, ambiguous
mounts, malformed images, and overwrite races.

Serial transfer through USB CDC or UART2 is a fallback:

```sh
cargo run -q -p bao-uf2send -- /absolute/path/to/image.uf2 \
  --port /dev/serial/by-id/<boot1-device> --baud 1000000
```

An accepted `Wrote` response proves command framing, not RRAM persistence, on
stock boot1.

## 6. Verify persistence before boot

Remain in or re-enter boot1 and run `audit` again. Save the complete output.
Require next-stage validation to identify the expected developer-key image and
target without errors.

If the first developer transition resets the board, re-enter boot1 and repeat
the audit. Confirm the explicit developer-mode warning on revisions that
provide one. Do not treat transfer completion alone as installation success.

## 7. Boot and classify evidence

Start the chosen USB or UART capture, then press `PROG` to leave boot wait and
boot the installed next stage.

Classify the result precisely:

| Observation | Proven claim |
|---|---|
| Post-write audit validates expected image | Signed bytes persisted and boot1 accepts them |
| Boot1 CDC/MSC disconnects | Boot1 attempted handoff |
| New Zephyr USB identity enumerates | Zephyr executed USB initialization far enough to enumerate |
| Expected Zephyr banner/application marker appears | Zephyr reached console initialization and application code |
| Sleep/preemption pass marker appears | Timer IRQ, scheduler wakeup, and preemption passed that application |

A disconnect without a new marker is not proof of `main()`.

## 8. Prove boot1 recovery

1. Disconnect USB power.
2. Hold `PROG`.
3. Reconnect USB and release `PROG`.
4. Require boot1 CDC and `BAOCHIP` to return.
5. Capture another `audit` before any additional write.

If boot1 does not return, preserve host and console logs and stop. Do not try
`ALTCHIP`, boot1 replacement, or lifecycle commands as improvised recovery.

Restore Xous only with a host-validated compatible loader or matched
loader/kernel/apps set. On a developer-mode board, a local dev-signed matched
set is acceptable; restoring it does not reverse developer mode.

## Bring-up record

Each physical run should retain:

- board revision, public serial, and whether developer mode was already set;
- boot1 version and complete pre/post-write audits;
- host USB identities, block device, mount identity, and stable serial paths;
- artifact hashes, source revisions, inspection output, and transfer log;
- complete USB/UART output around handoff;
- exact pass marker or failure text; and
- successful or failed boot1 recovery.

## Cross-references

- [`architecture.md`](architecture.md) explains why each safety gate exists.
- [`s2nm5b-baseline.md`](s2nm5b-baseline.md) is an example read-only baseline.
- [`/tools/README.md`](/tools/README.md) documents image packing, inspection,
  and guarded MSC copy.
- [`/tools/uf2send/README.md`](/tools/uf2send/README.md) documents serial
  preflight, acknowledgments, retries, and the persistence limitation.
