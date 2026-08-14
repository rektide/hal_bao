# hal_bao

Zephyr RTOS support material for the [Baochip 1x](https://github.com/baochip) SoC
(BAO1X2S4F-WA) and the [Dabao](https://github.com/baochip/dabao) development board.

hal_bao is the project home for bringing Zephyr up on Baochip hardware. The chip's
only existing OS is [Xous](https://github.com/betrusted-io/xous-core) (Rust); there
is no vendor C SDK. Consequently hal_bao is *not* a classic vendor-HAL mirror like
`hal_stm32` — instead it holds:

- **Vendored generated hardware descriptions** (`include/bao1x_peri.h`,
  `include/bao1x_peri.svd`) extracted from the chip RTL by the
  [`rtl_to_svd.py`](https://github.com/baochip/baochip-1x/blob/main/rtl/scripts/headergen/rtl_to_svd.py)
  headergen flow, with provenance recorded in `include/PROVENANCE.md`.
- **Boot image tooling** (`tools/`): devkey signing wrapper and a
  `.data`-capable image packer (the stock `xous-copy-object` strips `.data`,
  which a Zephyr image cannot tolerate).
- **Tickets and planning** (`.beads/`) for the port effort, tracked as
  `bao-*` issues.
- **Design docs** (`.design/`) narrating the effort.
- **Bring-up guides** (`doc/bringup/`) cover artifact validation and hardware
  recovery procedures.

The Zephyr code itself (SoC port, drivers, devicetree, Dabao board) lives in the
Zephyr fork at `~/src/zephyr-baochip` — new files only, so it rebases cleanly
over upstream while we decide whether to move the port out-of-tree into this
module (via `zephyr/module.yml` `soc_root`/`dts_root`/`board_root`) or push it
upstream. `zephyr/module.yml` is currently a stub for that future.

## Research corpus

The research that motivated every decision here lives in
[`.design/research/`](.design/research/README.md) (durable copy: shared doc
repo, `~/a/doc/bao/`), start at `04-synthesis.md`:

- `00-soc-inventory.md` — CPU, memory map, peripherals, interrupt model, clocks
- `01-boot-delivery.md` — boot chain, image format, signing, UF2, emulation
- `02-zephyr-integration.md` — SoC/board port mechanics, module split analysis
- `03-rust-survey.md` — Rust support boundaries
- `04-synthesis.md` — decisions, phased plan (M0-M5), risk register

## Layout

```
include/       vendored generated register headers + SVD (CERN-OHL-W-2.0)
tools/         image signing/packing + flashing helpers
zephyr/        Zephyr module manifest (stub; see module.yml)
.design/       design waves and narrative
  init/       the story of the effort (read first)
  research/   wave-0 research corpus (docs 00–04)
.beads/        ticket database (bao-* issues)
doc/bringup/   manual hardware validation and recovery guides
```

## License

Project code: Apache-2.0. Vendored Baochip artifacts: CERN-OHL-W-2.0 (see
`LICENSES/`), retained from the baochip-1x repository they were generated in.
