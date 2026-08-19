---
type: Reference
title: Baochip 1x Corigine USB device controller — hardware programming reference (from Xous)
description: Complete register-level reference for the Corigine xHCI-style UDC on Baochip 1x, reverse-engineered from the Xous Rust driver, sufficient to write a Zephyr udc driver in C.
resource: hal_bao/.design/usb/xous-hw-reference.md
tags: [usb, udc, baochip, corigine, zephyr]
status: draft
generated:
  by: opencode-subagent
  at: 2026-08-19
verified: { by: unverified, at: never }
sources:
  - id: xous-core
    resource: /home/rektide/archive/betrusted-io/xous-core
    title: betrusted-io/xous-core working copy d5489ae306a2 (reference commit 5397e1b48 for boot1 glue; USB files differ only in speed-parameter plumbing between the two)
    author: betrusted-io
---

# Baochip 1x "Corigine" USB Device Controller — Xous hardware reference

Target: Zephyr UDC driver for ticket `halbao-m5-usb-udc`. Everything below was extracted
from the Xous (Rust) implementation, which is the only existing software for this hardware.
All file:line citations are relative to the xous-core repo root
(`/home/rektide/archive/betrusted-io/xous-core`, working copy `d5489ae306a2`). The pinned
reference commit for boot1's USB glue is `5397e1b488c081566cef2c0e597e05426f67c1c3`; the
USB-relevant files differ between that commit and the working copy only in cosmetic
speed-parameter plumbing (`usb.init()` → `usb.init(speed)`, verified by
`git diff 5397e1b48 d5489ae -- .../usb/`), so working-copy line numbers are cited throughout.

Reading order for the Rust sources:

| Concern | File |
| --- | --- |
| Register map (utra, generated) | `libs/bao1x-hal/src/usb/utra.rs` |
| Controller driver (TRBs, rings, commands, events) | `libs/bao1x-hal/src/usb/driver.rs` |
| no-std CSR accessor (volatile + fences) | `libs/bao1x-hal/src/usb/compat.rs` |
| boot1 USB glue (init/shutdown/SE0) | `bao1x-boot/boot1/src/platform/bao1x/usb/glue.rs`, `.../driver.rs`, `.../handlers.rs` |
| boot1 boot flow (SE0 timing) | `bao1x-boot/boot1/src/main.rs` |
| boot1 IRQ trampoline | `bao1x-boot/boot1/src/platform/bao1x/irq.rs` |
| baremetal (kernel-loader) USB | `baremetal/src/platform/bao1x/usb/{glue,driver,handlers,mod}.rs` |
| Xous OS service (userspace driver + CDC/HID) | `services/usb-bao1x/src/{hw,main}.rs` |
| Board pin/IFRAM maps | `libs/bao1x-hal/src/board/{dabao,baosec}.rs` |
| IOX (pin mux) driver | `libs/bao1x-hal/src/iox.rs` |
| Generated SoC map (IFRAM/IOX/irqarray bases) | `utralib/src/generated/bao1x.rs` |

---

## 1. MMIO base addresses and register map

### 1.1 Bases

| Block | Base | Size | Citation |
| --- | --- | --- | --- |
| Corigine UDC CSR | `0x5020_2000` | 0x3000 | `libs/bao1x-hal/src/usb/utra.rs:197,200` |
| — device register window | `+0x400` | — | `utra.rs:198` (`CORIGINE_DEV_OFFSET`) |
| — interrupter ("UICR") window | `+0x500` | 0x20 | `utra.rs:199` (`CORIGINE_UICR_OFFSET`) |
| irqarray1 (USB IRQ line) | `0xE000_5000` | 0x1000 | `utralib/src/generated/bao1x.rs:682` |
| IOX (pin mux) | `0x5012_F000` | — | `utralib/src/generated/bao1x.rs:3277` |
| IFRAM0 (DMA RAM) | `0x5000_0000` | 128 KiB | `utralib/src/generated/bao1x.rs:303-304` |
| IFRAM1 (DMA RAM) | `0x5002_0000` | 128 KiB | `utralib/src/generated/bao1x.rs:305-306` |

The xous kernel maps the CSR window and irqarray with plain R|W (`services/usb-bao1x/src/main.rs:93-99,116-122`).
There is **no clock-gating register anywhere in the USB path** — the block is always clocked; init begins
directly with register writes (verified: no `sysctrl`/clock reference in `libs/bao1x-hal/src/usb/`).

Offsets below are **byte offsets from `0x5020_2000`**. (utra.rs encodes them as 32-bit word
offsets after `CORIGINE_DEV_OFFSET/4`; the conversion used in the driver is visible at
`libs/bao1x-hal/src/usb/driver.rs:1249` and `compat.rs:20-71`.) "RC" = read-clear-on-write-back
(ack by writing the read value back), "W1C" = write-1-to-clear, "strobe" = write triggers action.

### 1.2 Device region (offset 0x400+)

