<!-- SPDX-License-Identifier: Apache-2.0 -->

# bao-uf2send

`bao-uf2send` transfers the canonical baremetal UF2 emitted by `bao-image` to
the Baochip boot1 REPL over USB CDC-ACM or Dabao UART2. It validates every UF2
block, the Baochip family ID, contiguous addresses beginning at `0x60060000`,
and the baremetal slot bound before opening the serial port.

List ports:

```sh
cargo run -p bao-uf2send -- --list-ports
```

Transfer over USB CDC-ACM:

```sh
cargo run -p bao-uf2send -- build/zephyr/zephyr.uf2 \
  --port /dev/ttyACM0
```

Transfer over a 3.3 V serial adapter connected to physical Dabao UART2 (PB14
board TX, PB13 board RX, plus ground):

```sh
cargo run -p bao-uf2send -- build/zephyr/zephyr.uf2 \
  --port /dev/ttyUSB0 --baud 1000000
```

The physical UART setting is 1,000,000 baud, 8-N-1; 1,000,000 is the CLI
default. `--timeout-ms` (default 500), `--settle-ms` (default 100), and
`--retries` (maximum attempts per block, default 3) tune the transfer.

The `bao-boot1-protocol` library validates the complete image before
`bao-uf2send` opens the serial port: UF2 magic and canonical flags, Baochip
family ID, 256-byte payloads, numbering, contiguous addresses from
`0x60060000`, and the baremetal slot bound. During transfer it disables local
echo twice byte-by-byte, waits for settling, and probes `has-crc`. A `true`
response selects CRC commands and exact size/address/CRC acknowledgments;
`false` or no recognized response falls back to legacy exact size/address
acknowledgments. CRC errors, mismatches, and timeouts are retried only up to the
configured attempt limit. Local echo is restored on success or failure.

An accepted `Wrote` acknowledgment confirms that boot1 handled the command,
but it does not prove persistence: affected boot1 versions can print `Write
error` after a failed RRAM write and then still print `Wrote`. The sender cannot
disambiguate that sequence, so verify the installed image independently.

Hold PROG to remain in boot1. A developer-signed image can irreversibly enter
developer mode and erase device secrets; inspect the device with `audit`
before transfer. Host preflight cannot determine key revocation,
anti-rollback, or require-PQ state. See the
[`manual validation guide`](/doc/bringup/manual-validation.md) for lifecycle
checks and recovery.
