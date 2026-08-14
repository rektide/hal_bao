---
type: Research
title: Baochip 1x boot chain & firmware delivery for Zephyr
description: How boot0/boot1 boot images, the exact signed image + UF2 format, the baremetal target's contract, and what a Zephyr port must match.
tags: [baochip, bao1x, zephyr, boot, uf2, signing]
status: draft
generated: { by: agent, at: 2026-08-14 }
sources:
  - ~/archive/betrusted-io/xous-core (read-only checkout)
  - ~/archive/baochip/baochip-1x
  - ~/archive/betrusted-io/dabao-base-app
  - ~/archive/baochip/bio-loader
---

# Baochip 1x boot chain & firmware delivery (for a Zephyr port)

Scope: how to build, sign, deliver and debug a non-Xous (baremetal-slot) image on BAO1X2S4F-WA, and
what machine state exists at entry. All paths below are relative to `~/archive/betrusted-io/xous-core`
unless prefixed with another repo root.

## 1. Boot chain stages

Chain: reset → `boot0` (mask ROM root of trust) → `boot1` (USB bootloader) → `baremetal` **or** `loader`
(same slot) → (loader only) Xous kernel → apps.

- `boot0` is immutable, burned at OSAT final test, sealed with JTAG-only IFR fuses
  (`bao1x-boot/BOOTCHAIN.md:7`, `:12`, `:33-35`). It self-validates and validates `boot1`
  (`bao1x-boot/boot0/src/main.rs:285-341`), runs the mutual-distrust key/collateral policy
  (`bao1x-boot/boot0/src/main.rs:128-279`), then jumps (`bao1x-boot/boot0/src/main.rs:364`).
- `boot1` validates the `baremetal`/`loader` slot (`bao1x-boot/boot1/src/secboot.rs:61-63`), disables
  all IRQs (`bao1x-boot/boot1/src/secboot.rs:86`), seals its key slots (`.../secboot.rs:122`), and jumps
  (`.../secboot.rs:127`). boot1 hosts a USB MSC (FAT32, volume label `BAOCHIP`, `ALTCHIP` when running
  alt-boot1 — `README-baochip.md:46-52`) plus CDC-ACM USB and 1 Mbaud UDMA UART2 serial consoles
  on PB13/PB14 (`README-baochip.md:27-28`, `README-consoles.md:5-16`). It has a command REPL (see §4).
- `baremetal` = "unsecured, bare-iron environment, no_std, alloc pre-initialized, USB serial console"
  (`README-baochip.md:59`). `loader` is the same slot blossoming into Xous: it unpacks kernel+init
  from RRAM, builds page tables, sets `satp`/`mepc` and `mret`s into the kernel in S-mode
  (`loader/src/asm.rs:276-301`). For Zephyr, the baremetal slot is the target.

### Flash/RRAM layout (code region = start + 768 B sigblock + 256 B statics record)

| Region | Base | Code/text origin | Size / notes |
|---|---|---|---|
| boot0 | `0x60000000` (`libs/bao1x-api/src/offsets/common.rs:8`) | `0x60000400` (xtask, `xtask/src/main.rs:708-717`) | ends at `0x60020000`; reset vector |
| boot1 | `0x60020000` (`common.rs:9`) | `0x60020400` (`bao1x-boot/boot1/src/platform/bao1x/link.x:5`) | 255k−3856 (link.x:5); PQ sig postpended |
| baremetal / loader | `0x60060000` (`LOADER_START`/`BAREMETAL_START`, `common.rs:10-11`) | `0x60060400` (`baremetal/src/platform/bao1x/link.x:5`, `loader/src/platform/bao1x/link.x:2`) | FLASH LENGTH = 255k − 1024 ⇒ code must end ≤ `0x6009FC00` |
| kernel | `0x6009FD00` = `0x600A0000 − SIGBLOCK_LEN` (`common.rs:13`) | `0x600A0000` | kernel+resident core services |
| apps (dabao) | `0x602FFD00` = `0x60300000 − SIGBLOCK_LEN` (`offsets/dabao.rs:25-26`) | `0x60300000` | detached apps in RRAM |
| storage end | `0x603DA000` (`RRAM_STORAGE_LEN`, `common.rs:16`) | — | above: data slots `0x603E0000` (`offsets.rs:80`), IFR `0x60400000` (`offsets.rs:87`) |