| Register | Off | Access | Bits / meaning | Citation |
| --- | --- | --- | --- | --- |
| DEVCAP | 0x400 | RO | VESION[7:0]; EP_IN[11:8]; EP_OUT[15:12]; MAX_INTS[25:16]; GEN1[27]; GEN2[28]; ISOCH[29]. Observed value `0x20014401` → version 1, **4 EP-IN + 4 EP-OUT (plus EP0), 1 interrupter, ISOCH cap, no Gen1/2** | utra.rs:3-10; log comment driver.rs:1199-1204 |
| DEVCONFIG | 0x410 | W (mask 0xFF) | MAX_SPEED[3:0]: 0=LS, 1=FS, 3=HS; USB3_DISABLE_COUNT[7:4]. Xous writes `0x80|speed` → LS `0x80`, FS `0x81`, HS `0x83` (i.e. usb3_disable_count=8 always). Reset value observed: max_speed=1 (FS), usb3_disable=8 | utra.rs:12-14; driver.rs:336-337,1287-1292; log driver.rs:1196-1197 |
| EVENTCONFIG | 0x414 | RW | Event-type enables: CSC[0], PEC[1], PPC[3], PRC[4], PLC[5], CEC[6], U3_PLC[8], L1_PLC[9], U3_RESUME_PLC[10], L1_RESUME_PLC[11], INACTIVE_PLC[12], USB3_RESUME_NO_PLC[13], USB2_RESUME_NO_PLC[14], **SETUP[16]**, STOPPED_LEN_INVALID[17], HALTED_LEN_INVALID[18], DISABLED_LEN_INVALID[19], DISABLE_EVENT[20] | utra.rs:16-34 |
| USBCMD | 0x420 | RW | RUN_STOP[0]; **SOFT_RESET[1] (self-clearing — poll until 0)**; INT_ENABLE[2]; SYS_ERR_ENABLE[3]; EWE[10]; FORCE_TERMINATION[11] | utra.rs:36-42 |
| USBSTS | 0x424 | mixed | CTL_HALTED[0] (RO); **SYSTEM_ERR[2] W1C**; **EINT[3] W1C**; CTL_IDLE[12] (RO) | utra.rs:44-48 |
| DCBAPLO | 0x428 | W | device/EP-context base pointer low [31:6] → **64-byte aligned** | utra.rs:50-51 |
| DCBAPHI | 0x42C | W | base pointer high (xous writes 0) | utra.rs:53-54 |
| PORTSC | 0x430 | RW/RC | CCS[0] RO; PP[3]; PR[4] (self-clearing reset); PLS[8:5]; SPEED[13:10] (1=FS, 2=LS, 3=HS, 4=SS, 5..7=SSP); LWS[16]; **CSC[17], PPC[20], PRC[21], PLC[22], CEC[23] are change flags — ack by writing the whole read value back**; WCE[25]; WDE[26]; WPR[31] | utra.rs:56-70; bitfield struct driver.rs:239-257 |
| U3PORTPMSC | 0x434 | W | write 0 = disable U1/U2 LPM | utra.rs:72; driver.rs:1387 |
| U2PORTPMSC | 0x438 | W | write 0 = disable USB2 LPM | utra.rs:74; driver.rs:1390 |
| DOORBELL | 0x440 | **W strobe** | TARGET[4:0] = physical endpoint index (PEI). Writing kicks the endpoint's transfer ring | utra.rs:78-79; driver.rs:1712-1717 |
| MFINDEX | 0x444 | — | SYNC_EN[0], OUT_OF_SYNC_EN[1], IN_SYNC_EN[2], INDEX_OUT_OF_SYNC_EN[3], MFINDEX_EN[17:4], MFOFFSET_EN[30:18] (defined, unused by xous) | utra.rs:81-87 |
| PTMCTRL / PTMSTS | 0x448 / 0x44C | — | PTM_DELAY[13:0] / status (unused) | utra.rs:89-95 |
| EPENABLE | 0x460 | RW (bit per PEI at bit *N*, N≥2) | read: endpoint enabled mask; **write `1<<pei` to clear the enable** (used by `ep_disable`) | utra.rs:97-98; driver.rs:2049-2059,2084 |
| EPRUNNING | 0x464 | RO-ish | bit per PEI at bit *N*; poll `& (1<<pei)` to wait for stop | utra.rs:100-101; driver.rs:2069-2075 |
| CMDPARA0 | 0x470 | W | command parameter 0 (per-command layout, see §2.4) | utra.rs:103-106 |
| CMDPARA1 | 0x474 | W | command parameter 1 (always 0) | utra.rs:108 |
| CMDCTRL | 0x478 | RW **strobe** | write `ACTIVE[0]=1 | TYPE[7:4]` to launch a command; **poll ACTIVE[0]==0** for completion; STATUS[19:16] != 0 → failure | utra.rs:110-114; driver.rs:1506-1535 |
| ODBCAP | 0x480 | RO | outbound-data-buffer RAM size[10:0] (defined; unused by xous) | utra.rs:116-117 |
| ODBCONFIG0..7 | 0x490..0x4AC | W | per-EP ODB offset/size fields (defined; unused by xous) | utra.rs:119-165 |
| DEBUG0 | 0x4B0 | RO | DEV_ADDR[6:0], NUMP_LIMIT[11:8] (defined; unused) | utra.rs:167-169 |

### 1.3 Interrupter region ("UICR", offset 0x500+)

Layout confirmed by the `Uicr` struct at `driver.rs:324-334` (iman, imod, erstsz, **rsvd**,
erstbalo, erstbahi, erdplo, erdphi).

| Register | Off | Access | Bits | Citation |
| --- | --- | --- | --- | --- |
| IMAN | 0x500 | RW/W1C | **IP[0] — write 1 to clear pending**; IE[1] interrupt enable | utra.rs:171-173 |
| IMOD | 0x504 | RW | MOD_INTERVAL[15:0]; MOD_COUNTER[31:16] (utra declares width 16 @ 32 — bogus; xous writes 0) | utra.rs:175-177; driver.rs:1370 |
| ERSTSZ | 0x508 | W | RING_SEG_TABLE[15:0] = number of ERST entries (xous: 1) | utra.rs:179-180 |
| (reserved) | 0x50C | — | gap — not ERSTBALO! | driver.rs:324-334 |
| ERSTBALO | 0x510 | W | ERST base pointer low [31:6] → **64-byte aligned** | utra.rs:182-183 |
| ERSTBAHI | 0x514 | W | ERST base high (0) | utra.rs:185-186 |
| ERDPLO | 0x518 | RW | DESI[2:0]; **EHB[3] — "event handler busy", must be set when writing the dequeue pointer**; DQ_PTR[31:4] → 16-byte aligned event dequeue pointer | utra.rs:188-191 |
| ERDPHI | 0x51C | W | DQ pointer high (0) | utra.rs:193-194 |

### 1.4 Undocumented "magic" registers (PHY/config, below 0x400)

`CorigineUsb::reset()` writes a fixed table of raw values before the soft reset
(`driver.rs:1207-1250`). Byte offsets from USB base, order matters (values as written):

```
0x0FC = 0x00000001     0x0F4 = 0x0000f023     0x110 = 0x00000000
0x084 = 0x01401388     0x088 = 0x3b066409     0x08C = 0x0d020407
0x090 = 0x04055050     0x094 = 0x03030a07     0x098 = 0x05131304
0x09C = 0x3b4b0d15     0x0A0 = 0x14168c6e     0x0A4 = 0x18060408
0x0A8 = 0x4b120c0f     0x0AC = 0x03190d05     0x0B0 = 0x08080d09
0x0B4 = 0x20060b03     0x0B8 = 0x040a8c0e     0x0BC = 0x44087d5a
```

An alternate table exists behind the `magic-manual` feature (`driver.rs:1227-1246`), differing
in the 0x088–0x0BC values. These are USB2-PHY analog tuning registers of unknown semantics —
**a C driver must reproduce the default table verbatim.** Additionally `driver.rs:2264` notes
"PHY control, 0x1C bit 1 set to 1 causes the device to hi-Z" (soft-disconnect per manual
§8.3.4) — xous never uses it, preferring the PC13/SE0 GPIO route (§6).

---

## 2. Controller architecture

### 2.1 Shape: xHCI-deviced (DBS = doorbell + command + event ring)

