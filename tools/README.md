<!-- SPDX-License-Identifier: Apache-2.0 -->

# Baochip image tooling

The `bao-image` Rust tool converts a linked Zephyr ELF into the presign format expected by
Baochip boot1. Unlike Xous's `xous-copy-object --bao1x`, it retains every
file-backed `PT_LOAD` segment at its physical/load address. This includes the
ROM load image for Zephyr's initialized `.data`, which Zephyr copies to ACRAM
during reset.

The tool directly reuses `bao1x-api` for the boot contract and
`xous-tools::sign_image` for signing and UF2 generation. The Xous dependency is
pinned to a reviewed commit so image semantics cannot drift between builds.

The packer rejects an ELF unless:

- its RISC-V entry and first load address are exactly `0x60060400`;
- all file-backed load images fit below `0x6009fc00`;
- initialized RAM sections have a ROM load address; and
- load images do not overlap.

Pack an ELF:

```sh
cargo run -p bao-image -- pack \
  build/zephyr/zephyr.elf build/zephyr/zephyr.presign.bin
```

Sign it with Xous's canonical signer. The wrapper fixes the signature length to
768 bytes, requests the signature-block jump, selects Baochip V1 and the
`baremetal` function code, and therefore causes the signer to emit both
`zephyr.img` and `zephyr.uf2` (family ID `0xa7d76373`):

```sh
cargo run -p bao-image -- sign \
  build/zephyr/zephyr.presign.bin build/zephyr/zephyr.img \
  --key ~/archive/betrusted-io/xous-core/devkey/dev.key \
  --git-describe v0.10.0-0-g0000000
```

Developer-signed images irreversibly put a device into developer mode and erase
on-chip secrets. Use a dedicated development board.

Run the host-side tests with:

```sh
cargo test -p bao-image
```

Serial delivery has two packages:

- `bao-boot1-protocol` is the reusable library for transport-independent
  canonical UF2 preflight, boot1 REPL negotiation, acknowledgments, and retry
  behavior.
- [`bao-uf2send`](/tools/uf2send/README.md) is the serial-port binary for USB
  CDC-ACM and physical Dabao UART2.

The CLI preflights the complete image before opening the serial port. This is
artifact validation, not device lifecycle validation: run boot1 `audit` before
transfer as described in the
[`manual validation guide`](/doc/bringup/manual-validation.md).
