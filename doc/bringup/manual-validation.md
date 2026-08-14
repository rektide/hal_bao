<!-- SPDX-License-Identifier: Apache-2.0 -->

# Manual M1 hardware validation

This procedure tests the first signed Zephyr image on a Baochip Dabao board. It
validates boot1 acceptance and the handoff into the baremetal slot. The current
image has no Zephyr console or system clock driver, so it does **not** print
`Hello World` after handoff.

## Destructive consequences

Do not continue until all of these conditions are acceptable:

1. Use a dedicated development board. A devkey-signed image irreversibly puts
   the device into developer mode, erases factory-programmed secrets, and
   increments a one-way counter.
2. The image replaces the Xous loader in the shared loader/baremetal slot. Xous
   will not boot again until a compatible, signed Xous `loader.uf2` is restored.
3. A device on which the developer key was revoked with boot1 `lockdown` will
   reject this image.
4. This artifact has no post-quantum signature. A device configured with
   `require-pq` will reject it.
5. Do not copy this image to an `ALTCHIP` volume. That environment is for
   updating boot1 itself.

Obtain and retain a matching Xous `loader.uf2` before testing if the board must
be restored afterward.

## Artifact bundle

The current local bundle is in
[`/.test-agent/m1-hardware-validation/`](/.test-agent/m1-hardware-validation/):

| File | Purpose |
|---|---|
| `dabao-zephyr-hello.uf2` | **The only file to copy to the board** |
| `dabao-zephyr-hello.img` | Signed Bao1xV1 baremetal image before UF2 wrapping |
| `dabao-zephyr-hello.presign.bin` | Trampoline plus flattened Zephyr ROM image |
| `dabao-zephyr-hello.elf` | Linked Zephyr ELF for inspection and debugging |
| `dabao-zephyr-hello.map` | Linker map |
| `zephyr.config` | Effective Zephyr Kconfig |
| `zephyr.dts` | Effective devicetree |
| `SHA256SUMS` | Bundle integrity manifest |

The bundle was built from:

- `zephyr-baochip` board commit `d622e1fa70dd`;
- `hal_bao` image tool commit `4eef1c40c4b0`;
- Xous signing implementation `5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b`;
- Zephyr SDK 1.0.1 `riscv64-zephyr-elf` toolchain; and
- Xous developer key with embedded version `v0.10.2-76-g5d5bbbfa`.

Verify the bundle from its directory:

```sh
cd /home/rektide/src/hal_bao/.test-agent/m1-hardware-validation
sha256sum -c SHA256SUMS
```

## Prepare observation

Boot1 exposes USB CDC-ACM while it is running. For visibility across the jump,
use a separate 3.3 V serial adapter connected to Dabao PB14 (board TX, adapter
RX), PB13 (board RX, adapter TX), and ground at 1,000,000 baud, 8-N-1. Do not
connect a 5 V serial adapter.

The current Zephyr image does not configure the PB13/PB14 UDMA UART. The serial
capture is therefore useful for boot1 messages and failures, but output is
expected to stop after a successful handoff.

Before flashing, enter boot1 and optionally save its `audit` output:

1. Disconnect USB power.
2. Hold `PROG`, connect USB, then release `PROG`.
3. Locate the new USB CDC serial device and connect at 1,000,000 baud.
4. Run `audit` and save the output, especially lifecycle/developer state and
   boot1 revision.

On Linux, inspect candidate serial and storage devices with:

```sh
dmesg --follow
lsblk -o NAME,LABEL,FSTYPE,SIZE,MOUNTPOINTS
```

## Install the image

1. Confirm that exactly one FAT volume has label `BAOCHIP`.
2. Confirm that the selected mount is not labeled `ALTCHIP`.
3. Copy only `dabao-zephyr-hello.uf2` to that mounted volume.
4. Flush writes and cleanly unmount the volume.
5. Press `PROG`, the button closest to USB, to boot the installed image.

Example, replacing `/media/$USER/BAOCHIP` with the mount shown by `lsblk`:

```sh
cp dabao-zephyr-hello.uf2 /media/$USER/BAOCHIP/
sync /media/$USER/BAOCHIP/dabao-zephyr-hello.uf2
```

Do not use Zephyr's generic `zephyr.uf2`; it lacks Baochip's signature block
and entry trampolines.

## Expected result

The present artifact provides these observable checkpoints:

1. Boot1 accepts all 36 UF2 blocks without reporting a family, address, or
   signature error.
2. Pressing `PROG` causes the `BAOCHIP` MSC and boot1 USB CDC devices to
   disconnect as boot1 jumps to `0x60060000` through the signature trampoline.
3. Boot1 output on PB14 stops after handoff and the board remains powered.
4. The board does not enumerate a new USB device and does not print Zephyr
   output. This is expected for the current image.

These observations prove that boot1 accepted the signed image and attempted the
handoff. They do not yet distinguish successful execution of Zephyr `main()`
from an early silent fault. The DUART console ticket adds that decisive marker.

Record the following for the bring-up report:

- board revision and whether it was already in developer mode;
- boot1 `audit` revision and lifecycle state;
- host OS and the observed MSC/CDC device names;
- whether the UF2 copy completed without a boot1 error;
- serial output immediately before and after pressing `PROG`;
- whether holding `PROG` during a later power cycle re-enters boot1; and
- any status/error text verbatim.

## Re-enter boot1 and recover

To leave the silent Zephyr image, disconnect power, hold `PROG`, reconnect USB,
and release `PROG`. The `BAOCHIP` volume and boot1 USB CDC console should return.

To restore Xous, copy the previously retained, compatible signed `loader.uf2`
to `BAOCHIP`, flush and unmount it, then press `PROG`. The Xous kernel and app
regions are not modified by this Zephyr baremetal UF2, but the restored loader
must be compatible with those existing regions.

If boot1 does not reappear, stop and preserve the serial log. Do not attempt an
alt-boot1 update or write to `ALTCHIP` as part of this procedure.

## Rebuild the bundle

Build `hello_world` for the Dabao target in `zephyr-baochip`, then pack and sign
from `hal_bao`:

```sh
cargo run -p bao-image -- pack \
  /path/to/build/zephyr/zephyr.elf zephyr.presign.bin

cargo run -p bao-image -- sign zephyr.presign.bin zephyr.img \
  --key /path/to/xous-core/devkey/dev.key \
  --git-describe v0.10.2-76-g5d5bbbfa
```

`bao-image sign` emits `zephyr.uf2` beside `zephyr.img`. Signing with a
production or owner key requires a separate key-management procedure; do not
put private production keys in this repository or a build directory.