It is an xHCI-*style* device controller ("Corigine" UDC), not a DWC3: a single command
register (CMDCTRL) with parameter registers instead of a command ring, one doorbell register,
one interrupter with ERST/ERDP event ring, device-context (EP context array) pointer (DCBAP),
and TRB-based transfer rings per endpoint. Constants: 1 event ring, 1 ERST entry, 32 event
TRBs, EP0 ring 16 TRBs, per-EP rings 64 TRBs (`driver.rs:38-43`).

### 2.2 Endpoint addressing — PEI

* PEI (physical endpoint index) = `2*ep_num + dir_bit` where `dir_bit = 1` for USB OUT
  (host→device), `0` for USB IN (`driver.rs:1069`: `pei(ep,dir) = 2*ep + (dir?1:0)`,
  with `USB_RECV/CRG_OUT = true`).
* **Even PEI = USB IN (device→host); odd PEI = USB OUT** (`driver.rs:31-36`).
* EP0 is PEI 0 (nominally "IN"); EP0 OUT also uses doorbell 0 (`ep0_receive` →
  `knock_doorbell(0)`, `driver.rs:1864-1900`). PEI 1 events are ignored by the driver.
* Non-EP0 endpoints: hardware has **4 IN + 4 OUT** pairs (DEVCAP) → usable PEIs 2..9,
  i.e. EP1..EP4 (`driver.rs:1199-1204`). Software arrays are sized CRG_EP_NUM=8 but the
  transfer-ring allocation only has room for 8 rings (pei−2 ∈ 0..7) — `ep_enable`
  ring math `driver.rs:1990` and bound assert `driver.rs:1999-2002`.
* Endpoint context array is indexed **pei−2** (`driver.rs:2032`), 16 bytes per context,
  0x200 bytes reserved (`driver.rs:52-53`).
* Beware naming inversion in the vendor enum: Corigine "outbound" = device→host = **USB IN**;
  "inbound" = USB OUT (`driver.rs:27-36`). EpType values: `IsochOutbound=1, BulkOutbound=2,
  IntrOutbound=3, Invalid2=4, IsochInbound=5, BulkInbound=6, IntrInbound=7`
  (`driver.rs:196-205`); `epcx_setup` converts USB-OUT endpoints to `base + 4` ("inbound")
  (`driver.rs:634-642`).

### 2.3 TRB formats (16 bytes each, little-endian dwords)

**Transfer TRB** (`TransferTrbS`, `driver.rs:457-600`):

| dword | fields |
| --- | --- |
| dw0 | data buffer physical address (32-bit, IFRAM). For Link TRB: jump target TRB addr `[31:4]` |
| dw1 | address high — always 0 |
| dw2 | TRANSFER_LEN[16:0]; TD_SIZE[21:18]; INTR_TARGET[31:22] (xous always 0) |
| dw3 | CYCLE[0]; LINK_TOGGLE_CYCLE[1]; INTR_ON_SHORT_PKT[2]; NO_SNOOP[3]; TRB_CHAIN[4]; INTR_ON_COMPLETION[5]; **APPEND_ZLP[7]**; BLOCK_EVENT_INT[9]; **TRB_TYPE[15:10]**; DIR[16]; SETUP_TAG[18:17]; STATUS_STAGE_TRB_STALL[19]; **STATUS_STAGE_SET_ADDR[20]**; (isoc-only fields overlap [31:20]: FRAME_ID, SIA[31]) |

TRB types (`driver.rs:341-356`): 1=Normal, 3=DataStage, 4=StatusStage, 5=DataIsoch, 6=Link.
Events: 32=Transfer, 33=CmdCompletion, 34=PortStatusChange, 39=MfindexWrap, 40=SetupPkt.

**Event TRB** (`EventTrbS`, `driver.rs:669-719`):

| dword | fields |
| --- | --- |
| dw0 | Transfer events: **physical pointer to the completed transfer TRB**. SetupPkt events: setup bytes 0..3 |
| dw1 | SetupPkt: setup bytes 4..7 |
| dw2 | **TRB_TRAN_LEN[15:0] = residual (not-yet-transferred) length**; COMPL_CODE[31:24] |
| dw3 | CYCLE[0]; TRB_TYPE[15:10]; **ENDPOINT_ID[20:16] = PEI**; SETUP_TAG[22:21] |

Completion codes (`driver.rs:379-395`): 1=Success, 4=UsbTransactionError, 13=ShortPacket,
21=EventRingFull, 23=MissedService, 26=Stopped, 27=StoppedLenInvalid, 192=ProtocolStall,
193=SetupTagMismatch, 194=Halted, 195=HaltedLenInvalid, 196=Disabled.

**EP context** (`EpCxS`, 16 B, `driver.rs:603-666`):

| dword | fields |
| --- | --- |
| dw0 | EP_NUM[6:3]; INTERVAL[23:16] (xous: 3 for iso/intr endpoints, else 0) |
| dw1 | EP_TYPE[5:3] (see §2.2 encoding); MAX_BURST_SIZE[15:8] (xous: 15); MAX_PACKET_SIZE[31:16] (11 bits used) |
| dw2 | DEQ_CYC_STATE[0]; **DEQ_PTR_LO[31:4] = ring phys >> 4** |
| dw3 | 0 |

**ERST entry** (`ErstS`, 16 B, `driver.rs:720-729`): `{seg_addr_lo, seg_addr_hi, seg_size
(TRB count), rsvd=0}`.

### 2.4 Command register protocol

Write CMDPARA0/CMDPARA1, then CMDCTRL = `1 | (type << 4)`; poll CMDCTRL.ACTIVE==0; check
STATUS==0 (`driver.rs:1506-1535`). Xous first busy-waits for ACTIVE==0 before issuing
("overlapping commands can hang the system", `driver.rs:1507-1512`). **No timeouts anywhere.**

Command types (`driver.rs:151-166`) and parameter 0 layouts (utra.rs:103-106):

| Type | Name | param0 | param1 |
| --- | --- | --- | --- |
| 0 | INIT_EP0 | `ep0_ring_phys[31:4] | DCS[0]` (DCS = initial cycle state = 1) | 0 |
| 1 | UPDATE_EP0 | `MPS << 16` | 0 |
| 2 | SET_ADDR | `addr[7:0]` | 0 |
| 3 | SEND_DEV_NOTIFICATION | — | — |
| 4 | CONFIG_EP | `1 << pei` (context must be populated in DCBAP first) | 0 |
| 5 | SET_HALT | `1 << pei` | 0 |
| 6 | CLEAR_HALT | `1 << pei` | 0 |
| 7 | RESET_SEQ_NUM | — | — |
| 8 | STOP_EP | `1 << pei` | 0 |
| 9 | SET_TR_DQ_PTR | `1 << pei` (epcx.dw2 must be rewritten with new deq ptr first) | 0 |
| 10 | FORCE_FLOW_CONTROL | — | — |
| 11 | REQ_LDM_EXCHANGE | — | — |

