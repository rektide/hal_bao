---
type: Research
title: Baochip ticktimer system clock contract
description: RTL-proven ticktimer register semantics and an implementation-ready Zephyr system-clock design.
tags: [baochip, ticktimer, timer, sysclock, rtl, xous, zephyr]
status: stable
generated: { by: agent:opencode, at: 2026-08-14 }
sources:
  - id: baochip-rtl
    resource: https://github.com/baochip/baochip-1x/tree/83b220f790e7e846a6500264b480b42ad9ebd40b
    title: Baochip 1x RTL and LiteX generator
  - id: xous-core
    resource: https://github.com/betrusted-io/xous-core/tree/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b
    title: Xous Baochip ticktimer implementation
  - id: zephyr-system-timer-api
    resource: https://github.com/zephyrproject-rtos/zephyr/blob/main/include/zephyr/drivers/timer/system_timer.h
    title: Zephyr system timer API
---

# Baochip ticktimer system clock contract

This note closes the remaining design questions for `halbao-m2-sysclock`. It
specifies an MMIO system timer at `0xe001b000`; it does not claim that the
driver has been implemented or validated.

## Hardware contract

The block has nine 32-bit registers. Offsets and fields agree with generated
[`utralib`](https://github.com/betrusted-io/xous-core/blob/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b/utralib/src/generated/bao1x.rs#L2047-L2078).

| Offset | Register | Access | Semantics |
|---:|---|---|---|
| `0x00` | `CONTROL` | RW | Bit 0 `RESET`; writing 1 sends a reset pulse across the clock domain and zeroes `TIME`, writing 0 leaves it free-running. |
| `0x04` | `TIME1` | RO | `TIME[63:32]`, synchronized into the CPU clock domain. |
| `0x08` | `TIME0` | RO | `TIME[31:0]`. |
| `0x0c` | `MSLEEP_TARGET1` | RW | Comparator target high word; updates storage only. |
| `0x10` | `MSLEEP_TARGET0` | RW | Comparator target low word and target-transfer commit strobe. |
| `0x14` | `EV_STATUS` | RO | Bit 0 `ALARM`, the live level `TARGET <= TIME` while target transfer is not locked out. |
| `0x18` | `EV_PENDING` | RW1C | Bit 0 `ALARM`; for this level event it mirrors/reasserts from the live alarm condition. |
| `0x1c` | `EV_ENABLE` | RW | Bit 0 enables alarm contribution to the direct CPU interrupt. |
| `0x20` | `CLOCKS_PER_TICK` | RW | Prescaler reload value, not a cycle count. |

The always-on counter increments when the prescaler is zero and then reloads
`CLOCKS_PER_TICK`; otherwise the prescaler decrements
([`bao_core.py:1840-1852`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/verilate/bao_core.py#L1840-L1852)).
The period is therefore `reload + 1` input clocks. At Dabao's normal 700 MHz
fclk, the timer input is the 350 MHz CPU clock. Exactly 1 kHz requires:

```text
CLOCKS_PER_TICK = 350,000,000 / 1,000 - 1 = 349999
```

Xous writes 350000
([`implementation.rs:117-126`](https://github.com/betrusted-io/xous-core/blob/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b/services/xous-ticktimer/src/platform/bao1x/implementation.rs#L117-L126)); that produces a
350001-cycle period and must not be copied.

`TIME1` and `TIME0` are independent views, not an atomic read latch. Read
high-low-high and retry if the high words differ:

```text
do { hi0 = TIME1; lo = TIME0; hi1 = TIME1; } while (hi0 != hi1)
return (hi1 << 32) | lo
```

Program a target high word first and low word last. RTL makes only the low-word
write assert `msleep_target_re`, which starts transfer and temporarily locks out
the system-domain alarm until the round trip completes
([`cram_axi.sv:22288-22295`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/rtl/modules/vexriscv/rtl/cram_axi.sv#L22288-L22295),
[`bao_core.py:1876-1907`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/verilate/bao_core.py#L1876-L1907)).

The comparator is `target <= time`, so an expired target remains asserted; a
pending clear alone immediately reasserts. Every arm or rearm must be:

```text
EV_ENABLE = 0
MSLEEP_TARGET1 = target >> 32
MSLEEP_TARGET0 = target              # commit
EV_PENDING = 1                       # W1C stale level
EV_ENABLE = 1
```

The same disable, reprogram, clear, enable ordering is proven by Xous
([`implementation.rs:208-228`](https://github.com/betrusted-io/xous-core/blob/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b/services/xous-ticktimer/src/platform/bao1x/implementation.rs#L208-L228)).
The interrupt is CPU external line 20. In the port's flattened namespace direct
IRQs start at 336, making the DTS IRQ **356** (`336 + 20`); it must bypass all
irqarray bank MMIO. The mapping is defined by
`include/zephyr/dt-bindings/interrupt-controller/baochip-bao1x-intc.h` in the
Zephyr port (`BAO1X_IRQ_DIRECT_BASE` and `BAO1X_IRQ_DIRECT(line)`).

## Boot handoff

Boot1 does not initialize the ticktimer. The loader only reads `TIME0` for a
security UI delay
([`loader/src/secboot.rs:488-506`](https://github.com/betrusted-io/xous-core/blob/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b/loader/src/secboot.rs#L488-L506)).
Zephyr therefore inherits an already-advanced free-running count, the RTL
divider default, target zero, a likely asserted/pending alarm level, and a
disabled event. Initialization must not reset `TIME`: treat its current value
as an arbitrary epoch, disable the event, install 349999, clear stale pending,
capture the current count as the announcement baseline, connect/enable IRQ 356,
then arm the first deadline. This preserves monotonicity across handoff while
taking ownership of divider and alarm state.

## Zephyr driver model

Use the port's locked timer API. `sys_clock_set_timeout()` and
`sys_clock_elapsed()` arrive with the system clock lock held; the ISR obtains
`sys_clock_lock()`, updates private accounting, and transfers the key to
`sys_clock_announce_locked()`
([`system_timer.h:56-85`](https://github.com/zephyrproject-rtos/zephyr/blob/main/include/zephyr/drivers/timer/system_timer.h#L56-L85)).
No second driver spinlock is needed.

Driver state is a 64-bit `announced` hardware-tick baseline and, if useful for
avoiding repeated division, the last elapsed result. With the hardware at 1
kHz, set `CONFIG_SYS_CLOCK_HW_CYCLES_PER_SEC=1000`; derive hardware ticks per
Zephyr tick from `CONFIG_SYS_CLOCK_TICKS_PER_SEC` and reject configurations
where 1000 is not evenly divisible by it.

- `sys_clock_cycle_get_64()` returns the coherent raw count;
  `sys_clock_cycle_get_32()` returns its low word. Select
  `TIMER_HAS_64BIT_CYCLE_COUNTER` and `SYSTEM_CLOCK_LOCK_FREE_COUNT` because
  high-low-high needs no mutable software state.
- `sys_clock_elapsed()` returns `floor((now - announced) / hw_ticks_per_tick)`
  in tickless mode and zero in periodic mode.
- The ISR disables the level event before accounting. It reads `now`, computes
  all whole ticks since `announced`, advances `announced` by exactly that many
  tick quanta, and never advances it to an arbitrary sub-tick `now`.
- In tickless mode, `sys_clock_set_timeout(ticks, idle)` ignores `idle`, clamps
  to the representable/kernel limit, includes already elapsed whole ticks, and
  arms an absolute tick-aligned target. Zero requests use the earliest safe
  future boundary. The ISR clears the event and leaves it disabled before
  `sys_clock_announce_locked()`; the timeout processing re-enters
  `sys_clock_set_timeout()` under the same kernel lock to arm the next target.
- In periodic mode, `sys_clock_set_timeout()` is a no-op and
  `sys_clock_elapsed()` returns zero. Before announcing, the ISR rearms the next
  absolute tick boundary with the full disable/reprogram/clear/enable sequence.
  It announces the actual elapsed whole-tick count, not blindly one, so late
  service catches up without cumulative drift.

All arithmetic involving absolute targets and the baseline is unsigned 64-bit.
An arm whose computed target is no longer in the future must select the next
safe tick boundary rather than enabling an already-high comparator.

## Integration plan

The implementation belongs in the Zephyr tree, not this repository:

- `drivers/timer/baochip_ticktimer.c`: MMIO, coherent reads, alarm sequencing,
  locked accounting, ISR, and `SYS_INIT(... PRE_KERNEL_2,
  CONFIG_SYSTEM_CLOCK_INIT_PRIORITY)`.
- `drivers/timer/Kconfig.baochip_ticktimer`, `drivers/timer/Kconfig`, and
  `drivers/timer/CMakeLists.txt`: `BAOCHIP_TICKTIMER`, default on when the DT
  compatible is enabled; select `TICKLESS_CAPABLE`,
  `TIMER_HAS_64BIT_CYCLE_COUNTER`, and `SYSTEM_CLOCK_LOCK_FREE_COUNT`.
- `dts/bindings/timer/baochip,ticktimer.yaml`: one MMIO range, one interrupt,
  and a required input `clock-frequency` used to derive and validate the
  reload.
- `dts/riscv/baochip/bao1x.dtsi`: enabled node at `0xe001b000`, size `0x24`,
  `clock-frequency = <350000000>`, and `<356>` interrupt through `&intc`.
- Baochip SoC Kconfig/defconfig: `SYS_CLOCK_EXISTS=y`, 1 kHz hardware-cycle
  rate, a supported `SYS_CLOCK_TICKS_PER_SEC`, and no RISC-V machine timer.

## Validation matrix

| Layer | Required evidence |
|---|---|
| Build | Dabao builds in tickless and `CONFIG_TICKLESS_KERNEL=n` configurations; devicetree reports base, size, frequency, and IRQ 356; no CLINT/machine-timer driver is linked. |
| Unit/native logic | Divider calculation yields 349999; target split is high then low; rollover reads retry; timeout arithmetic covers zero, one, maximum wait, wrap, late ISR, and elapsed-before-rearm. |
| Verilator | Counter advances once per 350000 input clocks; inherited nonzero time is not reset; IRQ 356 dispatches directly; target transfer does not glitch; W1C while target is expired reasserts; the prescribed reprogram sequence produces one wake; delayed ISR catches up. |
| Hardware | `k_cycle_get_64()` and `k_uptime_get()` are monotonic; measured uptime/sleep drift is bounded over a long interval; `k_sleep()` covers one and many ticks; tickless idle wakes at deadlines; periodic mode remains stable under interrupt load and around `TIME0` rollover. |

## Risks and boundaries

- The documented 350 MHz input depends on the current Dabao 700 MHz fclk
  policy. Runtime CPU-clock changes require divider reprogramming plus Zephyr's
  runtime frequency update contract, or must be prohibited while the driver is
  active.
- Target transfer crosses clock domains and has documented approximately 200
  ns slip. Tests need tolerance; software must not busy-wait for exact equality.
- The level comparator makes ordering correctness mandatory. Enabling before a
  future target has committed can create an immediate interrupt storm.
- Reprogramming `CLOCKS_PER_TICK` does not reset the current prescaler. The first
  interval after handoff may retain part of the inherited period; baseline
  capture and absolute deadlines prevent cumulative error, but tests should not
  demand exact first-tick phase.
- Suspend/resume through `susres` is outside M2. The counter is in the
  always-on domain, but Zephyr power-management integration needs separate
  validation before claiming deep-sleep timekeeping.

## Cross-references

- [`/.design/research/00-soc-inventory.md`](/.design/research/00-soc-inventory.md) - SoC clock tree, ticktimer placement, and direct CPU line inventory.
- [`/.design/research/04-synthesis.md`](/.design/research/04-synthesis.md) - milestone decision selecting ticktimer as the M2 system clock.
- [`/.design/research/06-irq-ack-semantics.md`](/.design/research/06-irq-ack-semantics.md) - flattened interrupt model and the distinction between direct lines and irqarray children.
- [`/doc/bringup/index.md`](/doc/bringup/index.md) - operator-facing validation documents; add the hardware timer procedure there only after an executable image exists.
