# Baochip 1x / Dabao SoC inventory for a Zephyr port

Hardware inventory distilled from the chip repo (`~/archive/baochip/baochip-1x`), the
generated register maps (SVD + utralib), and the Xous OS sources (`~/archive/betrusted-io/xous-core`).
Every claim cites `path:line`. `utralib/bao1x.rs` below means
`~/archive/betrusted-io/xous-core/utralib/src/generated/bao1x.rs`; `SVD` means
`~/archive/baochip/baochip-1x/rtl/scripts/headergen/output/bao1x_peri.svd`.

Part: **BAO1X2S4F-WA** — TSMC22ULL, VexRiscv RV32IMAC(Zkn) @ "400MHz", 2MiB SRAM, 4MiB RRAM
(ch00-00-introduction.md:3-8). The "2S4F" in the part number = 2MiB SRAM / 4MiB Flash(RRAM).

---

## 1. CPU core

VexRiscv ("VexRiscvAxi4"), generated from `~/archive/baochip/baochip-1x/VexRiscv/GenCramSoC.scala`
(same dir also holds `VexRiscv_CramSoC.v`, `VexRiscv_CramSoC.yaml`, `GenCramSoC.scala`,
`memory_AesZknPlugin_rom_storage_Rom_1rs.v`). RTL copy under `rtl/modules/vexriscv/`.

* **ISA**: RV32**IMAC** plus **Zkn** AES instructions:
  * `compressedGen = true` (RVC) — GenCramSoC.scala:90
  * `MulPlugin`, `DivPlugin` — GenCramSoC.scala:157-158
  * `AesZknPlugin` — GenCramSoC.scala:159 (AES round ROM:
    `VexRiscv/memory_AesZknPlugin_rom_storage_Rom_1rs.v`; chaffing/AES constant-time use
    discussed in docs/src/ch00-00-rtl-overview.md:97-111)
  * **A extension**: `withAmo = true, withLrSc = true`, `withExclusive = false` —
    GenCramSoC.scala:123-126
* **Privilege modes**: M/S/U with Sv32 paging — `CsrPlugin(CsrPluginConfig.linuxFull(mtVecInit =
  0x60000000))` + `MmuPlugin(exportSatp = true)` (GenCramSoC.scala:160-183).
  * The book claims "Sv39" (ch00-00-introduction.md:4) — **impossible on RV32; it is Sv32**.
    Confirmed by Xous kernel which uses 9-bit PID inside Sv32 PTEs
    (kernel/src/arch/riscv/irq.rs:384) and `satp` (kernel/src/arch/riscv/irq.rs:397).
  * MMU `ioRange` marks top-nibble 0x4/0x5/0xA-0xF as non-cacheable (GenCramSoC.scala:171-182),
    i.e. all peripherals (0x40..0x5F) and CSR space (0xE0..) are uncached by translation.
* **Caches**: 16KiB I$ and 16KiB D$, both 4-way, 32B lines (GenCramSoC.scala:93,114;
  yaml `size: 16384, bytePerLine: 32` VexRiscv_CramSoC.yaml). iBus memDataWidth 64.
* **Interrupt interface**: `ExternalInterruptArrayPlugin` — GenCramSoC.scala:184-190.
  A **32-line bit-masked IRQ array**, **not a PLIC**. Custom CSRs:
  * machine: `machineMaskCsrId = 0xBC0` ("MIM"), `machinePendingsCsrId = 0xFC0` ("MIP")
  * supervisor: `supervisorMaskCsrId = 0x9C0` ("SIM"), `supervisorPendingsCsrId = 0xDC0` ("SIP")
  * Xous boot1 drives MIM/MIP as raw CSRs (bao1x-boot/boot1/src/platform/bao1x/irq.rs:4,23,34,198);
    the Xous kernel drives SIM/SIP (`csrrs 0x9C0`, `csrrs 0xDC0`) —
    kernel/src/arch/riscv/irq.rs:37-51. Note the mask register is *inverse-logic*: setting a
    bit **enables** the IRQ (kernel/src/arch/riscv/irq.rs:100-110).
* **Debug**: JTAG `DebugPlugin`, 4 hardware breakpoints (GenCramSoC.scala:191-194;
  yaml `hardwareBreakpointCount: 4`).
* **Reset/trap vectors**: `mtVecInit = 0x60000000` (GenCramSoC.scala:161) — matches RRAM base;
  `LITEX_CONFIG_CPU_RESET_ADDR = 1610612736` = 0x60000000 (utralib/bao1x.rs:5449). Single
  trap vector (`stvec`), no vectored table (kernel/src/arch/riscv/asm.S:28-29).