### 2.5 Cycle-bit / ring semantics

* All rings start with **producer cycle state = 1 (true)** (`pcs = true` init: EP0
  `driver.rs:1429`, other EPs `driver.rs:2028`, event `ccs = true` `driver.rs:1349`).
* Transfer rings: last entry is a **Link TRB** pointing back to the ring base with
  LINK_TOGGLE_CYCLE=1 (`setup_link_trb` `driver.rs:479-489`; placement `driver.rs:1432-1433,2022-2024`).
  When software enqueue wraps onto the Link TRB it first writes the *current* PCS into the
  Link TRB's CYCLE bit, toggles PCS, then continues at ring base (`increment_enq_pt`,
  `driver.rs:780-800`).
* Event ring has **no Link TRB**; wrap is positional: `evt_seg0_last_trb` = base + 31; after
  consuming it, toggle CCS and reset the software dequeue pointer to base
  (`driver.rs:1606-1621`; service copy `services/usb-bao1x/src/hw.rs:369-385`).
* Consumer processes events while `event.dw3.CYCLE == ccs` (`driver.rs:1583-1585`).
* Transfer completion updates the software dequeue pointer from `event.dw0 + 1`; if that
  TRB is a Link, wrap to ring base (`driver.rs:2934-2942`; baremetal copy
  `baremetal/.../usb/driver.rs:127-133`).
* Alignment: TRB rings 16-byte aligned (low 4 bits dropped by hw: deq_ptr field, ERDP DQ_PTR,
  link target all `[31:4]`); ERST base and DCBAP 64-byte aligned (fields at bit 6; asserted
  `driver.rs:1378`). Page-aligned IFRAM allocation satisfies all.
* Max transfer per TRB = 1024 bytes (`MAX_TRB_XFER_LEN`, `driver.rs:81`); longer transfers are
  chained TRBs of ≤1024 with CHAIN=1 and IOC on the last (`ep_xfer`/`bulk_xfer`
  `driver.rs:1914-1975,2191-2241`).

---

## 3. Initialization sequence (exact)

`reset()` (cold, `driver.rs:1192-1266`) — boot1/kernel call this **before** `init()`:

