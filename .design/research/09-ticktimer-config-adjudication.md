---
type: Decision
title: Baochip ticktimer configuration adjudication
description: Reconciles configurable-rate simulation evidence with the RTL proof required to own the divider and reset epoch safely.
tags: [baochip, ticktimer, zephyr, rtl, cdc, adjudication]
status: stable
generated: { by: agent:opencode, at: 2026-08-17 }
sources:
  - id: baochip-rtl
    resource: https://github.com/baochip/baochip-1x/tree/83b220f790e7e846a6500264b480b42ad9ebd40b
    title: Baochip 1x RTL and LiteX generator
  - id: zephyr-configurable-driver
    resource: urn:git:commit:5f9bf7e519c901799cadfa05edde76bf9b185253
    title: Local Zephyr configurable ticktimer implementation
  - id: zephyr-configurable-tests
    resource: urn:git:commit:9f1bb96cb066b79458919e4943afee7749b35ec4
    title: Local Zephyr configurable ticktimer tests
---

# Baochip ticktimer configuration adjudication

## Decision status and scope

**Accepted, superseding the generalized takeover and rate matrix in local
Zephyr commits `5f9bf7e519c9` and `9f1bb96cb066`.** Preserve their standard
configuration interface, chosen-node selection, exact arithmetic diagnostics,
and reusable tests. Replace their three-distinct-sample takeover with the
two-reset sequence below and limit configurations to the conservative
software-only envelope defined here.

This decision covers boot-time adoption of the Baochip 1x ticktimer as Zephyr's
system timer with a fixed input clock. It does not approve runtime input-rate or
hardware-cycle-rate changes, unrestricted rates, suspend/resume restoration, or
an RTL change. It reconciles the fixed 1 MHz proof in
[`07-ticktimer-sysclock.md`](07-ticktimer-sysclock.md), the provider constraints
in [`08-device-creation-reform.md`](08-device-creation-reform.md), and the later
generalization in the two Zephyr commits.

## Claims and adjudication

| Claim | Evidence | Counterargument | Adjudication | Confidence |
|---|---|---|---|---|
| 100 kHz, 1 MHz, and 10 MHz configurations operate in the current Verilator model. | Commit `9f1bb96` records `PROJECT EXECUTION SUCCESSFUL` and measured cadence lines for all three rates; default tickless and periodic variants also pass. | These are finite executions with one initial state and scheduling history. They do not expose which divider/reset event produced observed `TIME` values. | **Accept as empirical integration evidence only.** Keep the logs and cadence test, but do not infer divider ownership from PASS. | High |
| Three changed coherent `TIME` samples prove takeover by the newly programmed divider. | Commit `5f9bf7e` assumes one pre-write payload and one old-divider expiration can cross, then accepts three changes. Native helper tests show that the sample-counting algorithm is bounded and behaves as coded. | The prescaler is separate destination-domain storage and a divider write does not reload it. `BlindTransfer` may reject/coalesce a reset while blinded. `BusSynchronizer` may expose in-flight values, skip intermediate values, or repeat values. Pause, resume-load, or reset can also cause changes. Distinct values establish progress, not cadence: the same values can arise under old and new divider periods. | **Reject as a proof of ownership.** The helper proves only that three coherent, pairwise changed samples were returned. It neither identifies their cause nor establishes the first interval under the selected divider. | High |
| A divider write followed by one reset request is enough. | MMIO fences order source-domain writes, and successful runs progress afterward. | A fence does not acknowledge the destination-domain divider load or reset. `BlindTransfer` explicitly blinds later source pulses until destination receipt returns; no software-visible acknowledgment exists. | **Reject.** Software must establish a post-divider reset epoch through observations whose interpretation follows from RTL bounds. | High |
| The original two-reset nonzero-to-zero sequence proves a new epoch. | Divider write precedes both pulses; observing coherent nonzero lets the first transfer complete and clears `BlindTransfer` before the second request; observing coherent zero after that request cannot be the prior nonzero sample. | Zero is transient and can be missed if the divider period is too short relative to `BusSynchronizer`; an unbounded poll would still not repair that. | **Accept only inside the envelope below.** Its period constraints make reset-zero residence observably longer than the synchronization bound. | High |
| Any exactly divisible nonzero rates are supportable. | The generalized driver checks nonzero values, ordering, exact division, and 32-bit reload fit. | Arithmetic validity does not imply observable reset, bounded takeover, or enough deadline margin for CDC. | **Reject.** Exact arithmetic checks are necessary but not sufficient; all envelope inequalities are mandatory. | High |
| 10 MHz is supported because it passes Verilator. | The 10 MHz tickless run at 10 kHz ticks passes and reports plausible cadence. | At 350 MHz, `P = 35`, far below the required 280-cycle reset-zero period. A passing trajectory does not prove all legal CDC phases can observe zero. | **Reject for production support.** Retain it as a negative too-fast configuration test. | High |
| Standard Zephyr controls should describe timer policy. | Commit `5f9bf7e` selects the chosen timer and derives divider/tick arithmetic from standard DT/Kconfig inputs. | None, provided hardware-specific safety bounds remain driver diagnostics rather than public tuning knobs. | **Accept.** Use fixed DT input rate, `zephyr,system-timer`, `CONFIG_SYS_CLOCK_HW_CYCLES_PER_SEC`, `CONFIG_SYS_CLOCK_TICKS_PER_SEC`, and `CONFIG_TICKLESS_KERNEL`; prohibit runtime rate updates. | High |
| Runtime rate updates can be layered on later without RTL work. | Zephyr has a runtime hardware-cycle-rate option and a future clock provider could report rates. | A rate change alters the live interval and Zephyr accounting while divider storage, prescaler reset, and epoch transfer have no atomic acknowledgment. | **Reject.** Require an RTL atomic divider+reset commit acknowledgment/generation or a coordinated clock-provider transition protocol. | High |

