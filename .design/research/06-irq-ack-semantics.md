---
type: Research
title: Baochip irqarray acknowledgment semantics
description: RTL-proven edge, level, mask, pending, acknowledgment, and race contract for the Zephyr interrupt controller.
tags: [baochip, irqarray, interrupts, rtl, xous, zephyr]
status: stable
generated: { by: agent:opencode, at: 2026-08-14 }
sources:
  - id: baochip-rtl
    resource: https://github.com/baochip/baochip-1x/tree/83b220f790e7e846a6500264b480b42ad9ebd40b
    title: Baochip 1x RTL
  - id: xous-core
    resource: https://github.com/betrusted-io/xous-core/tree/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b
    title: Xous core Baochip interrupt implementation
---

# Baochip irqarray acknowledgment semantics

This note resolves risk R3 from the port synthesis. The Baochip interrupt path
has two layers with different jobs:

1. Each `irqarrayN` converts 16 peripheral signals into sticky event bits and
   ORs enabled pending bits onto CPU external interrupt line N.
2. VexRiscv's ExternalInterruptArray masks and reports those 32 CPU lines. It
   has no claim, completion, or acknowledgment operation.

The conclusions below are from Baochip RTL revision
`83b220f790e7e846a6500264b480b42ad9ebd40b` and Xous revision
`5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b`.

## Register contract