1. Write the magic PHY table (§1.4), in table order.
2. `USBCMD.INT_ENABLE = 0` (rmwf).
3. `USBCMD.SOFT_RESET = 1`; poll `USBCMD.SOFT_RESET == 0`.
4. Dummy readback: read 72 words at offsets 0..0x120 ("a dummy readback is in the reference
   code", `driver.rs:1260-1264`).

`init(speed)` (`driver.rs:1268-1393`):

5. Zero the whole IFRAM allocation (`CRG_IFRAM_PAGES * 4096` at MEMBASE, `driver.rs:1270-1276`).
6. `USBCMD.INT_ENABLE = 0`, then `USBCMD.RUN_STOP = 0`.
7. `USBCMD.SOFT_RESET = 1`; poll `== 0`.
8. `DEVCONFIG = 0x80 | maxspeed` (FS: 0x81; HS: 0x83; LS: 0x80) (`driver.rs:1287-1292`).
9. `EVENTCONFIG = CSC|PEC|PPC|PRC|PLC|CEC` (bits 0,1,3,4,5,6) (`driver.rs:1294-1302`).
10. Build ERST at MEMBASE+0 (1 entry: seg = MEMBASE+0x100, size 32); zero event ring at
    MEMBASE+0x100 (32×16 B).
11. `ERSTSZ = 1`; `ERSTBALO = MEMBASE`; `ERSTBAHI = 0`;
    `ERDPLO = (event_ring & ~0xF) | EHB(1<<3)`; `ERDPHI = 0` (`driver.rs:1359-1367`).
12. `IMAN = IE|IP` (sets IE=1, clears IP); `IMOD = 0` (`driver.rs:1369-1370`).
13. `DCBAPLO = MEMBASE+0x300` (64B-aligned EP-context array); `DCBAPHI = 0`
    (`driver.rs:1373-1381`).
14. `init_ep0()` (`driver.rs:1395-1448`): zero 16-TRB ring at MEMBASE+0x500; enq=deq=base;
    pcs=1; last TRB = Link→base (toggle=1); issue **INIT_EP0** with
    `param0 = ring_phys | DCS=1`; record `ep0_buf = MEMBASE+0x2600`. EP0 MPS 64.
15. `U3PORTPMSC = 0`; `U2PORTPMSC = 0` (`driver.rs:1387-1390`).

`start()` (`driver.rs:1458-1504`):

16. `EVENTCONFIG |= INACTIVE_PLC|USB3_RESUME_NO_PLC|USB2_RESUME_NO_PLC` (bits 12,13,14).
17. `USBCMD |= SYS_ERR_ENABLE|INT_ENABLE|RUN_STOP` (`driver.rs:1472-1478`).
18. `IMAN.IE = 1` (rmwf).
19. `set_addr(0)`: issue SET_ADDR(0), then enqueue an EP0 status-stage TRB with
    STATUS_STAGE_SET_ADDR=1 and ring doorbell 0 (`driver.rs:1694-1708,1503`).
20. (caller) `update_current_speed()`: read PORTSC SPEED field, and issue UPDATE_EP0 with
    MPS 64 (FS/HS/LS) or 512 (SS) (`driver.rs:1652-1679`).
21. (caller) clear irqarray1 `EV_PENDING = 0xFFFF_FFFF`, then set `EV_ENABLE` bit 0
    (`glue.rs:134-135`, `services/usb-bao1x/src/hw.rs:156-157`).

boot1 order-of-operations at power-on (with SE0 asserted, see §6):
`init_usb()` (construct driver, hook handler, `enable_irq(1)`) → `reset()` → `init(speed)`
→ `start()` → `update_current_speed()` → irqarray clear/enable
(`bao1x-boot/boot1/src/platform/bao1x/usb/glue.rs:107-145`, `.../driver.rs:13-29,70`).

Endpoint enable (after SET_ADDRESS! quirk §7): `ep_enable(ep, dir, mps, type)`
(`driver.rs:1982-2062`): clear 64-TRB ring at `MEMBASE+0x600+(pei-2)*0x400`; write Link TRB
in last slot; fill EP context at `DCBAP+(pei-2)*16`; issue CONFIG_EP(`1<<pei`); **poll
`EPENABLE & (1<<pei)` until set** (spin, no timeout). Disable: if `EPRUNNING & (1<<pei)`,
STOP_EP and poll `EPRUNNING & (1<<pei) == 0`; zero the context; write `1<<pei` to EPENABLE
(`driver.rs:2064-2087`).

---

## 4. Event handling & interrupt deassertion (level IRQ!)

### 4.1 The IRQ line

irqarray1 @ `0xE000_5000`, RISC-V external IRQ **1** (`IRQARRAY1_IRQ`, utra
`bao1x.rs:681`). Registers (word offsets → bytes): EV_SOFT 0x00, EV_EDGE_TRIGGERED 0x04,
EV_POLARITY 0x08, EV_STATUS 0x0C, **EV_PENDING 0x10 (W1C)**, **EV_ENABLE 0x14**
(`bao1x.rs:615-681`). Bit 0 = `USBC_DUPE` = Corigine controller interrupt; bit 1 = soft IRQ.
Xous runs it **level-triggered, active-low default** (`hw.rs:147-149` writes
EV_EDGE_TRIGGERED=0 and EV_POLARITY=0; commented-out experiments with edge mode at
`driver.rs:1492-1493`). Enable value: bit0 only in boot1 (`glue.rs:135`), bits 0|1 in the OS
service (CORIGINE_IRQ_MASK|SW_IRQ_MASK = 3, `hw.rs:256-257,157`).

### 4.2 Servicing order (all required to drop the level)

From `udc_handle_interrupt` (`driver.rs:1537-1559`), boot1's trampoline
(`bao1x-boot/.../irq.rs:219-266`), and the service handler (`hw.rs:259-425`):

1. **Immediately** read irqarray1 `EV_PENDING`, write it back (or `0xFFFF_FFFF`) to clear,
   and re-set `EV_ENABLE` — so events raised during handling are not lost
   (`hw.rs:264-270`).
2. Read `USBSTS`:
   * `SYSTEM_ERR` set → write 1 to USBSTS.SYSTEM_ERR (W1C), log USBCMD. Fatal-ish.
   * `EINT` set → write 1 to **USBSTS.EINT (W1C)**; then clear **IMAN.IP (write 1)**;
     drain the event ring (loop below); then write **ERDP = (new_dq & ~0xF) | EHB**
     and `ERDPHI = 0` (`driver.rs:1547-1549,1561-1563,1628-1634`).
3. After the ring work: if `IMAN.IE` reads 1, re-write `IMAN = IE|IP`
   (`driver.rs:1551-1553`, `hw.rs:396-398`).

**Deassertion requires**: USBSTS.EINT cleared + IMAN.IP cleared + ERDP written with EHB=1 +
irqarray1 EV_PENDING cleared. Miss any and the level line stays asserted (interrupt storm).

### 4.3 Event dispatch semantics

* **PortStatusChange (34)**: read PORTSC; **write the read value back** to clear CSC/PPC/PRC/
  PLC/CEC change bits (`driver.rs:2908-2910`); if PRC && !PR → bus reset finished →
  `update_current_speed()` and (in the service) signal device-stack reset; if CSC&&PPC&&PP&&CCS
  → cable connect → update speed. Finally **set `EVENTCONFIG.SETUP_ENABLE = 1`** — xous
  re-arms setup delivery on every port status change (`driver.rs:2926`; baremetal copy
  `.../usb/driver.rs:110`).
* **Transfer (32)**: `pei = dw3[20:16]`; `residual = dw2[15:0]`; completed-TRB pointer in dw0.
  Dequeue-advance: `deq = dw0+1`; if Link → wrap to ring base. PEI 0: Success or
  SetupTagMismatch(193) → EP0 phase complete (IN→`Data(0,1,0)`, OUT→`Data(1,0,0)`); other
  codes ignored. PEI ≥2: Success or ShortPacket(13) → transfer done, actual length =
  `trb.dw2.transfer_len − residual` (`driver.rs:2928-2996`, esp. `2977-2983`). Short packet
  is a *success* variant: `len = (p_trb.dw2 & 0xffff) - residual`.
* **SetupPkt (40)**: dw0/dw1 hold the 8 setup bytes LE; `setup_tag = dw3[22:21]` must be
  echoed in subsequent Data/Status TRBs' SETUP_TAG field (`driver.rs:2998-3016`).
* **CmdCompletion (33) / MfindexWrap (39)**: not handled (fall to "unexpected" log).
* Note the baremetal EP0 event direction FIXME: the dir→report mapping is intentionally
  *inverted* vs the PEI parity bit and "seems necessary to trigger the next packet send"
  (`baremetal/.../usb/driver.rs:140-144`). The std-driver version uses the straight mapping
  (`driver.rs:2947-2961`). Test both when porting; the inverted one is what ships in boot1.

### 4.4 Interrupt enable bits

Controller-level gates: `USBCMD.INT_ENABLE` (bit 2) and `IMAN.IE` (bit 1) plus
`EVENTCONFIG` per-event-type enables. Xous masks at the irqarray (EV_ENABLE=0) around
ring-touching sections rather than at the controller (`CorigineWrapper::disable_interrupts`,
`driver.rs:2344-2349`).

---

## 5. IFRAM

### 5.1 Extent and board allocations

IFRAM0 `0x5000_0000`+128 KiB, IFRAM1 `0x5002_0000`+128 KiB (`bao1x.rs:303-306`). It is
general DMA scratch RAM shared by all UDMA peripherals; USB grabs pages at the top of a bank:

| Board/build | Pages | Base | Citation |
| --- | --- | --- | --- |
| default (kernel, non-dabao) | 5 | `0x5003_B000` (IFRAM1 end − 5×4K) | `driver.rs:61-65` |
| Dabao (kernel + boot1) | 23 | `0x5002_9000` (IFRAM1 end − 23×4K) | `board/dabao.rs:22-24` |
| Baosec | 5 | `0x5001_B000` (IFRAM0 end − 9×4K) | `board/baosec.rs:50-51`, `dabao.rs:18` |

The OS asserts `CRG_UDC_TOTAL_MEM_LEN (0x3700) ≤ pages*4K`
(`services/usb-bao1x/src/main.rs:108-110`).

### 5.2 Layout used by xous (offsets from MEMBASE)

Computed from `driver.rs:49-79`:

| Off | Size | Contents |
| --- | --- | --- |
| 0x000 | 0x100 | ERST (1 used entry, 16 B) |
| 0x100 | 0x200 | event ring (32 TRBs) |
| 0x300 | 0x200 | EP contexts (16 B × (PEI−2), only PEI 2..9 real) |
| 0x500 | 0x100 | EP0 transfer ring (16 TRBs; last = Link) |
| 0x600 | 0x2000 | EP1..EP4 rings, 0x400 each, ring for PEI *n* at `0x600+(n−2)*0x400` |
| 0x2600 | 0x100 | EP0 data buffer (`ep0_buf`) |
| 0x2700 | 0x1000 | "app buffers": 8 × 512 B, slot `(ep−1)*2 + (OUT?1:0)`; EP0 repurposes `0x2600+idx` |
| total | 0x3700 | |

boot1 additionally treats the app-buffer 4 KiB as raw mass-storage scratch:
CBW @ `0x2700`, CSW @ `0x2900`, EP1 IN buf @ `0x2B00` (1 KiB), EP1 OUT @ `0x2F00` (1 KiB)
(`bao1x-boot/.../usb/handlers.rs:55-60`); the baremetal CDC build uses slot 2 as RX and
slot 3 as TX (`driver.rs:1088-1105`). **Adjacency hazard**: EP0 buffer (0x100) is small —
control data >256 B overruns into app buffers unless the caller bounds it; xous bounds
EP0 reads/writes to ≤64/256 (`CRG_UDC_EP0_REQBUFSIZE` usage, `driver.rs:2571-2574`).

### 5.3 Access properties

* Physically addressed by the UDC; xous maps it with **phys == virt** so pointers can be
  handed to hw unchanged (`driver.rs:925-934`; `services/usb-bao1x/src/main.rs:100-107`).
* **Byte-accessible**: all data paths use `u8` slices (e.g. `driver.rs:2574`, `handlers.rs:108`).
* **No cache maintenance is ever performed for USB**: only `compiler_fence(SeqCst)` around
  TRB writes (`driver.rs:488,522,554,599,1371,1704,1822...`). The kernel's RISC-V page flags
  carry no cache-attribute bits (`kernel/src/arch/riscv/mem.rs:22-36`) and
  `bao1x_hal::cache_flush()` (`libs/bao1x-hal/src/lib.rs:75-87`, `fence.i` + custom `0x500F`
  op) is never called from USB code. Treat IFRAM as coherent/uncached device RAM; a Zephyr
  port should map it non-cacheable (or prove coherence) — do not add speculative flushes
  where xous has none, and do not let normal .data/.bss land in IFRAM.
* **Ownership handoff**: boot1/kernel-loader and the OS both use the same MEMBASE constants;
  the region is "claimed" purely by convention (top-down page reservations documented in
  `board/dabao.rs:26-29`). A new owner must treat the whole `CRG_IFRAM_PAGES` region as
  boot1/OS property until the previous stage has called `stop()` (§6); after `stop()` + SE0
  re-enumeration the whole region is free to re-init (`init()` re-zeros it anyway,
  `driver.rs:1270-1276`).

---

## 6. PC13 / SE0 ownership handoff (disconnect trick)

The controller has no functional soft-disconnect in the xous path; disconnect/re-enumerate
is done by **GPIO-forcing SE0** on the USB port through the IOX pin mux.

### 6.1 Pin and IOX registers

* **DaBao: port C pin 13 (PC13)** (`libs/bao1x-hal/src/board/dabao.rs:35-36`;
  boot1 copy `bao1x-boot/.../bao1x.rs:39-40`). **Baosec: PF5** (`board/baosec.rs:310-313`).
* IOX base `0x5012_F000`. Relevant registers (byte offsets; per-port stride 4 for banked
  regs; generated defs at `utralib/src/generated/bao1x.rs:3085-3266`):
  * `SFR_AFSEL_CRAFSEL0..11` @ 0x00..0x2C — 2 bits/pin; PA=regs 0/1, PB=2/3, **PC=4/5**
    (PC13 → CRAFSEL5 bits 27:26... i.e. AFSEL reg 5, bit pair (13−8)*2), PD=6/7, PE=8/9,
    PF=10/11. Value 0 = GPIO function, 1 = AF1, ... (`iox.rs:87-181`).
  * `SFR_GPIOOUT_CRGO0..5` @ 0x130+4*port — output register, 1 bit/pin (`iox.rs:39-41`,
    generated `:3148`).
  * `SFR_GPIOOE_CRGOE0..5` @ 0x148+4*port — output-enable (1 = drive) (`iox.rs:31-33`,
    `:3166`).
  * `SFR_GPIOPU_CRGPU0..5` @ 0x160+4*port — pull-up enable (`iox.rs:35-37`, `:3184`).
  * `SFR_GPIOIN_SRGI0..5` @ 0x178+4*port — input value (`iox.rs:63-75`, `:3202`).
  * `SFR_CFG_SCHM_...0..5` @ 0x230+4*port — schmitt trigger (`:3223`).
  * `SFR_CFG_DRVSEL_CR_CFG_DRVSEL0..5` @ 0x260+4*port — 2 bits/pin drive strength
    (0=2 mA, 1=4 mA, ...) (`iox.rs:332-366`, `:3265`).
  * `SFR_PIOSEL` @ 0x200 — BIO mux; **PC13 maps to BIO bit 29** and must not be claimed by
    the BIO subsystem while used as SE0 (`iox.rs:290-299`).
* Setup used by xous for SE0 output: function=GPIO (AFSEL=0), dir=Output (OE=1),
  pullup=Enable, slow-slew=Enable, drive=2 mA (`board/dabao.rs:38-50`,
  `bao1x-boot/.../bao1x.rs:56-67`). Input/boot-switch mode: dir=Input, schmitt=Enable,
  pullup=Enable (`board/dabao.rs:53-65`).

### 6.2 Boot1 bring-up sequence (exact)

From `bao1x-boot/boot1/src/main.rs:218-258`:

1. Configure **both candidate SE0 pins** (baosec PF5 via `setup_usb_pins`, dabao PC13 via
   `setup_dabao_se0_pin`) as GPIO outputs and **drive Low** — SE0 asserted = device
   disconnect state while board type is still unknown (main.rs:220-223).
2. Spend ≥250 ms initializing display/logo (or `delay(250)`) — this doubles as SE0 hold time.
3. `glue::setup(speed)` = full controller init (§3) while SE0 still asserted
   (main.rs:239-244; speed comes from the `UsbDefaultSpeed` one-way fuse: Full→FS else HS).
4. `delay(150)` (main.rs:245).
5. **Release SE0**: drive both pins High (main.rs:246-247).
6. On dabao-family boards, immediately reconfigure PC13 as **input** (boot-switch read mode,
   main.rs:248-255 → `setup_dabao_boot_pin`); baosec keeps PF5 driving high.
7. Poll for Configured state / disconnect by PORTSC (main.rs:274-356).

Baremetal hot-plug variant (`baremetal/.../usb/glue.rs:64-76`): SE0 low → 500 ms →
`setup()` → 500 ms → SE0 high.

### 6.3 Shutdown / handoff to the next stage (boot1 → kernel)

`boot()` at `bao1x-boot/boot1/src/main.rs:396-402`:

1. `glue::shutdown()` (`bao1x-boot/.../usb/glue.rs:147-157`): set `USB_CONNECTED=false`;
   `disable_all_irqs()` (RISC-V mie); then `usb.stop()` = **`IMAN.IE=0`; `USBCMD.INT_ENABLE=0`;
   `USBCMD.RUN_STOP=0`; `EVENTCONFIG=0`; irqarray1 `EV_PENDING=0xFFFF_FFFF`;
   `EV_ENABLE.USBC_DUPE=0`** (`driver.rs:1685-1692`).
2. Reconfigure the **active board's** SE0 pin as GPIO **Output**, drive **Low** (re-assert
   SE0 "so we re-enumerate with the OS stack", main.rs:400-401). The off-board candidate
   pin is set to input first (main.rs:365-381).