## Evidence is not proof

The Verilator evidence from `9f1bb96` is valuable and remains accepted:

| Hardware-cycle rate | Tick rate | Mode evidence | Result |
|---:|---:|---|---|
| 100 kHz | 100 Hz | periodic | PASS |
| 1 MHz | 1 kHz | tickless and periodic | PASS |
| 10 MHz | 10 kHz | tickless | PASS |

Those runs establish that each tested image booted, advanced cycles near its
configured rate, honored the exercised sleep, and delivered timer-driven
preemption in that simulation. They do **not** prove divider/reset ownership.
That proof must account for the RTL state and CDC mechanisms:

- `CLOCKS_PER_TICK` is storage distinct from the live prescaler. The prescaler
  loads it only on reset, resume-load, or expiration.
- `CONTROL.RESET` crosses through `BlindTransfer`, which accepts a source pulse
  only while not blinded and clears that state only after the destination pulse
  returns through its acknowledgment synchronizer.
- `TIME` crosses through a recurring `BusSynchronizer` handshake. Software may
  see an in-flight old payload, repeated payloads, or a later payload while
  intermediate counter values are skipped.
- `TIME` can change or stop because of ordinary expiration, reset, pause, and
  resume-load. Three distinct numbers do not identify one of those causes or
  measure the spacing between destination increments.

Accordingly, “three changed coherent observations” is a progress predicate,
not a cadence or ownership predicate.

## Accepted interface

The supported public contract is:

- the devicetree `clock-frequency` is a fixed, nonzero ticktimer input clock;
- the board selects the enabled Baochip timer through `zephyr,system-timer`;
- `CONFIG_SYS_CLOCK_HW_CYCLES_PER_SEC` selects the nonzero `TIME` rate;
- `CONFIG_SYS_CLOCK_TICKS_PER_SEC` selects the nonzero kernel tick rate;
- `CONFIG_TICKLESS_KERNEL` selects tickless or periodic kernel operation;
- input/HW and HW/ticks divisions must both be exact;
- `P - 1`, where `P = input / HW`, must fit the 32-bit reload register;
- the safety-envelope inequalities below must hold with exact, actionable
  build diagnostics; and
- `CONFIG_SYSTEM_CLOCK_HW_CYCLES_PER_SEC_RUNTIME_UPDATE` is unsupported.

No timer or clock API may change the input or derived rate at runtime.

## Conservative software-only envelope

Let:

```text
input = fixed ticktimer input frequency
HW    = CONFIG_SYS_CLOCK_HW_CYCLES_PER_SEC
ticks = CONFIG_SYS_CLOCK_TICKS_PER_SEC
P     = input / HW                  input clocks per TIME increment
C     = HW / ticks                  TIME increments per kernel tick
reload = P - 1
```

All operands are nonzero, both divisions are exact, and `reload <= UINT32_MAX`.
In addition, accept a configuration only when:

```text
P >= 280
P + 286 <= 4096
C * P >= P + 140
```

The constants are intentionally conservative:

- **140 input cycles** is the adopted upper visibility bound for a new
  `BusSynchronizer` payload: its 128-cycle retry timeout plus synchronizer and
  registered request/data/response pipeline allowance. It is a hardware-cycle
  bound, not a count of CPU polling-loop iterations.
- **280 cycles** requires the reset-zero residence `P` to contain the full
  140-cycle visibility bound plus one additional full 140-cycle slack window.
  This avoids treating equality with the CDC bound as safe.
