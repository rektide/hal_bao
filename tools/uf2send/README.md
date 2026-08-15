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

Physical Dabao UART2 uses 1,000,000 baud (the default). `--baud`,
`--timeout-ms`, `--settle-ms`, and `--retries` tune the transport. The sender
disables local echo twice byte-by-byte, waits 100 ms by default, then probes
`has-crc`. It uses CRC acknowledgments when supported, accepts only an exact
size/address acknowledgment for each block, bounds retries, and restores
boot1 local echo on success or failure.

The protocol implementation is reusable as the `bao-boot1-protocol` workspace
crate. A `Wrote` acknowledgment confirms that boot1 handled the command, but it
does not prove persistence: boot1 can print `Write error` after a failed RRAM
write and then still print `Wrote`. Verify the installed image independently.

Hold PROG to remain in boot1. A developer-signed image can irreversibly enter
developer mode and erase device secrets; inspect the device with `audit`
before transfer.
