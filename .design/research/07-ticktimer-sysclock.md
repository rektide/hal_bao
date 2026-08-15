---
type: Research
title: Baochip ticktimer system clock contract
description: RTL-proven ticktimer semantics and the bounded, Verilator-validated 1 MHz Zephyr system clock.
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
  - id: zephyr-baochip-bounded-alarm
    resource: urn:git:commit:09c0c221b34fc694e9a3a45877a04269421d48f2
    title: Local Zephyr commit implementing bounded alarm programming
  - id: zephyr-baochip-takeover
    resource: urn:git:commit:5238ced66f59b9fc5694d88c2007d90817aad4e0
    title: Local Zephyr tip implementing bounded Baochip ticktimer operations
  - id: zephyr-baochip-runtime-overlay
    resource: urn:git:commit:69c8bd9e46b27b25a785a1a44c767fc244a46b80
    title: Local Zephyr runtime-test overlay routing output through DUART
---

# Baochip ticktimer system clock contract

This note records the final design and current evidence for
`halbao-m2-sysclock`. The MMIO system timer at `0xe001b000` is implemented in
the local Zephyr tree through commits `09c0c221b34f` (bounded alarm commits) and
`5238ced66f59` (bounded counter reads). With the DUART test overlay in
`69c8bd9e46b2`, both tickless and periodic kernel tests pass in the Baochip
Verilator model. Hardware validation remains open.

> **Correction, 2026-08-14:** The initial version incorrectly configured the
> hardware counter itself for 1 kHz and attempted to preserve its inherited
> epoch. That leaves no sub-tick cycle resolution and does not establish which
> divider produced the inherited count. The corrected contract resets the
> counter after selecting an exact 1 MHz post-divider rate, retains Zephyr's
> 1 kHz tick rate, and expresses every baseline, elapsed-time, and deadline
> calculation in 1 MHz `TIME` counter cycles. It also removes ISR-side
> provisional rearming: after an interrupt, the kernel selects and programs the
> next deadline through `sys_clock_set_timeout()` in both tickless and periodic
> configurations.

## Hardware contract