- **286 cycles** is two 140-cycle CDC allowances (reset
  acceptance/acknowledgment and `TIME` visibility) plus six registered-edge
  allowances around pulse issue, destination action, and source observation.
  The takeover budget is conservatively represented as `P + 286`; capping it
  at **4096** keeps initialization failure bounded and rejects rates so slow
  that the selected software budget no longer covers the proof. This numerical
  poll budget is a liveness limit, not the reason reset zero is observable; the
  independent `P >= 280` condition supplies that proof.
- **`C * P >= P + 140`** requires one kernel tick to contain one complete
  divider period plus the visibility allowance. This is the minimum accepted
  alarm/tick margin; notably `C = 1` always fails.

These are sufficient software support conditions, not claims that all excluded
rates are physically broken or that the bounds are maximally permissive.

For the current 350 MHz input, exact-divisor hardware rates are supported from
roughly 100 kHz through 1.25 MHz. The intended matrix is:

| Role | HW rate | `P` | Envelope result |
|---|---:|---:|---|
| Lower supported test | 100 kHz | 3500 | Passes: `3500 + 286 = 3786` |
| Default | 1 MHz | 350 | Passes with 70-cycle slack above `P >= 280` |
| Upper boundary | 1.25 MHz | 280 | Passes the reset-visibility boundary exactly |
| Too fast | 10 MHz | 35 | Rejected by `P >= 280`, despite empirical PASS |
| Too slow | 70 kHz | 5000 | Rejected by `P + 286 <= 4096` |

The tick rate must independently satisfy exact division and the margin. For
example, selecting `ticks = HW` gives `C = 1` and fails because `P` cannot be at
least `P + 140`.

## Accepted takeover sequence

Restore the bounded two-reset sequence:

```text
EV_ENABLE = 0
full data-synchronization fence
CLOCKS_PER_TICK = P - 1
full data-synchronization fence

CONTROL = RESET                     # first request
full data-synchronization fence
poll bounded coherent TIME until TIME != 0
fail initialization if the nonzero phase exhausts its budget

CONTROL = RESET                     # second request
full data-synchronization fence
poll bounded coherent TIME until TIME == 0
fail initialization if the zero phase exhausts its budget

CONTROL = 0
full data-synchronization fence
last_count = 0
full data-synchronization fence
```

The coherent nonzero observation is essential. It demonstrates destination
progress after the first request and allows the `BlindTransfer` acknowledgment
to return and clear its blind state before software emits the second pulse. The
second phase starts after a known nonzero CPU-domain sample, so its coherent
zero cannot be the same stale sample. RTL reset priority then establishes
`TIME = 0` and reloads the live prescaler from the already ordered divider
storage. Because reset leaves zero resident for `P` input cycles and `P >= 280`,
the 140-cycle visibility bound has one additional complete bound of slack in
which to expose that zero. Only then are `CONTROL = 0` and baseline zero valid.

Both observation loops must fail with bounded, phase-specific diagnostics.
Their iteration accounting must be derived from the accepted `P + 286` budget;
no million-iteration loop or unbounded wait is part of the proof.

## Long-term unrestricted support

Higher rates, arbitrary rates, and runtime changes require a stronger hardware
or provider contract. Accept either:

- an RTL atomic divider-plus-reset commit with a software-visible acknowledgment
  or monotonically changing generation that identifies the applied settings;
  or
- a coordinated clock-provider protocol that quiesces alarms, changes the
  input, commits and acknowledges divider/reset ownership, establishes a fresh
  epoch, updates Zephyr's timekeeping rate coherently, and then resumes alarms.

Sampling ordinary `TIME` values cannot substitute for either contract.

## Migration from `5f9bf7e` and `9f1bb96`

1. Preserve `zephyr,system-timer` selection, fixed `clock-frequency`, standard
   Kconfig controls, runtime-update rejection, nonzero checks, exact-divisibility
   checks, reload-fit validation, and existing bounded counter/alarm helpers.
2. Remove `baochip_ticktimer_observe_takeover()`, its three-change native tests,
   and the `DIVIDER_PERIOD * 7 + 32` takeover rationale.
3. Restore two phase-specific bounded helpers or loops: coherent nonzero after
   the first reset, then coherent zero after the second reset. Restore
   `last_count = 0` and first target `CYC_PER_TICK`.
4. Add compile-time diagnostics for `P >= 280`, `P + 286 <= 4096`, and
   `C * P >= P + 140`, with guarded arithmetic so invalid zero/inexact inputs
   still produce their intended primary diagnostic.
5. Keep the runtime cadence/preemption test structure, replace the supported
   10 MHz scenario with 1.25 MHz, and move 10 MHz into pristine negative builds.
6. Update board and test documentation to state the conservative envelope, not
   “any exact division,” while retaining 1 MHz as the default.

## Evidence and test plan