* **Frequency**: CPU clock = fclk/2 (boot1 banner "CPU @ {}MHz!" uses `fclk_freq / 2_000_000`,
  bao1x-boot/boot1/src/platform/bao1x/bao1x.rs:514; comment "generally [PLL target] is 2x of the
  CPU clock frequency", libs/bao1x-hal/src/clocks.rs:66-70). Dabao ships 700MHz fclk → **350MHz
  CPU** (offsets/dabao.rs `DEFAULT_FCLK_FREQUENCY: 700_000_000`, x86 cite below); rated 400MHz CPU
  (=800MHz fclk; `LITEX_CONFIG_CLOCK_FREQUENCY: 800000000` utralib/bao1x.rs:5447). boot0 runs the
  chip at reduced 200MHz for max compatibility (bao1x-boot/BOOTCHAIN.md:13).

---

## 2. Memory map

Physical regions from utralib/bao1x.rs:247-348 (`HW_*_MEM` consts), cross-checked against linker
scripts (xous-core `loader/src/platform/bao1x/link.x:1-5`, `bao1x-boot/boot1/src/platform/bao1x/link.x:1-7`,
`bao1x-boot/boot0/link.x:1-7`, `kernel/link.x:1-5`) and RRAM partition offsets
(libs/bao1x-api/src/offsets/common.rs:8-16).

| Region | Base | Size | Notes / citations |
|---|---|---|---|
| RRAM ("RERAM", non-volatile) | 0x6000_0000 | 4 MiB (0x400000) | utralib:319-320; boot slots below |
| — boot0 | 0x6000_0000 | 128 KiB | BOOT0_START common.rs:8; R/O, OSAT-burned (BOOTCHAIN.md:11-12) |
| — boot1 | 0x6002_0000 | 256 KiB | BOOT1_START common.rs:9; boot1/link.x:5 (0x60020400, 255k-3856) |
| — loader/baremetal | 0x6006_0000 | 256 KiB | LOADER_START common.rs:10; loader/link.x:3 |
| — kernel | 0x600A_0000−SIG | — | KERNEL_START common.rs:13 |
| — data slots | 0x603E_0000 | 64 KiB | offsets.rs:80-81 |
| — ACRAM data slots | 0x603D_C000 | 8 KiB | offsets.rs:85-86 |
| — security vectors | 0x603D_A000..0x6040_0000 | ~150 KiB | RRAM_STORAGE_LEN 0x3DA000 common.rs:15-16 (OWC/keyslots above storage) |
| IFR (info. row/fuses, CP_ID) | 0x6040_0000 | 0x400 | offsets.rs:87-92 |
| SRAM (main RAM) | 0x6100_0000 | 2 MiB | utralib:317-318; linker RAM ORIGIN 0x61000000 LEN 2M (loader/link.x:4) |
| IFRAM0 (uDMA buffer SRAM) | 0x5000_0000 | 128 KiB | utralib:303-304 |
| IFRAM1 (uDMA buffer SRAM) | 0x5002_0000 | 128 KiB | utralib:305-306 |
| NULL region (reads 0) | 0x5004_0000 | 64 KiB | utralib:307-308 |
| UDMA peripheral CSRs | 0x5010_0000 | 128 KiB | utralib:309-310 |
| IFSUB (PWM/SDDC/APB_THRU/IOX/BIO…) | 0x5012_0000 | 12 KiB | utralib:253-254 |
| SDDC data buffer | 0x5014_0000 | 64 KiB | utralib:313-314 |
| AORAM (always-on RAM) | 0x5030_0000 | 16 KiB | utralib:347-348 |
| USB device controller (UDC) | 0x5020_0000 | 64 KiB | utralib:315-316; Corigine dev regs @ 0x50202000 (usb/utra.rs:197) |
| CPU CSR space (LiteX core complex) | 0xE000_0000 | 256 KiB | utralib:247-248 (`HW_CSR_MEM`) |
| RRC (coresub reset/clock) | 0x4000_0000 | 64 KiB | utralib:257-258 |
| CORESUB (QFC/PL230/MDMA/MBOX/SRAMTRM) | 0x4001_0000 | 64 KiB | utralib:255-256 |
| SCE (crypto engine + keyslots) | 0x4002_8000 | 32 KiB | utralib:249-250; keyslot SEGs 0x40020000-0x400223xx utralib:267-296 |
| SYSCTRL (CGU "daric") | 0x4004_0000 | 64 KiB | utralib:251-252 |
| SECSUB (MESH/SENSORC/GLUECHAIN) | 0x4005_0000 | 64 KiB | utralib:259-260 |
| AO (always-on) | 0x4006_0000 | 64 KiB | utralib:263-264; AOPERI 0x40061000 len 20480 utralib:265-266 (RTC PL031 lives here, rtc.rs:144) |
| XIP (QSPI memory-mapped flash) | 0x7000_0000 | 64 MiB | utralib:321-322 (Dabao: unused, no external flash) |
| Xous kernel virtual (FLASH) | 0xffd0_0000 | 512 KiB | kernel/link.x:3-4 (Sv32 virtual, maps RRAM) |
| Xous kernel virtual (RAM) | 0xffd8_0000 | 512 KiB | kernel/link.x:3 |

SVD base addresses match utralib exactly for all 51 SVD peripherals (cross-checked by script;
the SVD decimal bases, e.g. `1073811456` = 0x40011000 PL230, SVD:15). The SVD is explicitly
*partial* — "The rest of the SVD file is generated by Litex upon compiling the CPU core complex"
(docs/src/ch00-00-rtl-overview.md:13), which is why utralib has 20 extra modules (d11ctime,
susres, coreuser, csrtest, irqarray0-19, mailbox, mb_client, resetvalue, ticktimer, timer0).

### RRAM layout at a glance (all in 4MiB @ 0x6000_0000)
boot0 (0x0-0x20000, R/O) → boot1 (0x20000-0x60000) → loader/baremetal (0x60000-0xA0000) →
kernel (0xA0000) → [user area; Dabao `APP_RRAM_START` = 0x6030_0000-SIG, offsets/dabao.rs:22-23] →
ACRAM slots (0x3DC000) → data slots (0x3E0000) → security vectors to top.

---

## 3. Peripherals

Two register universes:
1. **LiteX core-complex CSRs @ 0xE000_xxxx** (generated by LiteX; utralib only): ticktimer,
   timer0, susres, mailbox, mb_client, d11ctime, coreuser, csrtest, irqarray0-19.
2. **SoC peripherals @ 0x4000_xxxx / 0x501x_xxxx** (SVD + `rtl/scripts/headergen/output/doc/*.rst`).

Xous driver locations: `libs/bao1x-hal/src/*` (HAL, kernel+userspace), `services/*` (Xous
servers), `bao1x-boot/*` (bootloaders), `loader/`, `baremetal/`.

### 3.1 Full peripheral table

| Name | Base | Function | Xous driver | RTL |
|---|---|---|---|---|
| RRC | 0x4000_0000 | reset/clock controller (coresub) | minimal | rtl/modules/rrc/ |
| QFC | 0x4001_0000 | QSPI flash controller (XIP master) | rram.rs? (Dabao unused) | doc/qfc.rst |
| PL230 | 0x4001_1000 | ARM PL230 DMA (8 ch, AHB) | xous-pl230 crate | ips/…, SVD:15-17 |
| MDMA | 0x4001_2000 | memory DMA + event select | — | doc/mdma.rst |
| MBOX_APB | 0x4001_3000 | APB mailbox | libs/bao1x-hal/src/mbox.rs | doc/mbox_apb.rst |
| CORESUB_SRAMTRM | 0x4001_4000 | SRAM trim/wait-states | sram_trim.rs | doc/coresub_sramtrm.rst |
| **SCE keyslots** | 0x4002_0000-0x4002_23FF | LKEY/KEY/SKEY/SCRT/MSG/HOUT/SOB/PKB/PIB/POB/PSOB/AKEY/AIB/AOB/RNGA/RNGB segments (256B-2KiB each) | libs/bao1x-hal/src/sce/* | rtl/modules/crypto_*/ |
| SCE_GLBSFR | 0x4002_8000 | crypto engine global SFRs | sce.rs | crypto_top/ |
| COMBOHASH | 0x4002_B000 | hash (SHA-2 family; “combo”) | sce/hash.rs | crypto_hash/ |
| PKE | 0x4002_C000 | public-key engine (ECDSA/Ed25519, big-int) | sce.rs (ed25519-dalek-bao1x) | crypto_pke/ |
| AES | 0x4002_D000 | AES engine | sce/aes.rs | crypto_aes/ |
| TRNG | 0x4002_E000 | true RNG | sce/trng.rs (boot1/trng.rs) | crypto_trng/ |
| ALU | 0x4002_F000 | crypto ALU (modular add/mul helper) | sce.rs | crypto_alu/ |
| SCEDMA | 0x4002_9000 | SCE DMA | sce.rs | crypto_top/ |
| SYSCTRL ("daric CGU") | 0x4004_0000 | clock gen: PLL/dividers/gates/reset (SFR_RCURST0!) | clocks.rs | sysctrl/ |
| WDG_INTF | 0x4004_1000 | **watchdog** = ARM CMSDK APB WDT (LOCK @+0xC00, magic 0x1ACCE551; feed=0x5A to INTCLR) | wdt.rs:116,28-60 | ips/timer_unit |
| DUART | 0x4004_2000 | debug UART (1-pin TX) | debug.rs | modules/*/duart.sv |
| TIMER_INTF | 0x4004_3000 | (standard CMSDK timer, no SVD regs) | — | ips/timer_unit |
| EVC | 0x4004_4000 | event control (CM7/timer event routing) | — | doc/evc.rst |
| RBIST_WRP | 0x4004_5000 | SRAM trim/MBIST wrapper | sram_trim.rs (writes SFRCR_TRM/SFRAR_TRM) | rbist/ |
| MESH | 0x4005_2000 | security mesh config | hardening.rs | sec/rtl/mesh.sv |
| SENSORC | 0x4005_3000 | security sensors (VD masks) | hardening.rs | sec/rtl/sensorc.sv |
| GLUECHAIN | 0x4005_4000 | glitch/lockup chain | hardening.rs | sec/rtl/gluechain.sv |
| AO_SYSCTRL | 0x4006_0000 | always-on: PMU/LDO trim, wake masks, RTC clock div, AOPADPU | clocks.rs, boot1 | ao/ |
| AOBUREG | 0x4006_5000 | 8× always-on backup regs (u32) | buram.rs (BackupManager) | ao/rtl/aobureg.sv |
| DKPC | 0x4006_4000 | debug key protection? (dkpcen ctl) | — | ao/rtl/dkpc.sv |
| **RTC (PL031)** | 0x4006_1000 | ARM PL031 RTC (DR/MR/LR/CR/IMSC/RIS/MIS/ICR @0xFE0/0xFF0) | rtc.rs:144 | ao/ (proprietary) |
| UDMA_CTRL | 0x5010_0000 | uDMA global: REG_CG (clock gate), REG_CFG_EVT (event routing), REG_RST | udma/mod.rs:46-160 | ips/udma |
| UDMA_UART_0..3 | 0x5010_1000/2000/3000/4000 | 4 UARTs (RX/TX DMA descriptors + REG_UART_SETUP, REG_IRQ_EN) | udma/uart.rs | ips/udma |
| UDMA_SPIM_0..3 | 0x5010_5000..8000 | 4 SPI masters | udma/spim.rs | ips/udma |
| UDMA_I2C_0..3 | 0x5010_9000..C000 | 4 I2C masters | udma/i2c.rs | ips/udma |
| UDMA_SDIO | 0x5010_D000 | SDIO host (uDMA) | — | ips/udma |
| UDMA_I2S | 0x5010_E000 | I2S | — | ips/udma |
| UDMA_CAMERA | 0x5010_F000 | camera (DVP) | gc2145/, ov2640/ | ips/udma |
| UDMA_FILTER | 0x5011_0000 | filter coproc | — | ips/udma |
| UDMA_SCIF | 0x5011_1000 | smart card if | — | ips/udma |
| UDMA_SPIS_0/1 | 0x5011_2000/3000 | 2 SPI slaves | — | ips/udma |
| UDMA_ADC | 0x5011_4000 | ADC (temp sensor + PA04-PA07 ext) | udma/adc.rs | ips/udma |
| PWM | 0x5012_0000 | 4× adv timers × 4 ch, LUT/pattern mode | — (pins show PWM1.x/2.x) | ifsub/ |
| SDDC | 0x5012_1000 | CrossBar SD device/host ctrl (clk/cmd/dat0-3 + 64K buffer @0x5014_0000) | — | ifsub/rtl/sddc.sv:16-40 |
| APB_THRU / UDP | 0x5012_2000 | USB PHY regs? (4K window; shares HW_UDP_MEM) | usb/ | ifsub/ |
| BIO_BDMA | 0x5012_4000 | BIO subsystem ctrl (4 cores, FIFOs, QDIVs) | libs/xous-bio-bdma/ | bio_bdma/rtl/bio_bdma.sv |
| BIO_IMEM0..3 | 0x5012_5000..8000 | 4 KiB each, per-core instruction mem | xous-bio-bdma/src/lib.rs:112-133 | bio_bdma/ |
| BIO_FIFO0..3 | 0x5012_9000..C000 | FIFO CSRs (also mapped as core x16-x19 regs) | xous-bio-bdma | bio_bdma/ |
| IOX (GPIO) | 0x5012_F000 | 6 ports × 16 pins GPIO + AF mux + IRQ | iox.rs | ifsub/rtl/iox.sv |
| **Corigine USB dev (UDC)** | 0x5020_0000 (regs @ +0x2000) | USB2 + USB3(Gen1/Gen2) device ctrl, xHCI-style rings/doorbells | usb/driver.rs, services/usb-bao1x | (closed) |
| d11ctime | 0xE000_0000 | free-running count + heartbeat bit | loader delay (loader/…/bao1x.rs:42-47) | LiteX core |
| susres | 0xE000_1000 | suspend/resume control | services/susres | LiteX core |
| coreuser | 0xE000_2000 | coreuser LUT: maps address regions to user IDs (per-process perms) | coreuser.rs | LiteX core |
| csrtest | 0xE000_3000 | CSR r/w test block | — | LiteX core |
| irqarray0..19 | 0xE000_4000 + n*0x1000 (order 0,1,10..19,2..9) | 16-event IRQ banks (see §4) | kernel + all servers | LiteX core |
| mailbox | 0xE001_8000 | host-mailbox (word in/out, 2048-deep) | bao1x-mbox2 service | LiteX core |
| mb_client | 0xE001_9000 | mailbox client side | same | LiteX core |
| resetvalue | 0xE001_A000 | PC reset value readout | — | LiteX core |
| **ticktimer** | 0xE001_B000 | 64-bit wall-clock timer + alarm | xous-ticktimer service | LiteX core |
| timer0 | 0xE001_C000 | 32-bit periodic timer | boot1 delay (bao1x.rs:536-585) | LiteX core |

### 3.2 UART (console)

Two distinct UART systems:

* **DUART** @ 0x4004_2000 — debug one-liner: `SFR_TXD` (+0x00), `SFR_CR` (+0x04, bit0 = clock
  enable), `SFR_SR` (+0x08, busy), `SFR_ETUC` (+0x0C, tick divider; boot0 writes 34 on ring
  osc, then FREQ_OSC_MHZ=48 on xtal — boot0/src/platform/bao1x/bao1x.rs:101-106,386-389 and
  clocks.rs:135-140). Poll-only; used for boot X's and panic prints
  (debug.rs:21-29,155-167). This is the "audit" console in the SVD doc duart.rst.
* **UDMA UART_2** @ 0x5010_3000 — **the serial console on Dabao**, pins PB14 (TX) / PB13 (RX),
  AF1, **1,000,000 baud 8N1** (`UART_BAUD = 1_000_000`, bao1x-api/src/lib.rs:46; pin setup
  libs/bao1x-hal/src/board/dabao.rs:67-89). Registers (SVD UDMA_UART_2):
  `REG_RX_SADDR/RX_SIZE/RX_CFG`, `REG_TX_SADDR/TX_SIZE/TX_CFG` (uDMA descriptors, buffer in
  IFRAM), `REG_STATUS` (+0x20), **`REG_UART_SETUP` (+0x24)** — value `0x0316 | (clkdiv << 16)`
  where clkdiv = perclk/baud (udma/uart.rs:127-146; 0x16 = 8N1 + poll mode), `REG_ERROR`,
  `REG_IRQ_EN` (+0x2C), `REG_VALID`, `REG_DATA`. TX/RX are buffer-DMA ("UDMA") style, not
  byte-FIFO style: you point RX_SADDR at an IFRAM buffer and read back asynchronously
  (setup_async_read / read_async, uart.rs; boot1 debug.rs:67-84). RX IRQ routed via
  UDMA_CTRL event channels (debug.rs:129-130) and taken on **irqarray5** (CPU line 5).
  UART1 (0x50102000) is a second console on Baosec (loader/…/bao1x.rs:289).

### 3.3 GPIO (IOX)

`rtl/modules/ifsub/rtl/iox.sv`, base 0x5012_F000. **6 ports (PA-PF) × 16 pins = 96 GPIOs**,
2-bit AFSEL per pin (AF0=GPIO, AF1/AF2/AF3 = peripheral). Evidence: AFSEL is 12×16bit regs =
96×2 bits with 2 regs per port (HAL set_alternate_function splits pins 0-7/8-15 per port,
iox.rs:87-180); per-port single regs for GPIOOUT/GPIOOE/GPIOPU/GPIOIN (6 regs each, iox.rs:31-83).

Registers (word offsets from SVD/utralib, utralib indices are ×4):
* `SFR_AFSEL_CRAFSEL0..11` +0x00-0x2C (2/pin function select)
* `SFR_INTCR_CRINT0..7` +0x100-0x11C (10-bit fields: interrupt config per pin — sense/polarity)
* `SFR_INTFR` +0x120 (8-bit raw IRQ flags → feeds PIOIRQ0-3)
* `SFR_GPIOOUT_CRGO0..5` +0x130-0x144 (output value, 16b/port)
* `SFR_GPIOOE_CRGOE0..5` +0x148-0x15C (output enable, 16b/port)
* `SFR_GPIOPU_CRGPU0..5` +0x160-0x174 (pull-up, 16b/port)
* `SFR_GPIOIN_SRGI0..5` +0x178-0x18C (input value, 16b/port)
* `SFR_PIOSEL` +0x200 (route pins to BIO vs IOX)
* `SFR_CFG_SCHM*` +0x230-0x244 (Schmitt trigger, 16b/port)
* `SFR_CFG_SLEW*` +0x248-0x25C (slew, 16b/port)
* `SFR_CFG_DRVSEL*` +0x260-0x274 (drive strength, 2b/4pins — Drive2mA/4mA enums in HAL)

Xous drives it via `setup_pin(port, pin, dir, function, schmitt, pullup, drive)` —
iox.rs:87+; note pull-up only (no pull-down), and per-pin drive strength (IoxDriveStrength).
BIO mapping: BIO0-31 ↔ PB0-15, PC0-15 (iox.rs:196-230 `set_ports_from_bio_bitmask`;
pins.csv rows BIO1..BIO29 → PB1..PB13, PC0..PC13).

### 3.4 BIO (4× PicoRV32 coprocessors)

`rtl/modules/bio_bdma/` (bio_bdma.sv, picorv32.v). 4 PicoRV32 cores (module bio_bdma.sv:1870,
NUM_MACH=4 :2035), each with private 4KiB IMEM (0x5012_5000..0x5012_8000), an 8-deep×32b FIFO
(windowed as CSR space BIO_FIFO0-3 and as RV32 custom regs x16-x19), quantum clock divider
(SFR_QDIV0-3: `DIV_FRAC`/`DIV_INT` bits, utralib:2944-2957; boot1 inits `0x1_0000`,
bao1x.rs:96-99), event/halt model via x27-x30, GPIO aliasing x21-x26, core id x31 — full CSR
contract in docs/src/ch01-00-bio-overview.md:7-39. Host side: BIO_BDMA regs incl.
`SFR_CTRL` (EN[4b], RESTART, CLKDIV_RESTART), `SFR_CONFIG` (SNAP routing, DISABLE_FILTER_PERI/MEM,
`CLOCKING_MODE` 2b), `SFR_FLEVEL`, `SFR_TXF0-3/RXF0-3` (FIFO head/tail access from APB),
`SFR_ELEVEL`, `SFR_SYNC_BYPASS`, `SFR_IO_OE_INV/O_INV/I_INV`, `SFR_IRQMASK_0-3`,
`SFR_IRQ_EDGE` (utralib:2858-2990). Xous: libs/xous-bio-bdma, libs/xous-bio, bio-lib; boot1
programs QDIV + clocking mode 3 (bao1x.rs:86-99). Dabao apps use BIO for WS2812/captouch
(dabao-base-app README:14-17). Host clock can be 1x or 2x CPU clock ("fast_bio",
clocks.rs:106-108, comment bao1x.rs:484-486).

### 3.5 Crypto engine (SCE) + TRNG + keyslots

Cluster at 0x4002_0000-0x4002_FFFF ("SCE" = secure crypto engine):
* SCE_GLBSFR (0x40028000, 21 regs) — global config/status, engine enable
* AES (0x4002D000, 13 regs) — AES block engine (CPU also has Zkn AES instructions; software
  chaffs between engine & instructions, ch00-00-rtl-overview.md:97-111)
* COMBOHASH (0x4002B000, 15 regs) — SHA family (Xous: sha2-bao1x crate)
* PKE (0x4002C000, 19 regs) — public key engine (Ed25519: ed25519-dalek-bao1x; PQ: slh-dsa-bao1x)
* ALU (0x4002F000) — bignum helper ops (SFR_CRFUNC)
* TRNG (0x4002E000, 15 regs) — health-checked TRNG (raw-gen mode used at boot,
  boot1/…/bao1x.rs:101-107)
* SCEDMA (0x40029000) — descriptor DMA for the above
* **Keyslots**: memory-mapped segments 0x40020000-0x400223FF: LKEY/KEY/SKEY/SCRT/MSG/HOUT/SOB
  (general purpose), PKB/PIB/POB/PSOB (PKE), AKEY/AIB/AOB (AES), RNGA/RNGB (1KiB each)
  (utralib:267-296). Populated at first boot from TRNG (offsets/common.rs:337-339 & slots.rs).

### 3.6 Timers / watchdog / RTC (Zephyr sys_clock candidates)

* **ticktimer** 0xE001_B000 — the Xous kernel clock. Regs (utralib:2047-2080):
  `CONTROL` (bit0 RESET), `TIME1/TIME0` (64-bit free-running count), `MSLEEP_TARGET1/0`
  (64-bit alarm compare), `EV_STATUS/EV_PENDING/EV_ENABLE` (ALARM bit, LiteX event style,
  write-1-to-clear), `CLOCKS_PER_TICK` (prescaler: Xous writes (fclk/2)/1000 so TIME counts ms —
  xous-ticktimer/src/platform/bao1x/implementation.rs:117-126). IRQ 20. One-shot alarm model:
  disable EV_ENABLE, write target, clear pending, enable (schedule_response, implementation.rs:208-228).
* **timer0** 0xE001_C000 — 32-bit down-counter w/ auto-reload: `LOAD`, `RELOAD`, `EN`,
  `UPDATE_VALUE`, `VALUE`, `EV_*_ZERO` (utralib:2081-2112). IRQ 30. Used by bootloaders for
  ms delays (boot1/…/bao1x.rs:536-550: RELOAD = sysclk/1000 × ms).
* **WDT (WDG_INTF)** 0x4004_1000 — ARM CMSDK APB watchdog: LOAD/VALUE/CONTROL(INTEN,RESEN)/
  INTCLR (**feed = write 0x5A**)/RAWINTSTAT/MASKINTSTAT, LOCK @+0xC00 (magic 0x1ACCE551)
  (wdt.rs:22-60,116). Xous enables 0x7FFFFFFF period ≈30s ±50% and feeds it from the ticktimer
  server (implementation.rs:128-134,232). Two-stage: 1st timeout IRQ, 2nd → hard reset.
* **RTC (PL031)** 0x4006_1000 — seconds counter w/ match+load, clocked via AO_SYSCTRL
  `CR_CLK1HZFD` divider (boot1 sets 15, bao1x.rs:127-129; AO_SYSCTRL @0x40060000 +0x04 SVD).
  Not battery-backed on Dabao; used as persistent-ish clock in Xous.
* **d11ctime** 0xE000_0000 — free counter + 1-bit heartbeat toggle (loader delay loop,
  loader/…/bao1x.rs:41-56).
* PWM @0x5012_0000 — 4 timer banks × 4 channels with threshold+LUT pattern generation
  (SVD REG_TIMn_CMD/CFG/CHx_TH/CHx_LUT, REG_EVENT_CFG, REG_CH_EN, REG_PREFD0-3) — usable as
  extra timers/PWM (pins PC0-3 = PWM2.0-2.3, PB1-3 = PWM1.1-1.3 per pins.csv).

### 3.7 ADC

UDMA_ADC @ 0x5011_4000: RX DMA regs + `REG_CR_ADC` (+0x10) with fields CHOPPER_EN, TEMP_BUF_EN,
BANDGAP_BUF_EN, EXT_BUF_EN, TEMP_V_CTRL, *_FILTER_BYPASS, DATA_COUNT, SENSOR_EN, ADC_EN,
ADC_RST, CLK_FD (`adc_clk = perclk / (2×FD)`, 0.2-1.6MHz), ADC_SEL (temp vs ext), VIN_SEL
(PA04=ADC0, PA05=ADC1, PA06=ADC2, PA07=ADC3) — libs/bao1x-hal/src/udma/adc.rs:31-80.
Samples stream to IFRAM buffer via uDMA RX.

### 3.8 USB

* Device controller: **Corigine** "CRG" UDC @ 0x5020_0000, device reg block @ 0x5020_2000
  (usb/utra.rs:197). xHCI-flavored: DEVCAP (version, EP counts, **Gen1+Gen2 = USB2+USB3 caps**),
  DEVCONFIG, EVENTCONFIG, USBCMD/USBSTS, DCBAP, PORTSC, DOORBELL, event rings, TRB-style
  processing (utra.rs:1-80; boot1/…/irq.rs:219-266 `process_event_ring`).
* IRQ on **irqarray1 bit0 `USBC_DUPE`** (and irqarray10 bit1 `USBC`).
* Buffers: 23 IFRAM1 pages + extended app page (dabao.rs:22-24).
* Dabao can force SE0 (bus park) by driving PC13 low as output — see §6.

---

## 4. Interrupt model

**No PLIC, no CLINT.** Two layers:

1. **Peripherals** → LiteX-style **event manager** banks `irqarray0..19` @
   0xE000_4000+i*0x1000 (physical order at lines 355-374: 0,1,10..19,2..9). Each bank has 6 regs:
   `EV_SOFT` (trigger, w1s `TRIGGER`), `EV_EDGE_TRIGGERED`, `EV_POLARITY`, `EV_STATUS`,
   `EV_PENDING` (RW1C), `EV_ENABLE` (16 bits) (utralib:545-612 for irqarray0). Edge vs level and
   polarity are programmable per event.
2. Each bank ORs into one CPU external interrupt **line n** of the 32-bit VexRiscv
   ExternalInterruptArray. Line assignment (utralib:5569-5593 + bank EV_STATUS field names):
   * 0-19 = irqarray0-19
   * 20 = **ticktimer** (`TICKTIMER_IRQ`, utralib:2079)
   * 21 = susres (SOFT_INT) `SUSRES_IRQ`
   * 22 = mailbox, 23 = mb_client
   * 30 = timer0 (`TIMER0_IRQ`, utralib:2110)
   * (24-29, 31 unused by Xous)

**Event → bank routing** (index `LITEX_IFSUB_EV_*` → bank = idx/16, bit = idx%16;
utralib:5456-5561): e.g. UART2_RX=88 → bank5 bit8; IOXIRQ=160 → bank10 bit0; USBC=161 →
bank10 bit1; SDDCIRQ=162; PIOIRQ0-3=163-166 (GPIO!); TRNG_DONE=48 → bank3 bit0 (AES/PKE/HASH/ALU
follow); QFCIRQ=32, MDMAIRQ=33, MBOX=34-37, AOWKUPINT=47, SDIO=128-131, I2S=132/133, CAM=136,
ADC=137, PWM0-3_EV=156-159, SEC0=240 → bank15 bit0, I2C NACK/ERR=200-207 → bank12. Many banks
are deliberate *duplicates* (`*_DUPE` fields) for glitch resistance.

**UDMA event routing**: UDMA_CTRL `REG_CFG_EVT` (+0x04) maps any peripheral event id to one of
4 "EventChannel"s (udma/mod.rs:120-150); boot1 maps Uart2 Rx/Tx → Channel0/1 (debug.rs:129-130).

**Software flow (Xous)**: boot chain runs M-mode with `mtvec`/MIM/MIP CSRs and `mie.MEIE`
(bao1x-boot/boot1/…/irq.rs:11-37,197-206). Hand-off to Xous writes `mideleg/medeleg = 0xffffffff`
so *everything* traps to S-mode, and reinstalls mtvec=abort (irq.rs:45-56). The kernel takes
`SupervisorExternalInterrupt`/`UserExternalInterrupt` at a single `_start_trap` (asm.S:83-144),
computes `sip & sim` (custom CSRs 0xDC0/0x9C0) to get a 32-bit pending mask, and dispatches
userspace ISR callbacks via `IRQ_HANDLERS[32]` (kernel/src/irq.rs:8, kernel/src/arch/riscv/irq.rs:284-306).
Unclaimed IRQs are masked (kernel/src/irq.rs:43-49). No nesting: ISRs run with interrupts off,
re-enabled on return (kernel/src/arch/riscv/irq.rs:151-154).

**Vector table expectations**: none beyond `stvec` pointing at one handler; `mtvecInit=0x60000000`
only matters before firmware sets it. Zephyr will set `mtvec` to its own single trap vector and
poll SIM/SIP (M-mode) or run the same trick in S-mode.

The exact trigger/clear priority, edge-versus-level acknowledgment order, and
lost-event constraints are resolved in
[`06-irq-ack-semantics.md`](06-irq-ack-semantics.md). In particular, MIP/SIP is
a read-only masked view rather than an acknowledgment register; W1C occurs at
the owning irqarray or peripheral event manager.

---

## 5. Clock / power

From `libs/bao1x-hal/src/clocks.rs` (`init_clock_asic`, :83-243) and boot0/boot1:

* **Clock tree**: XTAL 48MHz (Dabao Y2, §6) → PLL (M/N/frac, Q0/Q1 post-dividers) → fclk →
  CPU = fclk/2; ratio fclk:aclk:hclk:iclk:pclk = 16:8:4:2:1 (:88-96; nominal 800:400:200:100:50,
  consts at :20-26). **perclk always targets 100MHz** regardless of fclk (PERCLK_HZ :20,
  SFR_CGUFDPER :118-122). Reference consts: XTAL0=48MHz, OSC=32MHz ring (:25-26).
* **SYSCTRL (CGU) regs used**: `SFR_CGUFD_CFGFDCR_0_4_0..4` (5 programmable dividers),
  `SFR_ACLKGR/HCLKGR/ICLKGR/PCLKGR` (clock gates), `SFR_CGUSET` (commit, writes 0x32),
  `SFR_CGUSEL0/1` (source select: 0=clksys/RC, 1=PLL0; SEL1=1 means XTAL :133-136),
  `SFR_CGUFSVLD`, `SFR_IPCPLLMN` (M/N), `SFR_IPCPLLF` (frac), `SFR_IPCPLLQ` (post-div),
  `SFR_IPCCR` (VCO bias), `SFR_IPCLPEN` (PLL power), `SFR_IPCARIPFLOW` (access key 0x57/0x32),
  `SFR_RCURST0` (**system reset = write 0x55AA**, boot1/…/bao1x.rs:137-138).
* **Sequence** (boot1 → loader → kernel all call init_clock_asic): set dividers → gate clocks
  (boot1: HCLKGR=0x02 sce-on, ICLKGR=0x90 bio/udc-on, PCLKGR=0x80 mesh-on, clocks.rs:125-128) →
  switch source to XTAL → scale LDO via AO_SYSCTRL `SFR_PMUTRM0CSR/1CSR` (0.72/0.81/0.893V by
  target freq, :139-170) → power up PLL (IPCLPEN dance, 0x57/0x32 keys) → program M/N/frac/Q →
  select PLL on CGUSEL0 → verify CGUFSVLD.
* **Boot0 leaves**: all clocks ungated with conservative dividers, DUART running off ring osc
  (boot0/…/bao1x.rs:80-107), SRAM0 1 wait state (sramtrm SFR_SRAM0=0x8, :109-110), CPU at
  reduced speed (200MHz, BOOTCHAIN.md:13). Boot1 then sets the board-specific target
  (Dabao 700MHz fclk, offsets/dabao.rs `DEFAULT_FCLK_FREQUENCY`; baosec.rs:42 same default;
  OEM safe-mode 350MHz, boot1/…/bao1x.rs:36,469-476).
* **Suspend/resume**: SUSRES block @0xE000_1000 (PAUSE/LOAD, RESUME_TIME, INT via SOFT_INT,
  utralib:451-492); Xous suspends by pausing ticktimer & suspres and powering down via AO PMU;
  wake via AO_SYSCTRL `CR_WKUPMASK`/`CR_RSTCRMASK` (+0x08/+0x0C, SVD) and AOWKUPINT event 47.
* **Autosleep**: no dedicated block found in open sources; power management is explicit
  (PMU regs SFR_PMUCSR/PMUCRLP/PMUCRPD/PMUSR etc. in AO_SYSCTRL, SVD listing §3.1). BIO
  cores each have the "quantum" halt (QDIV divider / GPIO event) as their local idle mechanism
  (ch01-00-bio-overview.md:14-16).
* **Voltage**: single core rail ~0.8-0.9V internal LDO (PmuControl bitfield, clocks.rs:29-41);
  Dabao additionally regulates VSYS→0.85V/3.3V externally (§6). SRAM trim must track voltage
  (rbist SFRCR_TRM writes, boot1/…/bao1x.rs:322-346).

---

## 6. Dabao board (v3c)

From `~/archive/baochip/dabao` (Kicad `dabao_v3c.kicad_sch`, docs/pinout/pins.csv, README.md) and
Xous board support (`libs/bao1x-hal/src/board/dabao.rs`, BOOTCHAIN.md:45-56):

* **Form**: minimal SoM-style breakout: Baochip U3 + 2× MT3406 bucks (U4: 0.85V core rail
  `FB_0.85V`; U6: 3.3V `FB_3.3V`) w/ 2.2µH, from VSYS; also an internal 2.5V rail from the chip.
  (kicad symbols U3 "Baochip", U4/U6 "MT3406", L1/L2, labels FB_0.85V/FB_3.3V/2.5V/VSYS/EN).
* **USB**: J1 = USB-C receptacle USB2.0 16P (TYPE-C-31-M-12) with 5.1k CC pulldowns R5/R11,
  EMS4000RSW ESD array (U5) on D+/D−. 48.000MHz crystal Y2 (X322548MSB4SI) w/ 33pF C0G load
  caps (XI_48M/XO_48M labels).
* **Buttons** (TS-1187A-B-B-B): SW1/SW2 → **RST_N** (chip RUN pin, physical pin 30 in pins.csv
  "RUN … RST_N") and **PC13** boot/PROG (BOOTCHAIN.md:45-46 "PC13 is dual-purposed as the boot
  update switch and USB disconnect switch"). PC13 is read active-low with pull-up
  (boot1/…/bao1x.rs:42-54,610-618); driven low as output it forces USB SE0
  (board/dabao.rs:35-50; boot1 SE0 pin PC13, bao1x.rs:39-40).
* **LEDs**: **none** on the board (only D1 = 1N5819WS power diode). Console + BIO are the UI.
* **Reserved boot pins** (do not repurpose in DT):
  * PC13 — PROG button / USB SE0 switch (physical pin 1, also BIO29/QSPI1CS1 alt)
  * PB14/PB13 — UART2 TX/RX serial console, 1Mbaud 8N1 (physical pins 15/16; setup in
    board/dabao.rs:67-89; baud bao1x-api lib.rs:46)
  * PF5 — USB SE0 on **Baosec** (BOOTCHAIN.md:47); not bonded out on Dabao header
* **Exposed header** (pins.csv, 40-pin, 2×20 like a pico): PC13/PC12/PC11/PC10/PC9/PC8/PC7
  (QSPI1: CS1/CS0/CLK/D3/D2/D1/D0 + SD CMD/CLK/D3-D0 alts), PC3/PC2/PC1/PC0 (SPI2 CS0/D1/D0/CLK,
  PWM2.3-2.0), PB14/PB13 (UART2), PB12/PB11 (I2C0 SDA/SCL + SPI2 CS1/CS0), PB1/PB2/PB3 (PWM1.1-3),
  PB4/PB5, RUN=RST_N, ON=EN, 3V3, VSYS, VBUS, GND ×6. PA4/PA5/PA6 (ADC0/1/2) are in the
  17-24 placeholder range (pins.csv README notes).
* Boot1 console runs over PB13/14 **or** USB-serial/USB-MSC (UF2); kernel+apps use USB
  (usb-bao1x service) (BOOTCHAIN.md:55-69).

---

## 7. Zephyr-relevant observations

* **CPU**: needs a new SoC family (rv32imac_zkn…). Zephyr `riscv` core with machine timer?
  **There is no CLINT/mtime** — `mtime`/`mtimecmp` don't exist. The RISC-V machine-timer driver
  cannot be used; sys_clock must be an **MMIO timer driver** on ticktimer (best: 64-bit
  free-run + comparator, IRQ 20, mmio regmap `zephyr,memory-region`-style binding needed) or
  timer0 (32-bit reload, IRQ 30). The ticktimer alarm is one-shot compare (EV_PENDING/EV_ENABLE
  LiteX event semantics, write-1-to-clear).
* **Interrupt controller**: custom, two-level. Zephyr needs a new INTC driver exposing the 32
  CPU lines with per-bank enable/pending, or flatten: IRQ numbers n=0..31 = CPU lines; per-bank
  events 16n..16n+16 map to line n (a natural Zephyr `interrupt-controller` w/ 320 "irqs", or
  model banks as nested INTCs). Masking via VexRiscv custom CSRs (M-mode: 0xBC0/0xFC0;
  S-mode: 0x9C0/0xDC0) — inverse-logic mask. No priority/nesting hardware.
* **UART**: **not 16550-compatible**. UDMA UARTs are descriptor-DMA engines pointing at IFRAM
  buffers with a `REG_UART_SETUP` line config; polling byte mode exists via REG_VALID/REG_DATA
  (command/poll mode bit 0x10 in SETUP). Simplest Zephyr console: write a small custom UART
  driver on UDMA_UART_2 polling `REG_VALID`, or use DUART (TX-only) for early console. The
  `ns16550` driver is useless here.
* **GPIO**: custom IOX — straightforward MMIO GPIO driver; 6 banks × 16 pins, no atomic
  set/clear aliases (read-modify-write per bank), pull-up only, per-pin drive strength/slew/
  schmitt, IRQ via INTCR/INTFR → PIOIRQ0-3 events (163-166). Zephyr `gpio_bao1x` style driver
  with pin-callback support routed through the INTC driver.
* **SPI/I2C**: only via **UDMA** (buffer-DMA to IFRAM + event interrupts + REG_SETUP line
  controls). A Zephyr `spi`/`i2c` driver would wrap UDMA_SPIM_n / UDMA_I2C_n with IFRAM
  bounce buffers; there is no pio byte-mode. Remember UDMA_CTRL clock gate + optional event
  channel routing at init, and 100MHz perclk default.
* **Flash**: RRAM is memory-mapped at 0x6000_0000 — Zephyr can XIP directly from it
  (`LITEX_CONFIG_BUS_STANDARD: AXI-LITE`, utralib:5565); no flash-controller driver needed for
  read; writes have 32B erase granularity (BOOTCHAIN.md:67). The bottom 0x60000 (boot0+boot1)
  is off-limits; a Zephyr image can live in the loader/baremetal slot @0x6006_0000 and be
  booted by the stock boot1 — the documented "baremetal" path (BOOTCHAIN.md:62-63,88-92).
* **Memory for DT**: sram0 @0x6100_0000/2MiB (linker evidence), ifram0/ifram1 as
  `zephyr,memory-region` DMA pools, aoram small always-on region, RRAM as flash-ish region.
  MMU `ioRange` (uncached nibbles 0x4/0x5/0xA-0xF) means Zephyr in M-mode with MMU off just
  sees normal memory at 0x60/0x61 and must not cache 0x40-0x5F (caches are physically indexed
  via translation; with satp=BARE the ioRange still applies per VexRiscv MmuPlugin config —
  verify in sim; Xous never runs with translation off).
* **Watchdog**: PL031-style CMSDK WDT — trivial Zephyr `wdog` driver (feed 0x5A, unlock
  0x1ACCE551, optional IRQ then reset).
* **Crypto/TRNG**: TRNG MMIO block maps to a Zephyr `entropy` driver candidate; AES/SHA/PKE and
  keyslots are very custom (descriptor SCE DMA, memory-mapped key segments) — likely out of
  scope for first bring-up; note Zkn AES instructions exist for sw crypto instead.
* **USB**: Corigine xHCI-device-style controller with rings in IFRAM — no Zephyr device class
  fits; defer or port services/usb-bao1x logic. High/full speed selectable by OWC
  (UsbDefaultSpeed, offsets/common.rs:123-129).
* **Weird vs standard, honestly**: DUART, ticktimer/timer0/mailbox/irqarrays are LiteX-CISR
  style (EV_PENDING RW1C) — pleasant. PL230/PL031/CMSDK-WDT are ARM PrimeCell standard.
  UDMA family, IOX, SCE, BIO, SDDC, security mesh/sensors are fully custom. There is **no
  standard RISC-V interrupt/timer substrate** (no PLIC/CLINT), so `CONFIG_RISCV_MACHINE_TIMER`
  and `CONFIG_PLIC` paths must be avoided; expect to add `soc/baochip/bao1x` with custom
  `soc_irq.h`/INTC + timer drivers plus a bespoke UART driver before anything else runs.

---

## Sources

* `~/archive/baochip/baochip-1x/README.md`
* `~/archive/baochip/baochip-1x/docs/src/ch00-00-introduction.md`
* `~/archive/baochip/baochip-1x/docs/src/ch00-00-rtl-overview.md`
* `~/archive/baochip/baochip-1x/docs/src/ch01-00-bio-overview.md`
* `~/archive/baochip/baochip-1x/VexRiscv/GenCramSoC.scala`, `VexRiscv_CramSoC.yaml`,
  `VexRiscv_CramSoC.v`, `memory_AesZknPlugin_rom_storage_Rom_1rs.v`
* `~/archive/baochip/baochip-1x/rtl/scripts/headergen/output/bao1x_peri.svd` (+ `bao1x_peri.h`,
  `bao1x_peri.rs`, `doc/*.rst`)
* `~/archive/baochip/baochip-1x/rtl/` (asic_top, modules/{bio_bdma,ifsub,sec,ao,crypto_*,…}, ips/udma)
* `~/archive/betrusted-io/xous-core/utralib/src/generated/bao1x.rs`
* `~/archive/betrusted-io/xous-core/bao1x-boot/BOOTCHAIN.md`
* `~/archive/betrusted-io/xous-core/bao1x-boot/boot0/link.x`, `boot0/src/platform/bao1x/bao1x.rs`
* `~/archive/betrusted-io/xous-core/bao1x-boot/boot1/src/platform/bao1x/{bao1x.rs,debug.rs,irq.rs}`
* `~/archive/betrusted-io/xous-core/bao1x-boot/boot1/src/platform/bao1x/link.x`
* `~/archive/betrusted-io/xous-core/loader/src/platform/bao1x/{bao1x.rs,link.x}`
* `~/archive/betrusted-io/xous-core/kernel/link.x`, `kernel/src/irq.rs`,
  `kernel/src/arch/riscv/{asm.S,irq.rs}`, `kernel/src/platform/bao1x/mod.rs`
* `~/archive/betrusted-io/xous-core/libs/bao1x-api/src/{lib.rs,offsets.rs,offsets/common.rs,offsets/dabao.rs,offsets/baosec.rs,clocks.rs}`
* `~/archive/betrusted-io/xous-core/libs/bao1x-hal/src/{clocks.rs,iox.rs,wdt.rs,rtc.rs,usb/utra.rs}`
* `~/archive/betrusted-io/xous-core/libs/bao1x-hal/src/board/dabao.rs`
* `~/archive/betrusted-io/xous-core/libs/bao1x-hal/src/udma/{mod.rs,uart.rs,adc.rs}`
* `~/archive/betrusted-io/xous-core/services/xous-ticktimer/src/platform/bao1x/implementation.rs`
* `~/archive/betrusted-io/xous-core/docs/memory.md`
* `~/archive/baochip/dabao/dabao_v3c.kicad_sch`, `~/archive/baochip/dabao/docs/pinout/{pins.csv,README.md}`
* `~/archive/betrusted-io/dabao-base-app/README.md`