Other memory: RRAM = `0x60000000`, 4 MiB, XIP-writable (`utralib/src/generated/bao1x.rs:319-320`);
ACRAM/SRAM = `0x61000000`, 2 MiB (`bao1x.rs:317-318`); off-chip SPINOR XIP/swap window = `0x70000000`,
128 MiB (`common.rs:20-21`). RRAM erase granularity is 32 bytes (`BOOTCHAIN.md:67`).

### Entry mechanics (important!)

1. boot0/boot1 call `jump_to(target, tag)` which sets **a0 = `KERNEL_START` (0x6009FD00), a1 = 0**, and
   jumps to `region_base ^ tag` = the region base (`libs/bao1x-hal/src/sigcheck.rs:856-873`; jump target
   returned by `validate_image` is `img_offset ^ tag`, `sigcheck.rs:431-437`).
2. Word at region base is a `jal x0, +768` inserted by the signer (`tools/src/sign_image.rs:392`,
   `generate_jal_x0` at `:48-82`) → lands on the 256-byte `StaticsInRom` record at base+768.
3. `StaticsInRom.jump_instruction = 0x1000006f` = `jal x0, +256` (`libs/bao1x-api/src/lib.rs:121-129`)
   → lands at base+0x400 = `_stext`, the first linked instruction.
4. So the ELF's `e_entry` is *never used by hardware* — the first instruction at the link origin must be
   the reset entry (`xous-copy-object` merely prints the entrypoint, `tools/src/bin/xous-copy-object.rs:94`).

## 2. Image format & signing

### Signed image layout (`Version::Bao1xV1`, `tools/src/sign_image.rs:273-486`)

```
[SignatureInFlash header, 768 B total]        SIGBLOCK_LEN = 768 (libs/bao1x-api/src/signatures.rs:6)
  +0   jal x0, +768                            (with --with-jump)
  +4   ed25519 signature [64]
  +68  aad_len(u32)=0, aad[60]
  +132 SealedFields:                            (signatures.rs:221-279)
       version=0x100, magic "yumy"/"Bao3" (signatures.rs:86), signed_len, function_code,
       anti_rollback, min_semver[16], semver[16], pubkeys[4] (ed25519 pk[32]+tag[4]),
       toolchain[20], corrected_version, pq_enabled, pubkeys_pq[4]
  ...  zero padding to 768
[0x300 StaticsInRom, 256 B]                     (packed by xous-copy-object)
[0x400 flat program image (text/rodata; ".data" stripped to poke table)]
[optional SLH-DSA-SHA2-128-24 signature, 3856 B, postpended]
```

- Signed region = `sealed_data ‖ padding ‖ payload`, signed with **ed25519ph (SHA-512 prehash)**
  (`sign_image.rs:394-405`); verified by boot1 via `validate_image` (`libs/bao1x-hal/src/sigcheck.rs:72-443`).
- PQ signature is optional (`pq_enabled=0` when no PQ key given, `sign_image.rs:317-319`, `:409-438`);
  SLH-DSA-SHA2-128-24, 3856 B (`bao1x-boot/PQ.md` table; `signatures.rs:15-18`). Devices can *require*
  PQ via the `require-pq` one-way counter (`bao1x-boot/boot1/src/repl.rs:724`).
- Function code for the baremetal slot: `Baremetal = 6` (`signatures.rs:38-39`); function-code
  mismatch → reject (`sigcheck.rs:120-126`). Anti-rollback for baremetal = 1
  (`signing/anti-rollback.hjson`).
- Key manifest = the 4 pubkeys inside `SealedFields`; slot 3 is the well-known developer key
  (`signatures.rs:251-253`). Developer-signed images are accepted by default but erase on-chip secrets
  and set a one-way `DEVELOPER_MODE` counter (`README-baochip.md:91-93`, `BOOTCHAIN.md:20-22`). Keys are
  checked in order with revocation one-way counters (`sigcheck.rs:135-158`).

### Developer key

`devkey/dev.key` is an ed25519 PEM ("kindly note this is a dev key..." — `devkey/README.md`);
`devkey/dev-pq.key` + `devkey/dev-pq.cache` are the SLH-DSA key and tree cache. xtask defaults to these
(`xtask/src/builder.rs:198-228`).

### Tooling pipeline (what xtask does for baremetal targets)