| Case | Configuration at 350 MHz input | Required result |
|---|---|---|
| Lower runtime | HW 100 kHz, ticks 100 Hz, periodic | Build; takeover, cadence, sleep, and preemption PASS |
| Default runtime | HW 1 MHz, ticks 1 kHz, tickless and periodic | Both modes PASS |
| Upper runtime | HW 1.25 MHz, ticks 1.25 kHz | Build and runtime PASS at `P = 280`, `C = 1000` |
| Too fast | HW 10 MHz | Build fails on `P >= 280`; retain prior Verilator PASS as empirical excluded-rate evidence |
| Too slow / budget | HW 70 kHz | Build fails on `P + 286 <= 4096` |
| Tick margin | ticks equal HW (`C = 1`) | Build fails on `C * P >= P + 140` |
| Inexact input/HW | for example HW 3 MHz | Build fails with exact input/HW divisibility diagnostic |
| Inexact HW/ticks | exact input/HW but non-dividing tick rate | Build fails with exact HW/ticks divisibility diagnostic |
| Zero | zero input, HW, and ticks cases as independently reachable | Each fails without divide-by-zero fallout and with the primary nonzero diagnostic |
| Reload overflow | exact ratio with `P - 1 > UINT32_MAX` in a compile fixture | Build fails with reload-fit diagnostic |
| Runtime update | enable `CONFIG_SYSTEM_CLOCK_HW_CYCLES_PER_SEC_RUNTIME_UPDATE` | Build fails with unsupported-runtime-update diagnostic |
| Takeover failures | scripted coherent-read fixtures never produce nonzero, or never expose zero | Each phase stops at its bound and returns `-ETIMEDOUT` |

Runtime logs must report selected input, HW, ticks, `P`, `C`, measured cadence,
and completion. Native tests validate arithmetic and loop exhaustion; Verilator
validates the integrated model; hardware remains a separate evidence column.

## Documentation process correction

The generalized implementation was treated as resolving configurable rates
before the new takeover argument had been accepted. That reversed the required
order: a passing implementation supplied evidence for behavior, then its
unreviewed proof premise was reported as settled design.

The rule going forward is: **empirical PASS does not supersede an unresolved
proof obligation.** Documents and issue closure must label simulation evidence,
RTL-derived invariants, assumptions, and accepted decisions separately. A new
algorithm that changes an ownership proof remains provisional until its causes,
CDC state, bounds, and counterexamples are adjudicated, even when every current
test passes.

## Primary references

- [`bao_core.py:1793-1853`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/verilate/bao_core.py#L1793-L1853) defines the separate prescaler, pause/load paths, `TIME` `BusSynchronizer`, reset `BlindTransfer`, reset priority, and reload/increment behavior.
- [`cdc.py:100-138`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/rtl/scripts/headergen/migen/genlib/cdc.py#L100-L138) defines recurring bus transfer, its 128-cycle timeout, buffering, and request/response path.
- [`cdc.py:140-184`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/rtl/scripts/headergen/migen/genlib/cdc.py#L140-L184) defines pulse blinding and acknowledgment.
- [`bao_core.py:1876-1914`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/verilate/bao_core.py#L1876-L1914) records alarm transfer/lockout behavior and the target synchronizer.
- Local Zephyr commit `5f9bf7e519c9`, `drivers/timer/baochip_ticktimer.c:23-110,235-264`, is the current standard-interface implementation and generalized takeover to revise.
- Local Zephyr commit `5f9bf7e519c9`, `drivers/timer/baochip_ticktimer.h:30-71`, is the rejected three-changed-sample helper.
- Local Zephyr commit `9f1bb96cb066`, `tests/drivers/timer/baochip_ticktimer/{README.md,testcase.yaml,src/main.c}`, records the 100 kHz/1 MHz/10 MHz matrix and cadence test.
- Local Zephyr `boards/baochip/dabao/dabao.dts:12-17` selects the timer; `dts/riscv/baochip/bao1x.dtsi:77-83` supplies its 350 MHz fixed input.

## Cross-references

- [`07-ticktimer-sysclock.md`](07-ticktimer-sysclock.md) supplies the accepted two-reset epoch proof, bounded coherent reads, alarm sequencing, and original 1 MHz evidence; this adjudication generalizes only its rate envelope.
- [`08-device-creation-reform.md`](08-device-creation-reform.md) establishes fixed-rate clock-provider ownership and the distinction between a synchronization visibility proof and a software poll bound; this adjudication replaces its fixed-1-MHz limitation with explicit conservative inequalities.
- [`06-irq-ack-semantics.md`](06-irq-ack-semantics.md) defines the direct interrupt path used after takeover and separates it from irqarray acknowledgment semantics.
- [`05-lifecycle-delivery-validation.md`](05-lifecycle-delivery-validation.md) explains why observed simulator or hardware handoff evidence must remain distinct from inferred ownership.
- [`04-synthesis.md`](04-synthesis.md) is the original milestone decision selecting the ticktimer as the system clock.