3. Comment of record: *"stop the USB subsystem so it can be re-init'd by the next stage.
   without this, USB init will hang later on"* (main.rs:397-398) — **a new owner MUST
   see USBCMD.RUN_STOP=0 and interrupts dead, or its init will hang** (the
   issue_command/EPENABLE polls never return).
4. The Xous kernel side then: `setup_usb_pins()` (output mode), and **releases SE0 by
   switching the pin to Input** (not by driving high) before the stack comes up
   (`services/usb-bao1x/src/main.rs:276-278`). On baosec, note: *"if SE0 is required, the
   KPC has to be un-configured to allow the SE0 I/O to actually be driven"* (main.rs:278) —
   the keyboard controller mux steals the pad otherwise.

**Electrical/functional summary for the Zephyr port**: assert disconnect = PC13 GPIO-out,
low, ≥250 ms; release = either drive high (boot1 style) or float as input with pull-up
(kernel style); after release the host takes ~1–2 s (debounce + reset) to re-enumerate.
Never leave both OE=1 low across a stage transition unless intentional.

---

## 7. Full-speed specifics, quirks, and failure modes

* **Speed selection** is purely `DEVCONFIG.MAX_SPEED` (§1.2) — FS=0x81, HS=0x83, LS=0x80.
  There is no separate HS-PHY init in software (the magic table covers PHY tuning). A note
  survives in git history that HS "sort of works" but had protocol/signal-integrity
  concerns; current code defaults to HS in the OS (`init(None)`, `driver.rs:2504`) and FS in
  boot1 when the `UsbDefaultSpeed::Full` fuse is set (main.rs:239-244). Post-reset speed is
  read back from `PORTSC[13:10]` (`driver.rs:1652-1672`) and EP0 MPS reprogrammed (64 for
  FS/HS/LS, 512 for SS) via UPDATE_EP0.