1. Build ELF for `riscv32imac-unknown-none-elf` (`xtask/src/main.rs:45`, `xtask/src/builder.rs:453-463`).
2. `xous-copy-object <elf> <presign.img> --bao1x` — flattens sections into a bin, prepends the 256 B
   `StaticsInRom` with `.data` origin/clear-size/poke-table (`tools/src/bin/xous-copy-object.rs:54-89`).
   `.data` is *stripped* from the image; non-zero words become pokes — **max 40 pokes, offsets limited
   to u16** (`tools/src/elf.rs:218-283`, `:338-361`; `xous-copy-object.rs:66-70`).
3. `xous-sign-image --loader-image <presign.img> --loader-key devkey/dev.key --loader-output <out.img>
   --min-xous-ver v0.9.8-790 --sig-length 768 --with-jump --bao1x --function-code baremetal [PQ args]`
   (`xtask/src/builder.rs:1065-1101`; CLI `tools/src/bin/xous-sign-image.rs:15-226`). Embedded semver comes
   from `git describe` unless `--git-describe` or CI env is used (`tools/src/sign_image.rs:96-125`) — run it
   inside the xous-core checkout. The tool then auto-generates the `.uf2` when the output ends in
   `.img`/`.bin` and a function code is given (`tools/src/bin/xous-sign-image.rs:215-226`).
4. Copy `.uf2` to the boot1 MSC volume; `sync`/clean unmount; press PROG (`README-baochip.md:12-14`).

Devkey-signed boot1-update path (`bao1x-alt-boot1`) shows the same flow with `--function-code baremetal`
but origin `LOADER_START` (`xtask/src/main.rs:733-745`).

### UF2 configuration

- Family ID: `BAOCHIP_1X_UF2_FAMILY = 0xa7d7_6373` (`libs/bao1x-api/src/lib.rs:29-31`).
- 512-byte UF2 blocks with 256-byte payloads, flags `0x2000` (family present); target address =
  region start + block offset (`tools/src/sign_image.rs:533-624`, `bin_to_uf2` at `:585-624`).
  Magic numbers are the standard UF2 ones (0x0A324655 / 0x9E5D5157 / 0x0AB16F30,
  `bao1x-boot/boot1/src/uf2.rs:8-17`).
- boot1 accepts UF2 blocks only for `target_addr ∈ [BAREMETAL_START .. RRAM end]` (or `[BOOT1_START,
  BAREMETAL_START)` when running alt-boot1) **and** family `0xa7d76373`
  (`bao1x-boot/boot1/src/platform/bao1x/usb/handlers.rs:240-258`); swap-region writes go to `0x7000_0000+`
  (`handlers.rs:267-292`). Function-code → base mapping: `baremetal → BAREMETAL_START (0x60060000)`
  (`tools/src/sign_image.rs:554-563`).

## 3. The baremetal target (the Zephyr slot)

### Build

`cargo xtask baremetal-bao1x-dabao` (or `-baosec`) (`xtask/src/main.rs:670-694`): builds crate `baremetal`
for `riscv32imac-unknown-none-elf`, sets link FLASH ORIGIN to `BAREMETAL_START + 768 + 256 = 0x60060400`
via `update_flash_origin` (`xtask/src/main.rs:1166-1185`), then copy+sign as above. Output artifacts land
in `target/riscv32imac-unknown-none-elf/release/` (`README-baochip.md:12`). `baremetal-bao1x-evb` links
into RAM at `0x61000400` instead (JTAG/EVB flow, `xtask/src/main.rs:696-706`).

### Linker script (`baremetal/src/platform/bao1x/link.x:1-22`)

```
MEMORY { FLASH : ORIGIN = 0x60060400, LENGTH = 256k - 1024
         RAM   : ORIGIN = 0x61000000, LENGTH = 2048k }
```
`.text`/`.rodata` in FLASH (XIP from RRAM), `.data`/`.bss`/stack/heap in RAM with `.data` LMA in FLASH
(`link.x:78-88`); riscv-rt-style sections, `ENTRY(_start)`, `.text.init` kept first (`link.x:43-65`) so
`_start` sits exactly at `0x60060400`.

### Runtime the crate provides

- `_start` (`.text.init`): `sp = RAM top − 4`, `mtvec = abort`, jump to `rust_entry`
  (`baremetal/src/asm.rs:32-56`).
