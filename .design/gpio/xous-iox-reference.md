---
type: Reference
title: Baochip 1x IOX pin-mux/GPIO block — hardware programming reference (from Xous + RTL)
description: Register-level reference for the IOX pin-mux/GPIO block of Baochip 1x, sufficient to write a Zephyr GPIO driver (baochip,iox-gpio) and the PC13 USB SE0 control for halbao-m5-usb-udc.
resource: hal_bao/.design/gpio/xous-iox-reference.md
tags: [gpio, iox, pinmux, baochip, usb, se0, zephyr]
status: draft
generated:
  by: opencode-subagent
  at: 2026-08-19
verified: { by: unverified, at: never }
sources:
  - id: xous-core
    resource: /home/rektide/archive/betrusted-io/xous-core
    title: betrusted-io/xous-core working copy (the only existing software for this hardware)
    author: betrusted-io
  - id: baochip-rtl
    resource: /home/rektide/archive/baochip/baochip-1x
    title: Baochip 1x RTL checkout (revision 83b220f7 per 06-irq-ack-semantics)
    author: baochip
---

# Baochip 1x IOX pin-mux / GPIO block — Xous + RTL hardware reference

Target: (a) Zephyr GPIO driver `baochip,iox-gpio` (M3 item consumed early by the M5 USB
ticket `halbao-m5-usb-udc`), (b) USB SE0 / boot-button control on PC13 for Dabao.

Sources: Xous (Rust) at `/home/rektide/archive/betrusted-io/xous-core` (cited relative to
that root), and RTL at `/home/rektide/archive/baochip/baochip-1x` (cited relative to that
root). Reading order for the Rust sources:

| Concern | File |
| --- | --- |
| HAL driver (all register programming) | `libs/bao1x-hal/src/iox.rs` |
| Enum types / trait contracts | `libs/bao1x-api/src/iox.rs` |
| Generated register map | `utralib/src/generated/bao1x.rs` (`pub mod iox`, lines 3082-3278) |
| Dabao board pins (SE0, console, I2C) | `libs/bao1x-hal/src/board/dabao.rs` |
| Baosec board pins | `libs/bao1x-hal/src/board/baosec.rs` |
| boot1 pin bring-up + PROG button | `bao1x-boot/boot1/src/platform/bao1x/bao1x.rs`, `bao1x-boot/boot1/src/main.rs` |
| Xous GPIO/IRQ service (userspace) | `services/bao1x-hal-service/src/main.rs` |
| Xous USB service SE0 release | `services/usb-bao1x/src/main.rs` |
| RTL for the block | `rtl/modules/ifsub/rtl/iox.sv` (+ `apb_sfr.sv` for CR/FR semantics) |
| SoC integration | `rtl/modules/ifsub/rtl/soc_ifsub.sv` |

---

## 1. Register map

Base `HW_IOX_BASE = 0x5012_F000` (`utralib/src/generated/bao1x.rs:3277`). APB slave 15 of
the peripheral bus (`soc_ifsub.sv:582-583`). All registers are **32-bit word accesses
only** (see §5). Offsets below are byte offsets; the utralib `Register::new(n, …)` index is
`n = offset/4`.