* **Max packet sizes**: FS bulk 64, FS interrupt 8; HS bulk 512, HS interrupt 16; EP0 64
  (`baremetal/.../usb/mod.rs:103-106`; `bao1x-boot/.../handlers.rs:66-69`).
* **QUIRK_SET_ADDRESS_BEFORE_STATUS** (`driver.rs:2354-2358`): the core cannot have EPs
  enabled until the device address is set; xous issues SET_ADDR and a status TRB with
  STATUS_STAGE_SET_ADDR=1 *during* the SET_ADDRESS control transfer
  (`driver.rs:1694-1708`), and defers all `ep_enable()` calls to after
  `set_device_address` (`driver.rs:2530-2545`). `max_packet_size_0(64)` is "*required by
  the corigine stack*" (`services/usb-bao1x/src/hw.rs:105-107`).
* **EP0 IN exactly 64 bytes cannot end a data phase**: xous uses `ep0_enqueue` (no status
  TRB) and expects a follow-up short/ZLP transfer; other lengths use `ep0_send` (data TRB +
  status TRB) (`driver.rs:2567-2583`).
* **EP0 OUT data stages must be armed manually**: the driver has no auto-priming of EP0
  receive; the handler hard-codes `SET_LINE_CODING` (0x21/0x20, wLength 7) priming
  (`driver.rs:3006-3014`); boot1 arms it in its setup dispatcher
  (`bao1x-boot/.../usb/driver.rs:303-313`). A generic stack (Zephyr) must prime EP0-OUT
  TRBs for every control OUT data phase, with the exact wLength.
* **Setup delivery enable**: EVENTCONFIG.SETUP_ENABLE is not set by init/start; it is set
  on every PortStatusChange event (`driver.rs:2926`). Set it in init and re-set after each
  bus reset.
* **PORTSC change-bit ack**: write the read value back verbatim (`driver.rs:2910`).
* **Stalls**: EP0 stall = status TRB with STATUS_STAGE_TRB_STALL=1 (`ep0_enqueue_zlp`,
  `driver.rs:1846-1853`); other EPs use SET_HALT/CLEAR_HALT + SET_TR_DQ_PTR re-ring
  (`driver.rs:2089-2122`). Known-bad: the EP0 stall path "works with linux, but not with
  windows" (`driver.rs:2133`), and the `ep_halt`/`ep_unhalt` EPRUNNING polls test the whole
  register rather than one bit (`driver.rs:2098,2117`) — copy semantics with care.
* **Missing IN-ACK interrupts**: the app-buffer allocator deliberately ignores overflow
  because "we aren't getting all the interrupts we expect" (`driver.rs:1114-1124`) — the
  OUT-side buffering is more robust than it looks; don't "fix" it blindly.
* **Disconnect detection** is by exact PORTSC match: `0x40B` (FS) / `0xC6B` (HS)
  (`glue.rs` both copies, e.g. `baremetal/.../usb/glue.rs:8-11`) — brittle; prefer CCS/PRC
  events in a new driver, but keep these as reference values.
* **Bounded waits — there are none**: every poll (`SOFT_RESET`, CMDCTRL.ACTIVE, EPENABLE,
  EPRUNNING) is an infinite spin (`driver.rs:1256,1509,1525,2056,2072`). The documented
  hang mode is "USB init hangs if previous stage didn't stop" (main.rs:397-398) and
  "overlapping commands can hang the system" (`driver.rs:1507`). Zephyr should wrap all
  these in `k_timeout`-bounded spins.
* `SET_SEL` handling inserts a **100 ms busy delay** (d11ctime timer abuse,
  `.../usb/driver.rs:209-215,77-91`).