The block has nine 32-bit registers. Offsets and fields agree with generated
[`utralib`](https://github.com/betrusted-io/xous-core/blob/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b/utralib/src/generated/bao1x.rs#L2047-L2078).

| Offset | Register | Access | Semantics |
|---:|---|---|---|
| `0x00` | `CONTROL` | RW pulse | Bit 0 `RESET`; `CSRField(..., pulse=True)` makes a write of 1 generate a one-cycle source-domain pulse, which `BlindTransfer` carries into the always-on domain. The destination pulse zeros `TIME` and loads the prescaler from `CLOCKS_PER_TICK`; it is not a persistent reset level. The takeover sequence writes 0 only after reset has been proved. |
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
fclk, the timer input is the 350 MHz CPU clock. The system-clock design uses an
exact 1 MHz hardware counter:

```text
INPUT_CLOCK_HZ                 = 350,000,000
CONFIG_SYS_CLOCK_HW_CYCLES_PER_SEC = 1,000,000
CLOCKS_PER_TICK                = 350,000,000 / 1,000,000 - 1 = 349
reload period                  = CLOCKS_PER_TICK + 1 = 350 input clocks
CONFIG_SYS_CLOCK_TICKS_PER_SEC = 1,000
CYC_PER_TICK                   = 1,000,000 / 1,000 = 1,000 counter cycles
```

Xous writes 350000
([`implementation.rs:117-126`](https://github.com/betrusted-io/xous-core/blob/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b/services/xous-ticktimer/src/platform/bao1x/implementation.rs#L117-L126)); that produces a
350001-input-clock period for Xous's approximately 1 kHz time base and must not
be copied into this 1 MHz Zephyr design.

Divider programming does not alter the live prescaler. Reset is therefore part
of divider ownership, not optional epoch cleanup. RTL defines `CONTROL.RESET`
as pulse-on-write and sends that pulse through `BlindTransfer`; in the
always-on domain reset has priority, sets `TIME = 0`, and loads the prescaler
from the new `CLOCKS_PER_TICK`
([`bao_core.py:1821-1853`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/verilate/bao_core.py#L1821-L1853)).
`BlindTransfer` suppresses another source pulse until the destination pulse has
been acknowledged
([`cdc.py:140-177`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/rtl/scripts/headergen/migen/genlib/cdc.py#L140-L177)).
The implemented takeover is therefore bounded and deliberately uses two reset
pulses:

```text
EV_ENABLE = 0
full data-synchronization fence
CLOCKS_PER_TICK = 349
full data-synchronization fence
CONTROL = RESET                   # first pulse through BlindTransfer
full data-synchronization fence
poll bounded coherent TIME reads until TIME != 0; else return -ETIMEDOUT
CONTROL = RESET                   # second pulse, after transfer progress
full data-synchronization fence
poll bounded coherent TIME reads until TIME == 0; else return -ETIMEDOUT
CONTROL = 0
full data-synchronization fence
last_count = 0                    # trusted only after the second observed reset
full data-synchronization fence
```

An immediate zero after the first write proves nothing: it can be a stale
synchronized zero inherited from boot. The first bounded phase instead obtains
a synchronized nonzero sample after programming the divider and issuing the
first pulse. It then issues the second pulse and waits for a synchronized zero,
which cannot be the previously observed sample. Because the fences order the
divider write before both reset requests, the nonzero-to-zero observation
proves that a reset crossed after divider ownership, even if `BlindTransfer`
coalesces closely spaced pulse requests. It does not rely on identifying which
request produced the observed reset, an old epoch, or assumed MMIO write order.
Reset leaves `TIME` at zero while the
350-input-clock prescaler counts down; the recurring `BusSynchronizer`
handshake refreshes the CPU-domain view during that interval
([`cdc.py:100-138`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/rtl/scripts/headergen/migen/genlib/cdc.py#L100-L138)).
Bounded coherent reads also prevent early boot from hanging forever if CDC or
the counter is not operating.

`TIME1` and `TIME0` are independent views, not an atomic read latch. Read
high-low-high and retry if the high words differ, but bound the operation to
three attempts:

```text
repeat at most 3 times:
    hi0 = TIME1; lo = TIME0; hi1 = TIME1
    if hi0 == hi1: return (hi1 << 32) | lo, coherent=true
return (latest hi1 << 32), coherent=false
```

On exhaustion, each mismatch proves that the counter reached the corresponding
`hi1` epoch. Returning the latest observed high word with a zero low word is
therefore a conservative floor for that latest epoch; it avoids combining a
new high word with a stale pre-rollover low word. At 1 MHz, the 32-bit low word
rolls only every `2^32 us`, or 4,294.967296 seconds (71 minutes 34.967296
seconds), so three consecutive rollover-window mismatches require pathological
CDC timing or multiple high-epoch advances during this short function. The
read uses only MMIO samples and local variables: it needs no software lock,
atomic operation, or mutable cross-call state. Takeover still accepts only a
result marked coherent when proving the nonzero-to-zero reset sequence.

Program a target high word first and low word last. RTL makes only the low-word
write assert `msleep_target_re`, which starts transfer and temporarily locks out
the system-domain alarm until the round trip completes
([`cram_axi.sv:22288-22295`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/rtl/modules/vexriscv/rtl/cram_axi.sv#L22288-L22295),
[`bao_core.py:1876-1907`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/verilate/bao_core.py#L1876-L1907)).

The comparator is `target <= time`, so an expired target remains asserted; a
pending clear alone immediately reasserts. Every arm or rearm must order MMIO
and check whether the committed target is still future. This operation is
bounded to three commits:

```text
EV_ENABLE = 0
full data-synchronization fence
MSLEEP_TARGET1 = target >> 32
MSLEEP_TARGET0 = target              # commit
full data-synchronization fence       # target transfer precedes pending clear
EV_PENDING = 1                       # W1C stale level
full data-synchronization fence
now = bounded TIME read              # mandatory after transfer and W1C
if target is not future and attempts remain:
    elapsed = floor((now - last_count) / CYC_PER_TICK)
    target = last_count + (elapsed + 2) * CYC_PER_TICK
    repeat commit, clear, and TIME recheck, for at most 3 total commits
full data-synchronization fence
EV_ENABLE = 1                        # future target, or expired third-target fallback
full data-synchronization fence
```

The recheck closes the time-of-check/time-of-use window in which a deadline can
expire while the target crosses domains or while stale pending state is
cleared. A retry retains the absolute phase relative to `last_count` and adds a
two-tick margin beyond the elapsed boundary, rather than using `now +
CYC_PER_TICK`, so late programming catches up without introducing phase drift
and has room for the CDC round trip. If all three targets expire, the driver
still enables the final expired target. Because the comparator is level
sensitive, that target requests a prompt catch-up IRQ instead of hanging in a
retry loop with the event disabled; this is the bounded liveness fallback.
The underlying disable, reprogram, clear, enable ordering is proven by Xous
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
disabled event. Counts produced before Zephyr selects its divider have no valid
relationship to `CONFIG_SYS_CLOCK_HW_CYCLES_PER_SEC`, so they must not be
preserved as Zephyr cycles.

Initialization must order ownership and the first alarm as follows:

```text
1. EV_ENABLE = 0.
2. Fence, program CLOCKS_PER_TICK = 349, and fence again.
3. Pulse CONTROL.RESET; bounded-poll coherent TIME until a nonzero sample.
4. Pulse CONTROL.RESET again; bounded-poll coherent TIME until synchronized zero,
   then write CONTROL = 0 and set last_count = 0. Fail initialization with
   `-ETIMEDOUT` if either observation cannot be made.
5. Commit a target with EV_ENABLE still zero, clear EV_PENDING, and recheck
   TIME; on expiry retry with the absolute two-tick margin, for at most three
   commits.
6. Connect and enable direct IRQ 356.
7. Set EV_ENABLE = 1 after the IRQ is ready. Normally the committed target is
   future; after three expiries the final expired level target deliberately
   produces the catch-up IRQ.
```

The first target is `last_count + CYC_PER_TICK`, one Zephyr tick boundary. This
bootstraps kernel accounting without enabling target zero or any inherited
alarm. After that first announcement, the kernel's normal post-announce path
calls `sys_clock_set_timeout()` with the next required wait.

## Zephyr driver model

Use the port's locked timer API. `sys_clock_set_timeout()` and
`sys_clock_elapsed()` arrive with the system clock lock held; the ISR obtains
`sys_clock_lock()`, updates private accounting, and transfers the key to
`sys_clock_announce_locked()`
([`system_timer.h:56-85`](https://github.com/zephyrproject-rtos/zephyr/blob/main/include/zephyr/drivers/timer/system_timer.h#L56-L85)).
No second driver spinlock is needed.

Driver state is a 64-bit `last_count` baseline measured exclusively in
post-divider 1 MHz `TIME` counter cycles. Set
`CONFIG_SYS_CLOCK_HW_CYCLES_PER_SEC=1000000`, retain
`CONFIG_SYS_CLOCK_TICKS_PER_SEC=1000`, and define `CYC_PER_TICK=1000`. Reject a
configuration where the hardware-cycle rate is not evenly divisible by the
kernel tick rate. The 350 MHz input-clock rate appears only in divider
derivation and validation; it must never enter timeout, elapsed, baseline, or
target arithmetic.

- `sys_clock_cycle_get_64()` returns the coherent raw count when available; if
  all three high-low-high attempts mismatch, it returns the conservative latest
  high-epoch floor. `sys_clock_cycle_get_32()` returns its low word. Select
  `TIMER_HAS_64BIT_CYCLE_COUNTER` and `SYSTEM_CLOCK_LOCK_FREE_COUNT` because
  the bounded read needs no mutable software state, lock, or atomic operation.
- `sys_clock_elapsed()` returns `floor((now - last_count) / CYC_PER_TICK)`
  in tickless mode and zero in periodic mode.
- The ISR's first hardware action is `EV_ENABLE = 0`. It then obtains
  `sys_clock_lock()`, reads `now`, computes
  `dticks = floor((now - last_count) / CYC_PER_TICK)`, and advances
  `last_count += dticks * CYC_PER_TICK`. It does not clear/re-enable early or
  advance the baseline to an arbitrary sub-tick `now`.
- In tickless mode, `sys_clock_set_timeout(ticks, idle)` ignores `idle`, clamps
  to the representable/kernel limit, includes already elapsed whole ticks, and
  arms an absolute tick-aligned target. With
  `elapsed = floor((now - last_count) / CYC_PER_TICK)`, the requested target is
  `last_count + (elapsed + ticks) * CYC_PER_TICK`. The exact future-boundary
  rule is
  `target = last_count + max(elapsed + ticks, elapsed + 1) * CYC_PER_TICK`,
  after applying the maximum-span clamp; thus a zero request and any target no
  longer in the future select the next tick boundary.
- Target programming is not complete at the low-word write. After target
  transfer and pending clear, the driver rereads `TIME`. If the target is no
  longer future, it recommits an absolute boundary with a two-tick margin. It
  makes at most three commit/read attempts; if the third target is also
  expired, enabling that level target forces a prompt catch-up IRQ for
  liveness.
- After updating `last_count`, the ISR calls
  `sys_clock_announce_locked(dticks, key)` with `EV_ENABLE` still zero. Current
  Zephyr then calls `sys_clock_set_timeout()` from its post-announce
  `reprogram_next(0)` path
  ([`timeout.c:337-406`](https://github.com/zephyrproject-rtos/zephyr/blob/c332f8ea93d9e0fff74a3b417533e219801b0690/kernel/timeout.c#L337-L406)); that callback performs the complete
  disable/target-high/target-low/W1C/enable sequence. There is no provisional
  ISR-side enable.
- In periodic mode, `sys_clock_elapsed()` remains zero, but
  `sys_clock_set_timeout()` must not be a no-op for this design: the kernel's
  post-announce call re-enables the alarm at
  `last_count + CYC_PER_TICK`, regardless of its `ticks` argument. Periodic mode
  cannot honor a multi-tick argument because `sys_clock_elapsed()` deliberately
  returns zero between announcements. The ISR announces the actual elapsed
  whole-tick count, not blindly one, so late service catches up and the next
  absolute one-tick target does not accumulate drift.

All arithmetic involving absolute targets and the baseline is unsigned 64-bit.
An expired arm selects a margin-bearing absolute boundary while attempts
remain; only exhaustion deliberately enables the already-high comparator.

## Implementation and runtime status

The implementation is in the local Zephyr tree through commit `5238ced66f59`,
with runtime overlay `69c8bd9e46b2`:

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
- Baochip SoC Kconfig/defconfig: `SYS_CLOCK_EXISTS=y`, 1 MHz hardware-cycle
  rate, `SYS_CLOCK_TICKS_PER_SEC=1000`, and no RISC-V machine timer.
- `09c0c221b34f`: limits alarm programming to three attempts, gives expired
  retries an absolute two-tick margin, and uses the final expired level target
  as the liveness IRQ fallback.
- `5238ced66f59`: limits high-low-high counter reads to three attempts and
  returns the conservative latest high-epoch floor on exhaustion; native logic
  tests cover immediate coherence, rollover then coherence, bounded exhaustion,
  and unsigned 64-bit wrap.
- `69c8bd9e46b2`: routes this test's console to the simulated DUART, making the
  kernel result observable in Verilator.

The preserved evidence is under
`/tmp/opencode/halbao-final-kernel-runtime/{tickless-final,periodic-final}`. Both
configurations boot Zephyr 4.4.99, run
`baochip_ticktimer.test_timekeeping_and_preemption`, report `PASS` in exactly
`0.019 seconds`, emit `TESTSUITE baochip_ticktimer succeeded`, and finish with
`PROJECT EXECUTION SUCCESSFUL`. The simulator process was externally timed out
only after ztest success, as recorded by `simulator_note=timeout_after_ztest_success`
and `result=PASS` in each `status.txt`.

The test sleeps for 12 ms and asserts that uptime advanced by at least 12 ms and
that both 32-bit and 64-bit cycle counters progressed. It then runs a busy
lower-priority thread while a higher-priority thread sleeps for 5 ms; the test
requires the timer wakeup to preempt the busy thread and signal within 100 ms.
For both images, generated devicetree selects IRQ 356 and the ISR table links
the timer handler at entry 356. Since no alternate system timer is configured,
successful sleep deadlines and timer-driven preemption establish that direct
IRQ 356 also fired, not merely that it linked.

This runtime evidence proves the integrated nominal path in the RTL model:
takeover completed, time and cycles advanced, `k_sleep()` did not return before
its deadline, scheduling preemption occurred, and direct IRQ 356 was linked and
serviced in both kernel modes. It does **not** inject CDC faults, force a
`TIME0` rollover during a read, force all three alarm commits to expire, measure
long-duration drift, exercise interrupt-load extremes, or substitute for
hardware measurements. The native logic tests establish the bounded fallback
algorithms for those synthetic cases, not their physical incidence.

## Validation matrix

| Layer | Required evidence |
|---|---|
| Build | Dabao builds in tickless and `CONFIG_TICKLESS_KERNEL=n` configurations; devicetree reports base, size, frequency, and IRQ 356; no CLINT/machine-timer driver is linked. |
| Unit/native logic | Divider calculation yields 349; `CYC_PER_TICK` is 1000; all formulas use counter cycles; target split is high then low; counter reads and alarm commits stop after three attempts; tests cover coherent reads, rollover retry, conservative floor and unsigned-wrap fallbacks, absolute two-tick-margin retries, final expired-target fallback, zero/one/maximum timeouts, late ISR, and elapsed-before-rearm. |
| Verilator, observed | Tickless and periodic images each emit `PROJECT EXECUTION SUCCESSFUL`; the sole ztest passes in 0.019 s. Its assertions establish a 12 ms `k_sleep()` deadline, uptime and 32/64-bit cycle progression, and timer wakeup preemption. Build artifacts link the sole system timer's direct ISR at IRQ 356, and successful timer-dependent completion establishes that it fires. |
| Verilator, still open | Inject CDC faults; force `TIME0` rollover during high-low-high reads; force three consecutive alarm-commit expiries and observe the level-IRQ fallback; verify exact divide-by-350 cadence, target-transfer/W1C edge cases, delayed-ISR catch-up, and sustained interrupt load independently of the nominal kernel test. |
| Hardware, still open | Confirm monotonic cycles/uptime and direct IRQ behavior on silicon; measure long-interval uptime/sleep drift; cover one- and many-tick sleeps, tickless idle, periodic interrupt load, `TIME0` rollover, CDC behavior, and clock-policy assumptions. |

## Risks and boundaries

- The documented 350 MHz input depends on the current Dabao 700 MHz fclk
  policy. Runtime CPU-clock changes require divider reprogramming plus Zephyr's
  runtime frequency update contract, or must be prohibited while the driver is
  active.
- Target transfer crosses clock domains and has documented approximately 200
  ns slip. Tests need tolerance; software must not busy-wait for exact equality.
- The level comparator makes ordering correctness mandatory. Enabling before a
  target commit and pending clear can create an unintended interrupt storm.
  The bounded algorithm's final expired target is a deliberate exception: it
  requests one prompt catch-up service for liveness, after which normal kernel
  reprogramming runs again.
- Reprogramming `CLOCKS_PER_TICK` does not reset the current prescaler. The
  required reset after divider programming is what establishes the exact first
  1 MHz counter interval and the trustworthy zero epoch.
- Suspend/resume through `susres` is outside M2. The counter is in the
  always-on domain, but Zephyr power-management integration needs separate
  validation before claiming deep-sleep timekeeping.

## Cross-references

- [`/.design/research/00-soc-inventory.md`](/.design/research/00-soc-inventory.md) - SoC clock tree, ticktimer placement, and direct CPU line inventory.
- [`/.design/research/04-synthesis.md`](/.design/research/04-synthesis.md) - milestone decision selecting ticktimer as the M2 system clock.
- [`/.design/research/06-irq-ack-semantics.md`](/.design/research/06-irq-ack-semantics.md) - flattened interrupt model and the distinction between direct lines and irqarray children.
- [`/doc/bringup/index.md`](/doc/bringup/index.md) - operator-facing validation documents; add the hardware timer procedure there only after an executable image exists.