| Register(s) | Byte offset | Access | Width | Fields | Reset | Citation |
| --- | --- | --- | --- | --- | --- | --- |
| `SFR_AFSEL_CRAFSEL0..11` | 0x000-0x02C (stride 4) | RW | 16 | 8 × 2-bit AF select per word: bits `[2p+1:2p]` = pin `p` of the word's 8-pin group | 0 (= GPIO) | utralib:3085-3119; RTL `apb_cr` IV default (`apb_sfr.sv:81`) |
| `SFR_INTCR_CRINT0..7` | 0x100-0x11C (stride 4) | RW | 11 (utra mask `0x3ff` — SVD artifact, bit 10 exists in RTL) | `select[6:0]` = pin number 0-95 (port*16+pin); `mode[8:7]` = 00 rising, 01 falling, 10 high-level, 11 low-level; `enable[9]`; `wakeup[10]` | 0 (all IRQ slots off) | utralib:3121-3143; RTL `iox.sv:93-137` (`DW(IOCW+4)`, IOC=96 → 7+4 bits) |
| `SFR_INTFR` | 0x120 | R / **W1C** | 8 | sticky per-slot IRQ flags — **bit `7-i` = slot `i`** (reversed order, see §3) | 0 | utralib:3145-3146; RTL `iox.sv:138`, `apb_sfr.sv:340` |
| `SFR_GPIOOUT_CRGO0..5` | 0x130-0x144 (stride 4) | RW | 16 | output value, 1 bit per pin (bit p = port pin p) | 0 | utralib:3148-3164; RTL `iox.sv:156` |
| `SFR_GPIOOE_CRGOE0..5` | 0x148-0x15C | RW | 16 | output enable, 1 bit/pin (1 = output) | 0 | utralib:3166-3182; RTL `iox.sv:157` |
| `SFR_GPIOPU_CRGPU0..5` | 0x160-0x174 | RW | 16 | pull-up enable, 1 bit/pin — **pull-up only; no pull-down register exists** | **0xFFFF (all pull-ups ON)** | utralib:3184-3200; RTL `iox.sv:158` (`.IV(16'hffff)`) |
| `SFR_GPIOIN_SRGI0..5` | 0x178-0x18C | RO | 16 | synchronized pad input, 1 bit/pin; readable in any AF mode (tapped at the pad, `iox.sv:79-81,211-227`) | — | utralib:3202-3218; RTL `iox.sv:159` |
| `SFR_PIOSEL` | 0x200 | RW | 32 | pad→BIO mux. **Bit-reversed within the register** (`REVX(1)`, `iox.sv:259`): PB pin *p* → bit `15-p`; PC pin *p* → bit `16+p`. BIO bits 22, 27, 30, 31 (i.e. PC6/PC11/PC14/PC15) are not bondable to BIO | 0 (all pads to IOX) | utralib:3220-3221; `libs/bao1x-hal/src/iox.rs:277-330` |
| `SFR_CFG_SCHM_CR_CFG_SCHMSEL0..5` | 0x230-0x244 | RW | 16 | Schmitt trigger enable, 1 bit/pin | 0 (off) | utralib:3223-3239; RTL `iox.sv:235` |
| `SFR_CFG_SLEW_CR_CFG_SLEWSLOW0..5` | 0x248-0x25C | RW | 16 | slow-slew enable, 1 bit/pin (1 = slow) | 0 (fast) | utralib:3241-3257; RTL `iox.sv:236` |
| `SFR_CFG_DRVSEL_CR_CFG_DRVSEL0..5` | 0x260-0x274 | RW | 32 | drive strength, 2 bits/pin `[2p+1:2p]`: 00=2 mA, 01=4 mA, 10=8 mA, 11=12 mA | 0 (2 mA) | utralib:3259-3275; RTL `iox.sv:237` |

Unmapped holes (e.g. 0x2C-0xFC, 0x124-0x12C, 0x180-0x1FC, 0x204-0x22C, ≥0x278) read as 0
(`apbx.prdata = '0 | …`, `iox.sv:265-271`). `IOX_NUMREGS = 64` in utralib is stale; the
populated window is 0x000-0x278. The write-lock input `ioxlock` is tied to 0 in both SoC
tops (`rtl/asic_top/rtl/soc_top.sv:772`, `soc_top_no_cm7_rv.sv:788`), so SFR writes are
always permitted — no unlock/arming sequence exists.

## 2. Pin model