- `early_init()`: SRAM trims for voltage, processes `StaticsInRom` (zero `.data+.bss`, apply ≤40 pokes),
  clock init to fclk 700 MHz (CPU 350 MHz), heap init (**`linked_list_allocator`, 256 KiB at
  `0x61006000`**), tick timer, UDMA-UART + USB consoles, IRQ setup
  (`baremetal/src/platform/bao1x/bao1x.rs:33-131`, `:149-156`). Panic handler prints and loops
  (`bao1x.rs:184-191`).
- Main loop: REPL over UART/USB-CDC, entirely IRQ-driven USB (`baremetal/src/main.rs:105-150`).
- `alloc` is pre-initialized (`extern crate alloc`, `baremetal/src/main.rs:4`; `README-baochip.md:59`).

### Machine state handed over by boot1 at entry

| Item | State | Evidence |
|---|---|---|
| Privilege | Machine mode (whole chain is M-mode; only the Xous loader ever touches `satp`/`mstatus.MPP`) | `loader/src/asm.rs:276-301`; no satp writes in boot0/boot1/baremetal |
| Registers | `a0 = 0x6009FD00` (KERNEL_START), `a1 = 0`; sp/mtp garbage from boot1 — must re-init | `libs/bao1x-hal/src/sigcheck.rs:857-871`; `baremetal/src/asm.rs:39-41` |
| IRQs | All disabled just before jump | `bao1x-boot/boot1/src/secboot.rs:86` |
| mtvec | Points at boot1's `abort` | `bao1x-boot/boot1/src/asm.rs:16-18` |
| MMU/caches | satp untouched (off); no explicit cache flush/disable in boot1 (loader only fences at MMU enable) | `loader/src/asm.rs:285-295` |
| Clocks | boot0 sets conservative clocks incl. DUART (`bao1x-boot/boot0/src/platform/bao1x/bao1x.rs:81-107`, `:142`); boot1 re-inits fclk = 700 MHz ⇒ CPU 350 MHz, perclk 100 MHz at jump | `bao1x-boot/boot1/src/platform/bao1x/bao1x.rs:468-488`, `:514`; `offsets/dabao.rs:31` |
| Serial consoles | UDMA UART2 is live at 1 Mbaud 8N1 on PB14=TX/PB13=RX. The separate TX-only DUART is at 0x40042000, but Dabao leaves its dedicated package pad unconnected | `README-baochip.md:28`; `README-consoles.md:5-16`; `dabao_v3c.kicad_pcb` DUART pad D3; `pad_frame_arm.sv` PAD_DUART |
| USB | Was MSC+CDC; IRQs now dead; boot1 asserts SE0 low on the normal boot path (next stage should de-assert if it wants USB) | `bao1x-boot/boot1/src/main.rs:386-430`, `repl.rs:152-161` |
| Watchdog | Off by default (only `oem-baosec-lite` enables WDT when on battery) | `bao1x-boot/boot1/.../bao1x.rs:516-529` |
| RAM (ACRAM) | Contains boot1 leftovers (stack/heap); only the next stage's own `.data/.bss` are cleared | `bao1x.rs:154-182` (boot1) / `baremetal .../bao1x.rs:60-72` (baremetal) |

### Could Zephyr link against/replace this?

Yes — Zephyr would *replace* the image in this slot; it does not link against the Rust crate. It must:

- Link ROM at `0x60060400` with reset entry as the *first* instruction (Zephyr's `__start` /
  `.text.__start` first in ROM, mirroring `.text.init` — `baremetal/.../link.x:53-65`).
- Put the entry trampoline at presign offset 0 (executes at `0x60060300`): either a real
  `StaticsInRom` (as `xous-copy-object` emits) or minimally `jal x0, +256` (`0x1000006f`) + padding,
  because the sigblock JAL lands at base+768 and expects to be carried to base+0x400.
- RAM at `0x61000000`..`0x61200000` (2 MiB) for `.data/.bss`/heaps; zero its own BSS.
- Provide its own `.data` initialization: the stock toolchain strips `.data` and only pokes ≤40 words
  (`tools/src/elf.rs:218-283`) — see risks in §7.

## 4. Loading over serial (boot1 console commands)