* The event ring handler aggregates events into one `CrgEvent` per IRQ; a Connect arriving
  concurrently with data "cannot be handled" by the API (`driver.rs:1637-1640`).

---

## 8. CDC-ACM usage on top (capability scoping)

* **boot1/baremetal hand-rolled CDC-ACM** (`baremetal/.../usb/mod.rs:77-112`): composite
  device, IAD; interfaces 0 (CDC control) + 1 (CDC data); endpoints **EP2 IN interrupt**
  (notif, MPS 16 HS / 8 FS, interval 9 HS uframes / 10 FS frames) and **EP3 bulk OUT + EP3
  IN** (MPS 512 HS / 64 FS). EP0 MPS 64. GET_LINE_CODING returns fixed 115200 8N1;
  SET_CONTROL_LINE_STATE no-ops (`baremetal/.../usb/driver.rs:303-338`). TX path pushes a
  queue into a 512 B IFRAM slot per bulk_xfer and re-arms on IN-complete
  (`baremetal/.../usb/handlers.rs:135-187`); RX re-arms OUT on every completion
  (`handlers.rs:120-123`). The HS descriptor set is served for `CONFIGURATION` and the FS
  set for `OTHER_SPEED_CONFIGURATION` (`handlers.rs:47-56`) — a quirk worth noting.
* **OS service** uses `usb-device` + `usbd-serial`: `SerialPort` with 1 KiB rx/tx backing
  buffers, `max_packet_size_0(64)`, `composite_with_iads()`, HID FIDO + NKRO keyboard
  alongside (`services/usb-bao1x/src/hw.rs:29,72-108`). Serial chunks are 512 B
  (`SERIAL_MAX_PACKET_SIZE`, `hw.rs:29`). ZLP conventions: bulk transfers always use
  explicit TRB lengths; `APPEND_ZLP` TRB bit exists (`CRG_XFER_AZP`, `driver.rs:88`) but is
  unused by CDC paths.
* **boot1 mass storage (MSC/UF2)** shows the multi-EP pattern: EP1 bulk IN/OUT with CBW/CSW
  in IFRAM app-buffer space and chained big reads/writes (`bao1x-boot/.../handlers.rs:55-69`,
  `driver.rs:990-1054`).

---

## 9. C driver port checklist (Zephyr `udc` driver)

Ordered work list, mapping xous behavior → Zephyr:

1. **Devicetree / mapping**: MMIO `0x5020_2000` (0x3000), irq = irqarray1 line, IRQn 1,
   level-triggered; IFRAM region per board (dabao `0x5002_9000`, 23 pages; default 5 pages
   at `0x5003_B000`). Map IFRAM non-cacheable. Reserve it from any Zephyr heap/linker use.
2. **udc_data/ep caps**: 1 control EP + 4 IN/4 OUT pairs (EP1..EP4, PEI 2..9), MPS from
   PORTSC speed; `ep2cap` mapping must use PEI math (§2.2), including the Corigine
   IN/OUT naming inversion.
3. **Init**: implement §3 exactly — magic table → INT_ENABLE=0 → SOFT_RESET poll → dummy
   readback → DEVCONFIG speed → EVENTCONFIG → ERST/ERDP(EHB) → IMAN(IE|IP)/IMOD=0 → DCBAP →
   EP0 ring + INIT_EP0 cmd → U3/U2PORTPMSC=0 → EVENTCONFIG extras → USBCMD
   (SYS_ERR|INT_EN|RUN) → SET_ADDR(0) → UPDATE_EP0 MPS. Add Zephyr-style bounded waits.
4. **SE0 bring-up policy** (§6): PC13 GPIO-out low ≥250 ms before init; release (drive high
   or float) after init + ~150 ms; on receiving the handoff from boot1, expect
   `USBCMD.RUN_STOP=0` already; implement `udc_ep_disable`-all + stop sequence symmetric to
   xous `stop()` for any downstream handoff.
5. **TRB engine**: 16-byte TRBs, per-EP 64-slot rings with Link TRB + PCS toggle in software;
   EP0 16-slot ring; chain >1024 B transfers; doorbell = PEI write to 0x440.
6. **Event ring**: 32 TRBs, positional wrap, CCS tracking, ERDP|EHB update after drain;
   SETUP_ENABLE bit set at init **and after each port-status-change**.
7. **IRQ handler** (§4): clear irqarray pending immediately; W1C USBSTS.EINT/SYSTEM_ERR;
   W1C IMAN.IP; drain ring; ERDP write; re-assert IMAN.IE. Verify the line actually drops —
   this is the critical level-IRQ contract.
8. **Control transfers**: setup delivered as TRB type 40 with tag; echo SETUP_TAG in
   data/status TRBs; SET_ADDRESS → SET_ADDR command + status TRB with SET_ADDR bit before
   status completes; prime EP0 OUT data stages explicitly with wLength; 64-byte IN data
   phase must be followed by a short/ZLP; stall = status TRB bit for EP0.
9. **Non-control EPs**: enable only after address set; CONFIG_EP + EPENABLE poll; context
   dw1 fields (type +4 for OUT, burst 15, MPS); STOP_EP/SET_TR_DQ_PTR for disable/unhalt.
10. **Speed handling**: DEVCONFIG write for desired max speed (FS for this ticket), read
    back PORTSC after reset, UPDATE_EP0 MPS, report via `udc_bus_speed`.

**Do NOT port** (xous-isms a Zephyr driver should replace):

* The console-shim/repl and hand-rolled USB device stack (boot1/baremetal descriptor
  machinery, `CrgEvent` PollResult emulation, VecDeques) — use Zephyr's `udc_api` +
  `usb_device`/CDC-ACM class stack instead.
* Infinite busy-waits — wrap with timeouts and surface errors.
* Exact-value PORTSC disconnect matching (0x40B/0xC6B) — use proper CCS/event-based detect.
* The irqarray SW-IRQ trick (`EV_SOFT`, bit 1) used to self-IPI from userspace threads
  (`hw.rs:160-163,399-424`) — Zephyr has threads/queues.
* The "ignore app-buffer overflow" hack and the Windows-unfriendly EP0 stall workaround —
  re-derive from the hardware behavior, keeping §7 quirks in mind.
* Their IFRAM phys==virt pointer aliasing — use proper DMA address mapping.
* The hand-maintained per-board IFRAM page reservations — express in devicetree
  (`zephyr,memory` or IOFRAM region), not in C constants.

---

## Cross-references

* `hal_bao` ticket `halbao-m5-usb-udc` (this document's consumer).
* xous-core sources enumerated in the table at the top — in particular
  `libs/bao1x-hal/src/usb/driver.rs` is the ground truth for every register transaction.