- **6 ports × 16 pins = 96 GPIOs** (`IoxPort::PA..PF = 0..5`,
  `libs/bao1x-api/src/iox.rs:13-20`; RTL `IOC = 16*6`, `soc_ifsub.sv:19,566-573`). Pin
  index for IRQ `select` and DT purposes is the flat `port*16 + pin` (0-95)
  (`services/bao1x-hal-service/src/main.rs:34`).
- **AFSEL encoding** (`libs/bao1x-api/src/iox.rs:25-30`): `0b00 = GPIO`, `0b01 = AF1`,
  `0b10 = AF2`, `0b11 = AF3`. Two 16-bit AFSEL words per port:
  word index `2*port + (pin ≥ 8 ? 1 : 0)`, bitpair `2*(pin % 8)`.
  Example **PC13**: port 2 → word `CRAFSEL5` (byte 0x14), bits `[11:10]`
  (`libs/bao1x-hal/src/iox.rs:120-134`).
- **Per-port 16-bit banks**: register byte offset = `bank_base + port*4`, bit `p` = pin p,
  for GPIOOUT (0x130), GPIOOE (0x148), GPIOPU (0x160), GPIOIN (0x178), SCHM (0x230), SLEW
  (0x248). This is exactly how the HAL's `set_pin_in_bank!` macro indexes
  (`libs/bao1x-hal/src/iox.rs:6-21`). DRVSEL is the 32-bit-per-port exception (0x260).
- **What "GPIO mode" means electrically**: the `iomtx` steers each pad between `afpad0`
  (the GPIO bank: value from GPIOOUT/GPIOOE/GPIOPU) and `afpad1..3` (peripherals) by the
  2-bit AFSEL, gated by a per-pin `afconnmask` bond map — if AFSEL selects an AF that is
  not bonded out for that pin, the pad **falls back to GPIO** (`iox.sv:305-327`). The AF
  function per pin/port is fixed at integration (see `soc_ifsub.sv:635+` `afconn`
  instances, e.g. PA3/PA4 = UART0 AF1, PC11 = SPIM1 AF1).
- Input path: pad → 2-flop pclk sync → `iopi` → GPIOIN; IRQ detection taps the same
  synchronized signal (3 further registers for edge detection, `iox.sv:103-123`). Edges
  on GPIOIN/IRQ therefore have a few-pclk latency.
- **BIO interaction**: PB0-15 and PC0-15 pads are shared with the BIO coprocessor block
  through `SFR_PIOSEL` (§1). A `1` in PIOSEL routes the pad to BIO instead of the IOX
  mux. Mapping used by Xous (`libs/bao1x-hal/src/iox.rs:211-245,277-330`): BIO bit 0-15 =
  **PB15..PB0 reversed**; BIO bit 16-31 = PC0..PC15 in order; BIO bits 22/27/30/31
  (PC6/PC11/PC14/PC15) not mappable. To give a pin to GPIO from BIO, clear its PIOSEL bit
  and set AFSEL (Xous sets AF1 + PIOSEL when claiming for BIO — the reverse claim is just
  a PIOSEL clear, it does not touch AFSEL).

## 3. Interrupt model

### 3.1 In-block IRQ: 8 multiplexed slots

The IOX has **8 interrupt slots**, each programmed through one `SFR_INTCR_CRINTi` word:

- `select[6:0]`: flat pin (port*16+pin, 0-95).
- `mode[8:7]`: `00` rising edge, `01` falling edge, `10` high level, `11` low level.
- `enable[9]`: slot armed.
- `wakeup[10]`: level-matched wakeup output (`wkupvld`) to the always-on domain —
  active when the pin sits at the post-edge level (`iox.sv:126-134`); unused by Xous'
  kernel but set by the HAL service (`services/bao1x-hal-service/src/main.rs:416`).

Xous encodes `IoxValue::Low` active → falling edge, `High` → rising edge
(`services/bao1x-hal-service/src/main.rs:411-414`, `IntMode` at 17-24, `IntCr` bitfield at
25-33). Level and both-edge modes exist in RTL but Xous only uses single edges.