The console (DUART or USB-CDC) runs a line-based REPL. Full command table from
`bao1x-boot/boot1/src/repl.rs:136-1126`: `reset`, `boot`, `uf2`, `has-crc` (uf2-spim build),
`localecho`, `bootwait`, `paranoid`, `skipping`, `qe`, `bogomips`, `boardtype`, `altboot`, `idmode`,
`audit`, `lockdown`, `require-pq`, `baosec-init`, `ifr`, `publock`, `peek`, `pq`, `ate`, `atecheck`,
`echo`.

**Yes, it can upload code**: `uf2 <base64-of-one-512B-UF2-block>` writes one block per command to RRAM
after the same address/family checks as MSC (`repl.rs:163-210`; CRC-protected variant at `:211-320`).
BOOTCHAIN describes this as "custom base64 encoded serialization protocol ... 32-byte erase block size
of RRAM" (`BOOTCHAIN.md:60-68`). `peek` reads memory (`repl.rs:968`); there is no `poke`. The
`bio-loader` Python tool demonstrates the identical host-side protocol style for the BIO coprocessor
(`~/archive/baochip/bio-loader/bio-loader/bio_loader.py:4-15`) — a Zephyr CI uploader would clone that
pattern against `uf2`.

## 5. Emulation / simulation for CI

- **Renode**: no bao1x platform exists. `emulation/` contains only Precursor/betrusted scripts
  (`emulation/soc/betrusted-soc.repl`; README describes the Platonic device, `emulation/README.md:1-30`),
  and xtask `renode-*` targets map to the `renode` (Precursor) utra target
  (`xtask/src/builder.rs:367-373`). A bao1x Renode platform (REPL + DUART/UDMA/IRQ-array/timer models,
  RRAM/ACRAM/XIP maps) would have to be written; Renode's RV32 CPU can execute RV32IMAC fine, and GDB
  attach on :3333 works there (`emulation/README.md:33-34`).
- **Verilator (exists today)**: `~/archive/baochip/baochip-1x/verilate/verilate.sh` builds the RTL sim
  (vexi/vexii cores) and either runs full Xous (`cargo xtask bao1x-sim`, `verilate.sh:93-117`) or
  **raw iron** images from `deps/nto-tests` via `cargo xtask boot-image` with a straight link script
  (`verilate.sh:118-146`). Sim memory map matches silicon: reram `0x60000000/4M`, sram `0x61000000/2M`,
  xip `0x70000000/128M`, plus a VexRiscv debug link at `0xefff0000`
  (`verilate/bao_common.py:96-102`). The CPU reset vector is `0x60000000 + --boot-offset` (ReRAM
  trimming bits, `verilate/bao_core_vexii.py:45-52`, `bao_soc.py:482`), and the `--bios` image is placed
  at RRAM offset 0 (`bao_common.py:191-197`). verilate.sh's own comment: "--boot-offset ... to match
  what is in link.x" (`verilate.sh:126-127`). **So arbitrary RV32IMAC images (e.g. Zephyr) already run
  under verilator** — link at RRAM base (no sigblock/JAL trampoline needed) or match boot-offset to the
  link origin; `mkimage.py` shows the image-assembly style (`verilate/mkimage.py`).

## 6. Debug

- **JTAG**: physically present but fused/locked: boot0 is written via "specialized JTAG commands" in the
  CP (wafer-probe) state, sealed by IFR bits that "fuse out" JTAG (`BOOTCHAIN.md:12`, `:33-35`);
  `ifr_0x280_jtag_disa.bin` locks JTAG write access to boot0 (`bao1x-boot/blobs/README.md`). boot1
  verifies IFR state showing the Cortex-M7 and hardware JTAG debug are disabled
  (`bao1x-boot/boot1/src/secboot.rs:43-54`). No OpenOCD/SWD/trace configs exist anywhere in-tree
  (no `*.cfg`/`*.tcl` matches). In sim, the VexRiscv debug module sits at `0xefff0000`
  (`verilate/bao_common.py:101`).
- **Debug UARTs**: UDMA UART2 drives PB14 (TX) / PB13 (RX) at **1,000,000 baud 8N1** and 3.3 V on Dabao
  (`README-baochip.md:28`; `README-consoles.md:9-16`; `libs/bao1x-api/src/lib.rs:46`). The separate
  TX-only DUART at `0x40042000` is trivial to drive and visible in Verilator, but its dedicated package
  pad is unconnected on Dabao (`pad_frame_arm.sv`; `dabao_v3c.kicad_pcb`). USB CDC-ACM dies with IRQs
  disabled at handoff.
