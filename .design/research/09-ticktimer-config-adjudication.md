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
  - id: baochip-takeover-prototype
    resource: file:/tmp/opencode/baochip-timer-takeover-prototype/RESULTS.md
    title: Throwaway takeover prototype measurements on Zephyr tip ce6cae8e5c19
---

# Baochip ticktimer configuration adjudication

## Decision status and scope

**Accepted, superseding the generalized takeover and rate matrix in local
Zephyr commits `5f9bf7e519c9` and `9f1bb96cb066`.** Preserve their standard
configuration interface, chosen-node selection, exact arithmetic diagnostics,
and reusable tests. Replace their three-distinct-sample takeover with the
two-reset sequence below and limit configurations to the conservative
software-only envelope defined here.

> **Revision, 2026-08-19:** the two-reset observe-zero takeover accepted
> below is falsified by measured prototype evidence; the accepted proof is
> now the cadence measurement, and the envelope inequalities are re-derived.
> See the
> [Addendum 2026-08-19](#addendum-2026-08-19-cadence-proof-replaces-the-falsified-observe-zero-takeover)
> at the end of this document. The public interface is unchanged.

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

- [`07-ticktimer-sysclock.md`](07-ticktimer-sysclock.md) supplies bounded coherent reads, alarm sequencing, and the original 1 MHz evidence; its two-reset epoch proof is falsified by the 2026-08-19 addendum, which supersedes that takeover with the cadence measurement.
- [`08-device-creation-reform.md`](08-device-creation-reform.md) establishes fixed-rate clock-provider ownership and the distinction between a synchronization visibility proof and a software poll bound; this adjudication replaces its fixed-1-MHz limitation with explicit conservative inequalities.
- [`06-irq-ack-semantics.md`](06-irq-ack-semantics.md) defines the direct interrupt path used after takeover and separates it from irqarray acknowledgment semantics.
- [`05-lifecycle-delivery-validation.md`](05-lifecycle-delivery-validation.md) explains why observed simulator or hardware handoff evidence must remain distinct from inferred ownership.
- [`04-synthesis.md`](04-synthesis.md) is the original milestone decision selecting the ticktimer as the system clock.

# Addendum 2026-08-19: cadence proof replaces the falsified observe-zero takeover

## Why this revision exists

The takeover accepted above — two reset requests separated by a coherent
nonzero observation and completed by a coherent zero observation — has been
tested against the integrated RTL model and falsified. A throwaway Zephyr
prototype (isolated copy of Zephyr tip `ce6cae8e5c19`, prepared `Vsim`
SHA-256 `c24080fe64b663feaa15ab2afa547886aeeaa938250d3b6f8088267f16a443d2`,
three deterministic reruns per configuration) never observed reset zero
under any deadline, `CONTROL` write sequence, or inter-write delay it tried.
The same prototype then implemented a cadence-based takeover and measured it
at all three supported rates; that proof is accepted here in the observe-zero
proof's place. The public interface and the conservative rate envelope are
unchanged. The envelope inequalities are re-derived below; one is retired as
an operative constraint.

## Claims and adjudication

| Claim | Evidence | Counterargument | Adjudication | Confidence |
|---|---|---|---|---|
| The two-reset nonzero-to-zero sequence proves a new epoch (accepted in the table above). | Variants A–D kept the accepted sequence and varied only deadlines, shared and separate, up to 524,288 mcycles per phase: the first reset yields coherent `TIME=1` promptly (~238–262 mcycles, one poll, three `TIME` reads), but zero is never observed — variant D exhausted 4,294 coherent polls, 12,882 `TIME` reads, and 523,889 mcycles with `TIME` advanced to 1,497. Variants E–I (`CONTROL=0` re-arm before pulsing, inter-write waits through 131,072 mcycles, explicit `0`/`RESET`/`0` triplets, distinct-value priming) all timed out at coherent `TIME=187` after 65,181 mcycles and 543 polls. | Deadline starvation and write-strobe gating are excluded: the RTL strobes `CONTROL` on every write regardless of stored value, `reset_xfer_blind` suppresses a request only until its acknowledgment returns, and the post-request trajectory is invariant across a 16x delay range — consistent with accepted resets followed by resumed counting. | **Reject — falsified.** Reset zeroes the timer-domain counter only transiently; software reads a separately synchronized snapshot whose free-running refresh skips intermediate values, so the transient zero is not software-visible at any `P`. The original `P >= 280` visibility rationale is wrong, not merely tight. | High |
| A bounded cadence measurement proves divider-rate ownership and establishes the driver baseline. | Measured mcycles per `TIME` increment: 349.69 vs 350 (−0.089%), 3492.8 vs 3500 (−0.206%), 279.00 vs 280 (−0.357%) at 1 MHz, 100 kHz, and 1.25 MHz; zero run-to-run spread across three deterministic reruns per configuration; deadline completed at 48.9–49.5% of budget in every configuration; first alarm punctual (below). | Cadence certifies the stored reload and therefore the rate; it cannot certify the absolute epoch of `TIME` — whether the counter was ever zeroed is unobservable through the snapshot synchronizer. | **Accept.** The epoch is unobservable and unneeded: the driver consumes only deltas. The baseline is the final measured `TIME`; alarm targets and elapsed/announce arithmetic are differences from that baseline. Rate ownership plus a relative baseline is exactly the contract the system-clock driver consumes. | High |
| `mcycle` is a valid elapsed-time reference for takeover deadlines and test assertions. | Empirically `TIME` advanced 41,267 while `mcycle` advanced 105,793 across a 40 ms sleep: the RTL gates the core clock during WFI and `CsrPlugin_mcycle` counts only core clocks, while the timer domain never gates. | None, provided the reference interval busy-polls, which the takeover does. | **Accept as a platform constraint, not a takeover claim.** Any `mcycle`-based assertion must use a busy-poll reference, never sleep-spanning deltas. | High |
| The envelope inequalities above remain operative unchanged. | All three supported rates pass; 10 MHz remains excluded. | Their reset-visibility rationale is falsified: `P >= 280` no longer purchases an observable zero, and `P + 286 <= 4096` no longer bounds a two-phase reset poll that cannot succeed. | **Split — re-derived below.** `P >= 280` survives with a new rationale; `P + 286 <= 4096` is retired as an operative constraint while the slow-rate envelope edge is retained conservatively; `C * P >= P + 140` survives for alarm margin. | High |

## Falsification evidence

| Group | What varied | Nonzero phase | Zero phase |
|---|---|---|---|
| A–D | shared vs separate deadlines, 636 (`P + 286`) to 524,288 mcycles per phase | coherent `TIME=1` on the first poll, 238–262 mcycles, three `TIME` reads | never zero; variant D: 4,294 coherent polls, 12,882 reads, 523,889 mcycles, `TIME` = 1,497 |
| E–I | `CONTROL=0` re-arm, waits through 131,072 mcycles, explicit `0`/`RESET`/`0` pulsing, distinct-value priming (artifacts `S1`–`S5`, `W8192`–`W131072`) | coherent `TIME=1` (or `TIME=3`) on the first poll | never zero; 543 coherent polls, 1,629 reads, 65,181 mcycles, `TIME` = 187, invariant across all variants |

Long zero-phase polling cost about 122.0 mcycles per coherent poll and 40.7
mcycles per `TIME` MMIO read, so the loops sampled several times per
synchronizer refresh period and still never sampled the transient. The root
cause is observability, not deadlines or write strobes:

- `cram_axi.sv:16185-16194` decodes the `CONTROL` write and strobes on every
  write, with no comparison against the stored value; `16268-16274` turns the
  strobe plus stored bit into the one-cycle `ticktimer_reset` request.
- `cram_axi.sv:12681-12685` admits a request only while `reset_xfer_blind`
  is clear; `19982-19991` sets blind on request and clears it on the returned
  acknowledgment (`19914-19916`). Pending requests are suppressed until
  acknowledged, not lost.
- `cram_axi.sv:19864-19883` shows an accepted request resetting
  `ticktimer_timer0`, reloading the prescaler, and immediately resuming
  counting: the timer-domain zero lasts less than one divider period.
- `cram_axi.sv:12645-12649` exposes the separately synchronized
  `ticktimer_timer_sync_o` as `TIME` rather than `timer0`; the snapshot is
  refreshed by a free-running ping-pong handshake with a 128-count cadence
  (`12669-12674`, `19891-19906`, `19961`) that drops intermediate values —
  including the transient reset zero.

Therefore no poll budget, write spacing, or `CONTROL` value sequence can make
reset zero software-visible on this RTL. The two-reset contract's zero phase
is unsatisfiable, which falsifies the sequence as a whole even though its
nonzero phase and its fences remain sound.

## Accepted takeover: cadence measurement

```text
EV_ENABLE = 0
full data-synchronization fence
CLOCKS_PER_TICK = P - 1
full data-synchronization fence
CONTROL = RESET                     # single request; blind is clear at issue
full data-synchronization fence

read coherent high-low-high TIME samples, each stamped with mcycle
discard one anchor sample plus two changed observations
    (one possible stale synchronized payload, one old-divider expiration)
mark start TIME and start mcycle
sample until the unsigned 64-bit TIME delta is at least 64 increments
    post-loop sanity: TIME delta <= 128, else fail with -EILSEQ
expected mcycle delta = TIME_delta * CPU_CLOCK_HZ
                         / CONFIG_SYS_CLOCK_HW_CYCLES_PER_SEC
accept when |measured - expected| is within tolerance
baseline last_count = final TIME
program the first alarm at last_count + CYC_PER_TICK
```

The single wait is bounded by one mcycle deadline taken before the reset
write: `2 * window + 128 + (P + 286)` mcycles, where
`window = 64 * CPU_CLOCK_HZ / CONFIG_SYS_CLOCK_HW_CYCLES_PER_SEC`. The
128-mcycle term covers the measured ~122-mcycle coherent poll; `P + 286` is
the retained CDC allowance for reset-request acknowledgment and synchronized
`TIME` refresh. All arithmetic is 64-bit with compile-time asserted products;
mcycle deltas use 32-bit unsigned subtraction, so a 32-bit `mcycle` wrap is
safe.

Measured results (three deterministic reruns per configuration,
bit-identical telemetry):

| Configuration | `P` | deadline (mcycles) | ratio measured vs expected | error | polls | deadline used |
|---|---:|---:|---:|---:|---:|---:|
| 1 MHz / 1 kHz | 350 | 45,564 | 349.69 vs 350 | −0.089% | 89 | 48.9–49.5% |
| 100 kHz / 100 Hz | 3500 | 451,914 | 3492.8 vs 3500 | −0.206% | 879 | 48.9–49.5% |
| 1.25 MHz / 1.25 kHz | 280 | 36,534 | 279.00 vs 280 | −0.357% | 72 | 48.9–49.5% |

- Run-to-run spread is zero in every configuration; rebuilt 1 percent
  tolerance confirmations also passed (`K1n`–`K3n`), with errors −0.464%,
  −0.178%, and +0.089%.
- The build-to-build envelope is −0.464%…+0.089%, consistent with one
  synchronizer refresh (~300 mcycles) plus one poll (~254 mcycles) over the
  17,920–224,000-mcycle windows: endpoint quantization, not period error,
  dominates.
- Production tolerance must cover that envelope with margin: 5 percent is
  comfortable; 1 percent is already build-phase sensitive at `P = 280`.
- The required discard count was exactly two changed observations in every
  run. The ~254-mcycle poll cadence (three MMIO reads plus loop and deadline
  checks) stayed below `P`, so no increment was skipped between polls and
  every window closed with `TIME` delta exactly 64.
- Takeover completed at 48.9–49.5 percent of the deadline (about 2.03x
  margin), leaving the second half of the budget for a slow inherited
  expiration or acknowledgment delay.
- First-alarm punctuality: the 40 ms test sleep ended at 41.3 ms (1 MHz) and
  40.9 ms (1.25 MHz); the 100 kHz case returned after five 10 ms tick
  boundaries instead of four — a tickless announce-alignment artifact of the
  missed init alarm at that rate, not a cadence error.

## RTL ownership argument

- The prescaler reloads from live register storage at expiry:
  `ticktimer_clkspertick = ticktimer_clocks_per_tick_storage`
  (`cram_axi.sv:12663`) with reload-on-zero at `19871-19874`. A
  `CLOCKS_PER_TICK` write updates storage immediately and never reloads the
  prescaler (the only immediate reload is the suspend/resume LOAD request,
  `5598` and `16153-16155`), so the inherited count expires at most once
  under the old reload — the hardware-reset default is 800,000 input cycles
  (`20676`) — and that single expiration is exactly what the discard rule
  removes.
- After the discards, the 64-increment window spans full periods of whatever
  reload value is stored. A wrong stored reload `P'` appears as a ratio of
  `P'/P`; the measured ratios certify the stored reload within tolerance,
  and the stored reload is what software wrote. The new divider therefore
  owns the counter's rate.
- A single reset request issued with `reset_xfer_blind` clear cannot be
  coalesced (`cram_axi.sv:16268-16274`, `12681-12685`, `19982-19991`); it
  zeroes `timer0` and reloads the prescaler in the timer domain
  (`19860-19883`). Coalescing requires a second request before the
  acknowledgment returns, which this sequence never issues.
- A lost or delayed request cannot corrupt the measurement: the divider
  write alone retimes the counter from the next expiration, so the cadence
  proof survives a lost reset; the loss only postpones the first increment,
  which the deadline detects and fails safe.
- What cadence cannot prove is the absolute epoch of `TIME`. It does not
  need to: `TIME` is consumed only through deltas (baseline, alarm targets,
  and elapsed/announce arithmetic are all differences). The snapshot
  refresh's skip behavior is precisely why the epoch is unobservable — the
  same fact that falsified observe-zero.
- The `mcycle` reference is valid because the measurement busy-polls: the
  core clock cannot gate without WFI, while the `TIME` domain never gates.

## Platform constraint: WFI gates the core clock

- `vexsys.sv:124` gates the VexRiscv core clock through an ICG when
  `wfi_active`; the sleep request is `cram_axi.sv:5213`
  (`wfi & cpu_int_active & ~axi_active & active_timeout==0`), with
  `cpu_int_active` true only while no interrupt is pending
  (`cram_axi.sv:16663`). `CsrPlugin_mcycle` increments only on core clocks
  (`VexRiscv_CramSoC.sv:7857`). The ticktimer lives in the always-on domain.
- Empirically, across a 40 ms sleep, `TIME` advanced 41,267 while `mcycle`
  advanced 105,793 — roughly 99.2 percent of the sleep was clock-gated.
- Consequence for tests and drivers: any `mcycle`-based assertion must use a
  busy-poll reference, never a sleep-spanning delta. The standard timer
  test's independent-reference assertion (`main.c:91`) compares
  `k_cycle_get_64()` progress against `mcycle` across `k_sleep()` and cannot
  pass under this gating; fixing it is a test-side or platform-side change,
  not a takeover change. No takeover acceptance may be gated on
  `PROJECT EXECUTION SUCCESSFUL` until that assertion is repaired.

## Envelope re-derivation: which inequalities survive

The original constants remain RTL facts where cited, but their roles change:

- **`P >= 280` — survives, new rationale.** Its reset-zero-visibility
  rationale is falsified: zero is not observable at any `P`. It survives as
  the conservative boundary of the measured cadence evidence: the measured
  poll cost (~122 mcycles per coherent poll uninstrumented; ~254 mcycles
  with mcycle stamping and deadline checks) must stay below one increment
  period `P` so increments are not skipped between polls and the discard
  accounting stays exact, and every cadence measurement, tolerance, and
  margin recorded above was taken inside it. At `P = 280` only ~26 cycles —
  about one tenth of a poll — separate the cadence from the boundary, which
  is why the 1 percent rebuild moved +0.089% there.
- **`P + 286 <= 4096` — retired as an operative constraint.** It bounded a
  two-phase reset-observation poll that cannot succeed. The cadence deadline
  `2 * window + 128 + (P + 286)` mcycles is self-bounding — linear in `P` —
  and completed at no more than 49.5 percent of budget at every supported
  rate. The `P + 286` term survives inside that deadline as the CDC
  allowance for reset acknowledgment and synchronized `TIME` refresh. The
  ~70 kHz slow-rate rejection is retained only because no runtime evidence
  exists below 100 kHz; widening downward is a new decision, not an
  entitlement of the cadence proof.
- **`C * P >= P + 140` — survives for alarm margin, re-derived.** It never
  depended on reset observability: it requires one kernel tick to contain
  one full divider period plus a CDC allowance so the first alarm target can
  commit, transfer, and be compared against a refreshed `TIME`. The
  falsification sharpened rather than removed this need: the measured CDC
  path costs (one `TIME` refresh ~300 mcycles; one coherent poll ~254
  mcycles) exceed the nominal 140-cycle payload-visibility allowance, so 140
  is a floor, not a sufficient margin. The operative alarm safety remains
  the two-tick absolute retry margin plus the three-commit bound with the
  expired-level fallback from [`07-ticktimer-sysclock.md`](07-ticktimer-sysclock.md);
  the measured first-alarm punctuality above is empirical support, not
  proof. `C = 1` still fails trivially.
- **10 MHz (`P = 35`) remains rejected** without invoking reset visibility:
  endpoint quantization alone is about (300 + 254) / (64 × 35) ≈ 25 percent,
  five times a 5 percent tolerance, and the ~254-mcycle poll cadence exceeds
  seven increment periods, so the loop could not even observe the individual
  changes the discard rule counts.

## Scope preserved, and explicit non-widening

- The public interface is unchanged: fixed DT `clock-frequency`,
  `zephyr,system-timer` selection, `CONFIG_SYS_CLOCK_HW_CYCLES_PER_SEC`,
  `CONFIG_SYS_CLOCK_TICKS_PER_SEC`, `CONFIG_TICKLESS_KERNEL`; exact
  divisibility, nonzero, and reload-fit checks; runtime updates unsupported.
- The supported envelope stays 100 kHz–1.25 MHz at the 350 MHz input. This
  addendum does not widen rates. The cadence proof's structure — rate
  certified by ratio, deadline linear in the window, epoch not required —
  may permit future widening (for example toward 10 MHz), but only through a
  new decision backed by new runtime evidence covering tolerance at smaller
  `P`, discard accounting with skipped increments, and alarm margin at the
  faster rate.
- The production implementation is still pending: this addendum adjudicates
  the proof, it does not ship the driver. The production takeover must
  replace both observe-zero loops with the cadence measurement above, keep
  the bounded diagnostics, and repair or replace the test's
  `mcycle`-across-sleep assertion before any acceptance is gated on
  `PROJECT EXECUTION SUCCESSFUL`.

## Process rule applied to this revision

The falsification is the standing rule — empirical PASS does not supersede an
unresolved proof obligation — operating in both directions. The prior
acceptance rested on an RTL argument (reset-zero residence `P` containing
two 140-cycle visibility bounds) that treated the `BusSynchronizer`
payload-visibility bound as if intermediate destination values were exposed;
the snapshot refresh skips them, and no measurement had been taken. This
revision therefore labels its own basis: the adjudication above rests on
prototype measurements from one deterministic simulator build (three reruns
per configuration, one configuration family) plus the RTL line-cited
arguments reproduced here. The production driver, its diagnostics, native
and Verilator tests, and hardware validation remain open obligations, and
the cadence PASS recorded here does not close them.

One reconciliation item is open: the fixed-rate `PROJECT EXECUTION
SUCCESSFUL` records preserved in `07-ticktimer-sysclock.md` predate the
prepared simulator build used here and are not explained by this prototype;
they must no longer be cited as zero-observability evidence, and tracing how
their zero phase completed (a different simulator build or timing) is an
open documentation task.

## References for this addendum

- Prototype results (throwaway, outside any repository):
  `/tmp/opencode/baochip-timer-takeover-prototype/RESULTS.md`, with artifacts
  under `A/`–`D/`, `S1/`–`S5/`, `W8192`–`W131072/`, `K1/`–`K3/`, and
  `K1n/`–`K3n/`; Zephyr tip `ce6cae8e5c19` isolated copy; prepared `Vsim`
  SHA-256 `c24080fe64b663feaa15ab2afa547886aeeaa938250d3b6f8088267f16a443d2`.
- [`cram_axi.sv`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/rtl/modules/vexriscv/rtl/cram_axi.sv) — `CONTROL` strobe (`16185-16194`), reset request (`16268-16274`), blind admission/acknowledgment (`12681-12685`, `19914-19916`, `19982-19991`), timer reset and reload (`19860-19883`), prescaler storage reload (`12663`, `19871-19874`), synchronized `TIME` exposure and refresh (`12645-12674`, `19891-19906`, `19961`), WFI request and gating (`5213`, `16663`), divider default (`20676`), suspend/resume load (`5598`, `16153-16155`).
- [`vexsys.sv`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/rtl/modules/core/rtl/vexsys.sv) — core-clock ICG during WFI (`124`).
- [`VexRiscv_CramSoC.sv`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/rtl/modules/vexriscv/lib/VexRiscv_CramSoC.sv) — `CsrPlugin_mcycle` increments on core clocks (`7857`).
- [`07-ticktimer-sysclock.md`](07-ticktimer-sysclock.md) — bounded coherent reads, alarm-commit retry and fallback, and the original (now falsified) two-reset text.