`SFR_INTFR` holds one **sticky, write-1-to-clear** flag per slot
(`apb_sfr.sv:340`: write clears selected bits, hardware sets from the event). **Bit order
is reversed**: flag for slot *i* is register bit `7-i`. This falls out of the RTL's
ascending packed array (`bit [0:INTC-1] frint`, `iox.sv:95,135`) and is confirmed by
Xous' handler, which tests `(irq_flag << bitpos) & 0x80`
(`services/bao1x-hal-service/src/main.rs:486-509`, comment at 491: "the bit position is
flipped versus register order in memory"). The Xous init sequence zeroes all INTCR slots
and INTFR before enabling (`main.rs:233-242`).

The block's single `intvld` output is the OR of the *raw slot events* (`|ctl_intvld`,
`iox.sv:124,132`), i.e. a 1-pclk **pulse** for edge modes / a level for level modes — it
is **not** the sticky INTFR. Consumers therefore rely on the irqarray's pending latch to
catch the pulse, and on INTFR to learn which slot(s) fired.

### 3.2 CPU-side routing

- IOX `intvld` → SoC event **IOXIRQ = 160** → **irqarray10, bit 0** → CPU external
  line 10 (`soc_ifsub.sv:183-190`; `utralib:5543`, `irqarray10` fields at
  `utralib:685-752`, `IRQARRAY10_IRQ = 10` at 751; physical base 0xE000_6000).
- **Correction to earlier notes**: PIOIRQ0-3 (events 163-166, irqarray10 bits 3-6, also
  duplicated into irqarray0 bits 4-7 as `*_DUPE`) are the **BIO coprocessor block's** four
  machine IRQs (`.irq(pioirq)` on the `bio_bdma` instance, `soc_ifsub.sv:355`), *not*
  IOX INTFR lines. `.design/research/00-soc-inventory.md` §3.3/§4 and
  `.design/research/04-synthesis.md` M3 say "INTCR/INTFR → PIOIRQ0-3"; that is wrong —
  the IOX interrupt is IOXIRQ only. (Xous' PL230 test even routes PIOIRQ0 into MDMA as a
  GPIO-adjacent BIO event, `libs/xous-pl230/src/pl230_tests/units.rs:253`.)