- Xous itself debugs via prints/consoles + hosted-mode emulation (`baosec-emu`, `README-baochip.md:244-246`).

## 7. Zephyr boot plan sketch (shortest path)

1. **Zephyr SoC/port**: RV32IMAC M-mode (VexRiscv-class, custom IRQ arrays — e.g. `irqarray5` at
   `0xe0013000`, `timer0` at `0xe001c000`, no CLINT/PLIC — `utralib .../bao1x.rs:360-379`), console on
   DUART @ `0x40042000` for simulation output, polling UDMA UART2 @ `0x50103000` for Dabao output,
   tick from `timer0`.
2. **Linker**: ROM `ORIGIN 0x60060400`, `LENGTH ≈ 0x3F800` (leave ~4 KiB headroom at the end for the
   optional 3856-byte PQ signature that is postpended after the payload); RAM `0x61000000`/2 MiB. Ensure
   the reset entry is the first byte of ROM.
3. **Pack**: either (a) reuse `xous-copy-object` (gives `StaticsInRom` + ≤40-word poke table — Zephyr
   `.data` must then be all-zero or tiny), or (b) emit our own presign blob:
   `[0x1000006f + 252 B pad][flat ROM incl. .data LMA image]` and let Zephyr's `__start` self-copy
   `.data` like a normal XIP build (the signer accepts any presign bytes — it only prepends the header,
   `tools/src/sign_image.rs:451-465`).
4. **Sign** (inside xous-core checkout, or pass `--git-describe`):
   `cargo run -p xous-tools --bin xous-sign-image -- --loader-image zephyr-presign.img --loader-key
   devkey/dev.key --loader-output zephyr.img --min-xous-ver v0.9.8-790 --sig-length 768 --with-jump
   --bao1x --function-code baremetal [--pq-key devkey/dev-pq.key --pq-key-cache devkey/dev-pq.cache]`.
   Classical-only is fine unless `require-pq` was set on the device. `.uf2` is emitted automatically.
5. **Deliver**: hold PROG (or `bootwait` mode) → copy `zephyr.uf2` to the `BAOCHIP` MSC volume →
   `sync`/eject → PROG again → boot1 validates (function code `baremetal`, devkey slot 3) and jumps.
   First boot of a dev image trips `DEVELOPER_MODE` (secrets erased; irreversible) — use a dev board.
6. **CI emulation**: verilator flow from `~/archive/baochip/baochip-1x/verilate/` with Zephyr linked at
   RRAM base and `--boot-offset 0` (per `verilate.sh` nto-tests precedent). A Renode bao1x platform is a
   possible later investment for fast functional CI.

### Open risks

- **`.data` initialization**: stock tooling strips `.data` (poke table ≤40 words, u16 offsets) —
  `tools/src/elf.rs:222-283`, `xous-copy-object.rs:66-70`. Zephyr builds with real initialized `.data`
  (device structs, etc.); plan on custom packing (3b) or verified-zero `.data`.
- **Entry trampoline**: hardware ignores `e_entry`; the word at presign offset 0 must jump the CPU from
  `0x60060300` to `0x60060400`; also JAL immediates in header/trampoline must stay in range (they do:
  +768/+256).
- **Size cap**: baremetal slot code budget ≈ 254 KiB (and −3856 B if PQ-signed) before the kernel
  sigblock at `0x6009FD00` (`common.rs:13`); a full-featured Zephyr could get tight — trim Kconfigs, or
  consider the kernel/swap slots later (those are Xous-specific formats today).
- **Non-standard interrupt/timer architecture**: IRQ arrays + CSR-block timer, no CLINT/PLIC/mtime —
   a custom Zephyr SoC + drivers are required; also confirm VexRiscv VexII CSR/extension subset
   (Zicsr/Zifencei) assumed by Zephyr's RV port.
- **Handoff hygiene**: SE0 pin (PC13 on dabao) may be left driven low; USB registers configured but
  IRQ-dead; caches/mtvec/sp must be reset by Zephyr before first exception/timer use
  (`bao1x-boot/boot1/src/main.rs:400-401`, `secboot.rs:86`).
- **Anti-rollback & require-pq**: dev-image `anti_rollback` must be ≤ the device's OWC value (default
  config writes 1 — `signing/anti-rollback.hjson`); don't casually raise it, RRAM OWC wear is limited
  (`anti-rollback.hjson` header comment).