The CPU plugin assigns machine mask/pending CSRs `0xBC0`/`0xFC0` and supervisor
mask/pending CSRs `0x9C0`/`0xDC0`
([`GenCramSoC.scala:184-189`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/VexRiscv/GenCramSoC.scala#L184-L189)).
Despite the name "mask", a 1 enables a line: RTL computes the interrupt as
`mask & externalInterruptArray`
([`VexRiscv_CramSoC.v:6911-6914`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/VexRiscv/VexRiscv_CramSoC.v#L6911-L6914)),
and writes replace the mask register
([`VexRiscv_CramSoC.v:8311-8319`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/VexRiscv/VexRiscv_CramSoC.v#L8311-L8319)).
The pending CSRs are read-only views of that already-masked result
([`VexRiscv_CramSoC.v:7430-7455`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/VexRiscv/VexRiscv_CramSoC.v#L7430-L7455)).
Consequently, MIP/SIP has no W1C or EOI meaning.

Xous independently confirms the polarity: enabling ORs a bit into SIM and
disabling clears it
([`kernel/src/arch/riscv/irq.rs:98-121`](https://github.com/betrusted-io/xous-core/blob/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b/kernel/src/arch/riscv/irq.rs#L98-L121)).
Its trap path reads `SIP & SIM`, globally disables external lines while a
userspace ISR runs, and restores SIM on ISR return
([`irq.rs:283-305`](https://github.com/betrusted-io/xous-core/blob/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b/kernel/src/arch/riscv/irq.rs#L283-L305),
[`irq.rs:131-154`](https://github.com/betrusted-io/xous-core/blob/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b/kernel/src/arch/riscv/irq.rs#L131-L154)).
The extra `& SIM` is harmless but redundant because SIP is already masked in
hardware.

Within a bank, `EV_ENABLE` also uses positive polarity. The bank output is the
OR of `EV_PENDING[i] & EV_ENABLE[i]`
([`cram_axi.sv:6013`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/rtl/modules/vexriscv/rtl/cram_axi.sv#L6013)).
`EV_PENDING` is W1C: a write strobe and each written 1 become the per-bit clear
signal
([`cram_axi.sv:5884-5900`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/rtl/modules/vexriscv/rtl/cram_axi.sv#L5884-L5900)).
Reset selects level mode, falling polarity (irrelevant in level mode), and all
events disabled
([`cram_axi.sv:22425-22435`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/rtl/modules/vexriscv/rtl/cram_axi.sv#L22425-L22435)).

## Edge and level behavior

`EV_EDGE_TRIGGERED[i] = 0` selects the raw hardware level. A high input is the
filtered trigger. Setting the bit selects edge detection; `EV_POLARITY[i] = 1`
detects a rising edge and 0 detects a falling edge
([`bao_core.py:927-949`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/verilate/bao_core.py#L927-L949),
[`cram_axi.sv:6014-6026`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/rtl/modules/vexriscv/rtl/cram_axi.sv#L6014-L6026)).
`EV_STATUS` reports the raw hardware input OR the software trigger, not the
filtered edge and not pending.

Every mode feeds the same sticky, one-bit pending latch. Trigger has priority
over clear:

```text
if filtered_trigger || soft_trigger: pending = 1
else if W1C:                       pending = 0
else:                              pending unchanged
```

This is explicit in the source generator
([`bao_core.py:950-959`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/verilate/bao_core.py#L950-L959))
and generated RTL
([`cram_axi.sv:16662-16673`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/rtl/modules/vexriscv/rtl/cram_axi.sv#L16662-L16673)).
The irqarray's own documentation states the resulting rule: a level source must
be cleared before pending, otherwise it immediately remains/reasserts pending
([`bao_core.py:973-985`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/verilate/bao_core.py#L973-L985)).

There is no event counter. Two edges before the first pending bit is cleared
coalesce into one observation. Trigger priority does guarantee that an edge in
the same always-on clock as W1C survives. An edge after W1C also sets the now
empty latch. Software can therefore minimize, but cannot eliminate, coalescing.

`EV_SOFT` participates as a level-like trigger. Software must first write it to
zero and then W1C pending; otherwise trigger priority keeps pending set. The RTL
documents this explicitly
([`bao_core.py:995-1006`](https://github.com/baochip/baochip-1x/blob/83b220f790e7e846a6500264b480b42ad9ebd40b/verilate/bao_core.py#L995-L1006)).

## What Xous does

The kernel dispatches CPU lines but deliberately does not acknowledge them;
the owning peripheral callback must clear its event source
([`kernel/src/irq.rs:20-50`](https://github.com/betrusted-io/xous-core/blob/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b/kernel/src/irq.rs#L20-L50)).
Most Baochip callbacks snapshot a bank's pending register and W1C exactly that
snapshot before doing deferred work, for example IOX
([`bao1x-hal-service/src/main.rs:52-66`](https://github.com/betrusted-io/xous-core/blob/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b/services/bao1x-hal-service/src/main.rs#L52-L66))
and camera
([`bao-video/src/main.rs:181-193`](https://github.com/betrusted-io/xous-core/blob/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b/services/bao-video/src/main.rs#L181-L193)).
This is correct for pulse or configured-edge sources and shortens the interval
during which edges coalesce.

Xous explicitly configures persistent sources when needed. Keyboard AOINT is
rising-edge filtered
([`keyboard.rs:288-296`](https://github.com/betrusted-io/xous-core/blob/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b/services/bao1x-hal-service/src/servers/keyboard.rs#L288-L296));
mailbox bits 1-3 are also rising-edge filtered
([`bao1x-mbox2/src/main.rs:188-202`](https://github.com/betrusted-io/xous-core/blob/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b/services/bao1x-mbox2/src/main.rs#L188-L202)).

USB intentionally uses level mode
([`usb-bao1x/src/hw.rs:145-157`](https://github.com/betrusted-io/xous-core/blob/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b/services/usb-bao1x/src/hw.rs#L145-L157)).
Its callback W1Cs irqarray pending before clearing the Corigine status
([`hw.rs:259-281`](https://github.com/betrusted-io/xous-core/blob/5d5bbbfa95c0dcef26fe1fe9b496b7f6f31d191b/services/usb-bao1x/src/hw.rs#L259-L281)).
Because trigger wins, that early W1C cannot clear a still-high USB source. Xous
may consequently take a cleanup interrupt after the source is deasserted. This
is safe but is not the lowest-reentry sequence to copy into Zephyr.

## Zephyr driver contract

Use flat event IRQs for irqarray children and direct CPU IRQs for ticktimer (20),
susres (21), mailbox (22/23), and timer0 (30). Maintain two enable layers:

- `EV_ENABLE[bit] = 1` enables a child event.
- MIM bit `bank = irq / 16` is 1 whenever at least one child in that bank is
  enabled. Direct CPU lines set their own MIM bit.

Initialization must mask MIM first, write every bank's `EV_ENABLE = 0` and
`EV_SOFT = 0`, program edge/polarity policy, W1C all stale pending bits, then
enable children and corresponding MIM lines. Never use read-modify-write on
`EV_PENDING`; write the exact bits being acknowledged.

The machine external trap handler should run with machine external interrupts
non-nested, read MIP, select CPU lines in a deterministic software order, and
drain until no enabled CPU line remains. For an irqarray line, snapshot
`pending = EV_PENDING & EV_ENABLE` and dispatch each set child according to its
configured mode:

```text
edge event:
    W1C this pending bit
    dispatch child ISR
    # no post-ISR W1C: it could erase an edge that arrived during the ISR

level event:
    dispatch child ISR             # ISR must deassert/clear peripheral source
    W1C this pending bit
    # if the source is still high, trigger priority leaves pending set
```

After the pass, reread `EV_PENDING & EV_ENABLE`. Continue draining if nonzero.
Direct CPU lines are acknowledged only at their peripheral event manager; MIP
itself is never written.

Two constraints must be explicit in the implementation and tests:

- A level-mode ISR must quiesce its peripheral before returning. If it does not,
  the pending latch correctly remains set and the drain loop can livelock.
- Edge mode provides at-least-one notification, not edge counting. Hardware
  edges that occur while pending is already one may coalesce. Drivers needing
  counts must drain a peripheral FIFO/status source rather than infer a count
  from irqarray pending.

Recommended assertions are that an event IRQ maps to bank `< 20` and bit `< 16`,
that direct CPU lines are not passed through bank MMIO, and that edge/polarity
configuration changes only while the child is disabled with stale pending
cleared. Verilator tests should cover same-cycle trigger+W1C (pending survives),
an edge arriving during its ISR (survives pre-ack), a level remaining asserted
through W1C (reasserts), two children sharing one bank, and simultaneous banks.

## Cross-references

- [`/.design/research/00-soc-inventory.md`](/.design/research/00-soc-inventory.md) - SoC-wide interrupt topology, bank addresses, and event routing.
- [`/.design/research/04-synthesis.md`](/.design/research/04-synthesis.md) - milestone plan and the R3 risk resolved by this note.
- [`/.design/research/05-lifecycle-delivery-validation.md`](/.design/research/05-lifecycle-delivery-validation.md) - revision-validation style and hardware bring-up constraints.