- Zephyr numbering (`zephyr-baochip` `include/zephyr/dt-bindings/interrupt-controller/baochip-bao1x-intc.h:7-8`):
  `BAO1X_IRQ_EVENT(bank, bit) = 16 + bank*16 + bit`, so
  **IOXIRQ = BAO1X_IRQ_EVENT(10, 0) = 176**;
  (for reference: USBC = 177, SDDCIRQ = 178, PIOIRQ0-3 = 179-182 — but those four are BIO's).
- The Zephyr INTC keeps irqarray10 in **level mode** (`dts/riscv/baochip/bao1x.dtsi:63-67`,
  bank-10 mask 0x0000), which correctly latches the IOX 1-cycle pulse; dispatch order is
  ISR-then-W1C for level events (`drivers/interrupt_controller/intc_baochip_bao1x.c:127-150`).
- **Ack protocol for the GPIO driver ISR**: read INTFR, W1C exactly that snapshot (this is
  the source clear), dispatch callbacks for set bits (bit 7-i → slot i), and let the INTC
  post-ack the bank pending. Clearing the source first is required for level sources and
  safe for the pulse source. Xous does bank-pending W1C in the first-stage handler
  (`main.rs:52-67`) and INTFR W1C in the second stage (`main.rs:486-488`); for a pulse
  source either order works.
- Shared-bank concern: IOXIRQ shares irqarray10 with USBC/SDDCIRQ/PIOIRQ0-3; the INTC
  driver already drains the whole bank, so the GPIO driver only owns event bit 0 and must
  not touch other bits (and must not assume it caused the interrupt — check INTFR != 0).
- Slot scarcity: only 8 slots; two pins claimed with the same select value would alias —
  the driver must track select values per slot and refuse duplicates (Xous tracks an
  `[Option; 8]` table, `main.rs:251-256`, and panics when exhausted, `main.rs:440`).

## 4. PC13/SE0 and reserved pins on Dabao (and the Baosec mirror)

Background (`README-baochip.md:27`): the Corigine USB PHY cannot force SE0 by itself; an
external USB switch (EMS4000-type) on each board does it, controlled by a GPIO. On Dabao
that GPIO is **PC13**; on Baosec it is **PF5** (`libs/bao1x-hal/src/board/dabao.rs:35-36`,
`libs/bao1x-hal/src/board/baosec.rs:311-313`). On Baosec, PC13 is instead `SPIM_CSN1_A[1]`
flash chip-select (`baosec.rs:171-178`) — a second reason PC13 is board-reserved.

The Xous register sequences, exactly:

**Force SE0 / disconnect (Dabao PC13)** — `setup_dabao_se0_pin`
(`bao1x-boot/boot1/src/platform/bao1x/bao1x.rs:56-68`, same as
`setup_usb_pins` in `board/dabao.rs:38-50`), then drive low:

1. `AFSEL`: CRAFSEL5[11:10] = `0b00` (Gpio)
2. `GPIOOE`: PC bit 13 = 1 (Output)
3. `SFR_GPIOPU`: PC bit 13 = 1 (pull-up left ON — "dabao switch happens by tri-state",
   `dabao.rs:45`)
4. `SFR_CFG_SLEW`: PC bit 13 = 1 (slow slew)
5. `SFR_CFG_DRVSEL`: PC pair [27:26] = `0b00` (2 mA)
6. `GPIOOUT`: PC bit 13 = 0 → pin drives low → USB switch opens (SE0)

**Release / reconnect** — either drive high (`GPIOOUT` bit 13 = 1;
`boot1/src/main.rs:246-247`, `baremetal/…/usb/glue.rs:71-72`) or tri-state to input:
`setup_dabao_boot_pin` / `setup_boot_pin` sets `AFSEL=Gpio`, `GPIOOE` bit13 = 0 (Input),
Schmitt ON, pull-up ON (`dabao.rs:53-65`, `bao1x.rs:42-54`); the pull-up holds the switch
closed. The Xous OS stage releases by direction change only:
`services/usb-bao1x/src/main.rs:276-278` (`setup_usb_pins` then `set dir = Input`,
with the note that on Baosec the KPC AF must be released for PF5 to be drivable).

**PROG button read (same pin, multiplexed in time)** — boot1 configures PC13 as
input+schmitt+pullup and samples `GPIOIN` PC bit 13; **0 = pressed** → stay in
bootloader (`bao1x.rs:610-619`, early-init at 322/336). Electrical model: button to
GND on the board, internal pull-up, Schmitt trigger conditioned; active-low.

**Boot sequencing** (why the pin flaps): boot1 asserts SE0 on *both* PC13 and PF5 before
the board type is known, waits ≥250 ms (display init; `boot1/src/main.rs:218-237`),
inits USB, releases both high, returns the off-target pin to input
(`main.rs:245-256`), and on Dabao puts PC13 back into boot-button mode
(`setup_dabao_boot_pin`). Before jumping to the next stage it re-asserts SE0
(output + low, `main.rs:397-401`); the baremetal/Xous stage then releases
(`baremetal/src/platform/bao1x/bao1x.rs:116-126`: Baosec drives high; Dabao sets up as
output then tri-states). Reboot hot-plug: low → 500 ms → init PHY → 500 ms → high
(`baremetal/…/usb/glue.rs:64-73`). A USB re-enumeration should be visible to the host as
a disconnect+connect, hence the ≥250 ms low.

**Reserved-pin contract on Dabao**: PC13 (USB SE0 + PROG button + BIO29), PB13/PB14
(console UART2 RX/TX, AF1, schmitt+pullup on RX / 4 mA slow-slew TX,
`dabao.rs:67-89`), PB11/PB12 (I2C0 AF1, `dabao.rs:91-115`). Generic GPIO clients must
not reconfigure these. Note **BIO PIOSEL bit 29 claim**: PC13 is BIO29; if any BIO
program claims BIO29, PIOSEL bit 29 routes the pad away from IOX and the SE0 GPIO writes
do nothing (§2). PIOSEL resets to 0, and the Zephyr GPIO driver should never write
PIOSEL — but the UDC bring-up should sanity-check `PIOSEL[29] == 0`.

## 5. Quirks that affect the driver

1. **Word-only access**: registers are captured from full 32-bit APB writes
   (`apb_sfr.sv:339` — no byte strobes). Use `sys_read32`/`sys_write32` exclusively.
   16-bit registers simply ignore `pwdata[31:16]`.
2. **No atomic set/clear aliases**: GPIOOUT/OE/PU/SCHM/SLEW/AFSEL/DRVSEL/PIOSEL are all
   plain RW words — every pin update is a read-modify-write of a 16-bit bank shared with
   15 other pins. Xous serializes this by funnelling all GPIO through the single-threaded
   `bao1x-hal-service` message loop (`services/bao1x-hal-service/src/main.rs:358-383`);
   its `SharedCsr` is an unsynchronized raw-pointer wrapper — no locks anywhere
   (`libs/bao1x-hal/src/shared_csr.rs:10`). The Zephyr driver must take a spinlock
   (or `irq_lock()`) around every bank RMW, including in the ISR path.
3. **Pull-up only** (no pull-down anywhere; the pad interface only carries `.pu`,
   `iox.sv:314-318`). `GPIO_PULL_DOWN` must fail with `-ENOTSUP`. There is also no
   open-drain/open-source control (`iocfg` has only schmitt/slew/drive,
   `iox.sv:243-249`).
4. **GPIOPU resets to 0xFFFF** — every pin comes up with its pull-up enabled
   (`iox.sv:158`). An input config that leaves PULL_UP unspecified still has the pull-up
   on unless the driver explicitly clears it. (Xous' `setup_pin` semantics: `None` =
   "don't touch".)
5. **INTFR bit reversal** (slot i ↔ bit 7-i) and its **W1C** semantics; never RMW
   EV_PENDING-style registers — write back exactly the snapshot of set bits
   (Xous: `wo(SFR_INTFR, irq_flag)` after a read, `main.rs:486-488`).
6. **intvld is a pulse for edge modes** (1 pclk) — the irqarray event must stay in level
   mode (it is, per the Zephyr DTSI) and the driver must read INTFR, not infer the source
   from the bank pending bit.
7. **AFSEL fallback**: programming an AF that isn't bonded (`afconnmask`) silently gives
   you GPIO (`iox.sv:305-327`). The driver should treat AF programming (pinctrl, later)
   as best-effort and keep the AF map in DT.
8. **Direction-before-value race**: Xous sets OE (Output) before writing GPIOOUT, so the
   pad momentarily drives its *previous* GPIOOUT value. For glitch-sensitive pins
   (SE0!) write GPIOOUT first, then OE — the recommended Zephyr `pin_configure` order.
9. **BIO aliasing**: PB0-15/PC0-15 pads have a second master (BIO via PIOSEL). The GPIO
   driver must not touch PIOSEL; pin ownership vs BIO is a system-level policy.
10. **Input latency / schmitt**: GPIOIN and IRQ detection are pclk-synchronized
    (2-5 flops); enable the Schmitt trigger on any pin used as an input (Xous does
    consistently — e.g. button, I2C SDA, keyboard columns) to reject chatter.
11. **irqarray10 is shared** (USBC, SDDCIRQ, PIOIRQ0-3 siblings); only event bit 0
    belongs to IOX (§3.2).

## 6. Zephyr driver recommendation

### DT binding (`baochip,iox-gpio`)

```dts
iox: gpio@5012f000 {
        compatible = "baochip,iox-gpio";
        reg = <0x5012f000 0x278>;
        interrupts = <BAO1X_IRQ_EVENT(10, 0)>;   /* IOXIRQ */
        interrupt-parent = <&intc>;
        gpio-controller;
        #gpio-cells = <2>;
        ngpios = <96>;
        status = "okay";
};
```

- **2-cell flat encoding: `<pin flags>` with `pin = port*16 + pin` (0-95)**, matching
  `ngpios = <96>` — idiomatic Zephyr (STM32-style flat numbering) and works with every
  stock macro (`GPIO_DT_SPEC_GET`, gpio-hogs, `gpio_dt` helpers). A 3-cell
  `<port pin flags>` or bank-style specifier would break `struct gpio_dt_spec` assumptions
  for no real gain; keep port/pin readable via `DT_ALIAS`/comments (e.g.
  `/* PC13 = 45 */`). PC13 = 45, PB13 = 29, PB14 = 30.
- Flags: support `GPIO_INPUT/OUTPUT/OUTPUT_{HIGH,LOW}`, `GPIO_PULL_UP`,
  `GPIO_ACTIVE_*`; reject `GPIO_PULL_DOWN`, `GPIO_OPEN_DRAIN/SOURCE`,
  `GPIO_SINGLE_ENDED` with `-ENOTSUP`. Schmitt: enable automatically for inputs
  (Xous precedent). Drive strength / slew: no standard flags — default 2 mA + fast slew,
  overridable later via pinctrl or vendor cells; do not block M5 on this.
- Board DTS marks **reserved pins** with `reserved-gpios = <&iox 45 0>, <&iox 29 0>,
  <&iox 30 0>;` (PC13, PB13, PB14) — the GPIO API then rejects `gpio_pin_configure` on
  them, which protects console + SE0 from generic clients. The USB UDC driver gets PC13
  by having the board DTS *not* reserve it for itself... cleaner: reserve it in the
  generic case and let the UDC node own a `se0-gpios` phandle whose pin is deliberately
  excluded from `reserved-gpios` only in the USB-enabled board variant. Avoid gpio-hogs
  for PC13 (the UDC needs dynamic ownership), but a hog that holds the console pins in
  their AF configuration is unnecessary (pin mux is static from pin_configure).
- `pinmux` ownership: peripheral AF selection (e.g. PB13/14 AF1 for UART2) is done by
  each peripheral's pinctrl later (M3+); for M5 the GPIO driver only needs AFSEL=0 GPIO
  writes.

### API mapping

| Zephyr API | Implementation |
| --- | --- |
| `pin_configure` | flags → RMW AFSEL word (bitpair → 0), OE, PU, OUT, SCHM, (SLEW/DRVSEL defaults); **write GPIOOUT before OE** (§5.8); all RMW under spinlock |
| `port_get_raw` / `pin_get` | `sys_read32(base + 0x178 + port*4)` |
| `port_set_bits_raw` / `port_clear_bits_raw` / `port_toggle_bits` | masked RMW of GPIOOUT bank (Xous `set_gpio_bank` pattern, `iox.rs:43-53`) — spinlock |
| `pin_interrupt_configure` | allocate one of 8 INTCR slots (bitmap; also track the `select` value to refuse duplicate-pin claims); write `select | mode<<7 | 1<<9` (`| 1<<10` only when PM wakeup arrives); map `GPIO_INT_EDGE_RISING→00`, `EDGE_FALLING→01`, `LEVEL_HIGH→10`, `LEVEL_LOW→11`; both-edge → two slots or `-ENOTSUP` initially |
| `manage_callback` + ISR | `IRQ_CONNECT(DT_INST_IRQN(0), …)`; ISR: snapshot INTFR, W1C snapshot, for each set bit `7-i` → slot i → dispatch callback with edges re-derived from stored mode; return; INTC post-acks the bank (level policy). Check INTFR≠0 before claiming. |
| `port_set_direction` etc. | OE RMW as above |

Deferred (M3 remainder / later): pinmux-pinctrl support for AF1-3, drive-strength/slew
DT plumbing, `wakeup` bit programming (PM), both-edge interrupts via slot pairs,
keyboard-matrix helper, BIO coexistence policy, debounce (software-only). The M5 USB
slice needs only: pin_configure, pin_set/clear, port get/set, pin_interrupt_configure
(optional — the UDC ticket itself only needs SE0 GPIO + the Corigine IRQ).

## 7. Test plan (native_sim / build-only)

1. **native_sim unit test** (`tests/drivers/gpio/baochip_iox`): overlay on `native_sim`
   adding the `iox` node at a RAM-backed MMIO window (native_sim maps MMIO into host
   memory). The driver talks to plain RAM, so register effects are directly assertable:
   - `gpio_pin_configure_raw(spec PC13=45, OUTPUT|LOW)` →
     `AFSEL5[11:10]==0`, `OE.PC & BIT(13)`, `!(OUT.PC & BIT(13))`, `PU.PC & BIT(13)`,
     and OUT was written *before* OE (order check via manual sequence or code review).
   - `gpio_pin_toggle` → OUT bit flips; `port_set_bits_raw(0x00FF)` → only masked bits
     change (RMW correctness).
   - `PULL_DOWN` → `-ENOTSUP`; reserved pin → configure fails.
   - IRQ: `pin_interrupt_configure(PB15=31, EDGE_RISING)` → slot word ==
     `31 | 0<<7 | 1<<9`; simulate the block by RAM-writing `INTFR = BIT(7)` (slot 0),
     invoke the ISR function directly, assert callback fired and INTFR cleared;
     reversed-order check with slot 3 → `BIT(4)`.
   - Slot exhaustion: register 8 pins, 9th fails; duplicate pin select rejected.
2. **Build-only**: `twister -p native_sim -T tests/drivers/gpio/...` plus
   `west build -b dabao samples/hello_world` (or `basic/blinky` against a header-pad
   GPIO) once the board exists — compile coverage for the real DT.
3. **On-target smoke (when HWMv2 arrives)**: PC13 sequence from §4 via a test shell
   command: drive low ≥250 ms, release to input, verify `GPIOIN` reads 1 (pull-up) and
   the PROG button pulls it to 0; `dmesg`-style check that USB re-enumerates.
4. **IRQ integration**: reuse `tests/soc/baochip/irqarray` patterns
   (`zephyr-baochip/tests/soc/baochip/irqarray/`) to co-schedule IOXIRQ with a sibling
   bank-10 event and verify no cross-ack.

## Cross-references

- [`/.design/usb/xous-hw-reference.md`](/.design/usb/xous-hw-reference.md) —
  [`/.design/usb/xous-hw-reference.md`](/.design/usb/xous-hw-reference.md): USB UDC
  hardware reference for `halbao-m5-udc`; its boot-flow sections cite the same SE0
  sequences from the UDC side.
- [`/.design/research/00-soc-inventory.md`](/.design/research/00-soc-inventory.md) — SoC
  map including IOX §3.3 and event routing §4; **this note corrects its claim that INTFR
  feeds PIOIRQ0-3** (§3.2 above).
- [`/.design/research/06-irq-ack-semantics.md`](/.design/research/06-irq-ack-semantics.md)
  — irqarray edge/level/ack contract the GPIO driver's ISR ordering relies on.
- [`/.design/research/04-synthesis.md`](/.design/research/04-synthesis.md) — M3 GPIO
  driver scope and M5 USB deferral context.