## Sources

- `~/archive/betrusted-io/xous-core/README-baochip.md`
- `~/archive/betrusted-io/xous-core/README-consoles.md`
- `~/archive/betrusted-io/xous-core/bao1x-boot/BOOTCHAIN.md`
- `~/archive/betrusted-io/xous-core/bao1x-boot/PQ.md`
- `~/archive/betrusted-io/xous-core/bao1x-boot/blobs/README.md`
- `~/archive/betrusted-io/xous-core/bao1x-boot/boot0/src/main.rs`
- `~/archive/betrusted-io/xous-core/bao1x-boot/boot0/src/platform/bao1x/bao1x.rs`
- `~/archive/betrusted-io/xous-core/bao1x-boot/boot1/src/main.rs`
- `~/archive/betrusted-io/xous-core/bao1x-boot/boot1/src/repl.rs`
- `~/archive/betrusted-io/xous-core/bao1x-boot/boot1/src/secboot.rs`
- `~/archive/betrusted-io/xous-core/bao1x-boot/boot1/src/uf2.rs`
- `~/archive/betrusted-io/xous-core/bao1x-boot/boot1/src/asm.rs`
- `~/archive/betrusted-io/xous-core/bao1x-boot/boot1/src/platform/bao1x/link.x`
- `~/archive/betrusted-io/xous-core/bao1x-boot/boot1/src/platform/bao1x/bao1x.rs`
- `~/archive/betrusted-io/xous-core/bao1x-boot/boot1/src/platform/bao1x/usb/handlers.rs`
- `~/archive/betrusted-io/xous-core/baremetal/Cargo.toml`
- `~/archive/betrusted-io/xous-core/baremetal/build.rs`
- `~/archive/betrusted-io/xous-core/baremetal/src/asm.rs`
- `~/archive/betrusted-io/xous-core/baremetal/src/main.rs`
- `~/archive/betrusted-io/xous-core/baremetal/src/platform/bao1x/link.x`
- `~/archive/betrusted-io/xous-core/baremetal/src/platform/bao1x/bao1x.rs`
- `~/archive/betrusted-io/xous-core/loader/src/asm.rs`
- `~/archive/betrusted-io/xous-core/loader/src/platform/bao1x/link.x`
- `~/archive/betrusted-io/xous-core/libs/bao1x-api/src/lib.rs`
- `~/archive/betrusted-io/xous-core/libs/bao1x-api/src/signatures.rs`
- `~/archive/betrusted-io/xous-core/libs/bao1x-api/src/offsets.rs`
- `~/archive/betrusted-io/xous-core/libs/bao1x-api/src/offsets/common.rs`
- `~/archive/betrusted-io/xous-core/libs/bao1x-api/src/offsets/dabao.rs`
- `~/archive/betrusted-io/xous-core/libs/bao1x-api/src/pubkeys/bao1.rs`
- `~/archive/betrusted-io/xous-core/libs/bao1x-hal/src/sigcheck.rs`
- `~/archive/betrusted-io/xous-core/xtask/src/main.rs`
- `~/archive/betrusted-io/xous-core/xtask/src/builder.rs`
- `~/archive/betrusted-io/xous-core/tools/src/elf.rs`
- `~/archive/betrusted-io/xous-core/tools/src/sign_image.rs`
- `~/archive/betrusted-io/xous-core/tools/src/bin/xous-copy-object.rs`
- `~/archive/betrusted-io/xous-core/tools/src/bin/xous-sign-image.rs`
- `~/archive/betrusted-io/xous-core/signing/anti-rollback.hjson`
- `~/archive/betrusted-io/xous-core/devkey/README.md`
- `~/archive/betrusted-io/xous-core/utralib/src/generated/bao1x.rs`
- `~/archive/betrusted-io/xous-core/emulation/README.md`
- `~/archive/baochip/baochip-1x/verilate/verilate.sh`
- `~/archive/baochip/baochip-1x/verilate/bao_common.py`
- `~/archive/baochip/baochip-1x/verilate/bao_soc.py`
- `~/archive/baochip/baochip-1x/verilate/bao_core_vexii.py`
- `~/archive/baochip/baochip-1x/verilate/mkimage.py`
- `~/archive/baochip/bio-loader/bio-loader/bio_loader.py`
- `~/archive/betrusted-io/dabao-base-app/README.md`
