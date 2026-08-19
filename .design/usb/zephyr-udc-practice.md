---
type: Research notes
title: Zephyr UDC driver practice survey (for halbao-m5-usb-udc)
description: Zephyr 4.4.99 UDC (next-gen USB device controller) API contract, driver best practices, cdc-acm-console integration recipe, test suite, and hal_* placement recommendation for the Baochip 1x UDC driver.
resource: /home/rektide/src/hal_bao/.design/usb/zephyr-udc-practice.md
tags: [usb, udc, zephyr, baochip, hal_bao]
status: stable
generated: { by: agent, at: 2026-08-19 }
sources:
  - /home/rektide/src/zephyr-baochip (Zephyr 4.4.99 fork, baochip additions)
  - /home/rektide/archive/zephyrproject-rtos (partial hal_* mirror)
---

# Zephyr UDC practice survey — Baochip 1x UDC driver (halbao-m5-usb-udc)

All paths below are relative to `/home/rektide/src/zephyr-baochip` (Zephyr
4.4.99) unless prefixed `~/archive/zephyrproject-rtos/` or `/home/rektide/src/hal_bao`.
Line numbers refer to the checked-out revision at time of writing.

Purpose: ticket `halbao-m5-usb-udc` — write a `struct udc_api` driver for the
Baochip 1x USB device controller, and decide where the driver + DT binding
should live (zephyr fork `~/src/zephyr-baochip` new-files-only now, possible
`hal_bao` module later).

---

## (A) `udc.h` API contract summary

File: `include/zephyr/drivers/usb/udc.h` (812 lines). The public doc page is
`doc/services/connectivity/usb/device_next/api/udc.rst` (:1 `.. _udc_api:`),
which is pure doxygen over this header — the header **is** the contract.

### Core data structures

| Struct | Where | Notes |
|---|---|---|
| `struct udc_device_caps` | udc.h:36-53 | `hs`, `rwup`, `out_ack` (ctrlr auto-acks status OUT, stack then skips enqueueing status buffer), `addr_before_status`, `can_detect_vbus`, `mps0` (`enum udc_mps0` udc.h:24-29). Filled by the driver in pre-init. |
| `struct udc_ep_caps` | udc.h:72-89 | Per-endpoint: `mps`, `control/interrupt/bulk/iso/high_bandwidth/in/out` bits. Static hardware truth, filled in pre-init (see (D)). |
| `struct udc_ep_stat` | udc.h:94-105 | `enabled`, `halted`, `data1`, `odd`, `busy`. Common layer owns `enabled`/`data1`; driver may own `busy`/`halted` bookkeeping. |
| `struct udc_ep_config` | udc.h:114-129 | One per hardware endpoint: `k_fifo fifo` (request queue), `caps`, `stat`, `addr`, `attributes`, `mps`, `interval`. Driver-declared arrays, registered via `udc_register_ep()`. |
| `enum udc_event_type` | udc.h:135-155 | `VBUS_READY`, `VBUS_REMOVED`, `RESUME`, `SUSPEND`, `RESET`, `SOF`, `EP_REQUEST`, `ERROR`. |
| `struct udc_event` | udc.h:165-178 | `type` + union (`value`/`status`/`buf` — `buf` only for `UDC_EVT_EP_REQUEST`) + `dev`. |
| `struct udc_buf_info` | udc.h:187-206 | Stored in `net_buf` user_data of every request: `ep`, `setup/data/status/zlp` stage flags, `claimed/queued` (TBD), `owner`, `err` (result code). Get with `udc_get_buf_info()` udc.h:772. |
| `struct udc_data` | udc.h:278-299 | **Mandatory** as `dev->data`: `ep_lut[32]`, `caps`, `k_mutex mutex`, `event_cb`, `event_ctx`, `atomic_t status`, `void *priv` (driver private), cached `setup[8]` + `setup_pending`/`setup_valid`. |
| `struct udc_api` | udc.h:229-257 | The vtable (below). `device_speed()`/`test_mode()` only required for HS controllers (comment udc.h:225-228). |

Status bits (udc.h:263-270): `UDC_STATUS_INITIALIZED`, `UDC_STATUS_ENABLED`,
`UDC_STATUS_SUSPENDED` — set/cleared **only** by the common layer; drivers read
via `udc_is_initialized()` (udc.h:317), `udc_is_enabled()` (udc.h:331),
`udc_is_suspended()` (udc.h:345) and set suspend via `udc_set_suspended()`
(udc_common.h:40).

### `struct udc_api` callbacks — must-do rules

Every wrapper in `udc_common.c` takes `api->lock(dev)` first, does the
state checks, calls the callback, then unlocks (e.g. `udc_ep_enqueue()`
udc_common.c:551-605). The skeleton comment says it plainly: *"you do not need
to implement basic checks, these are done by the UDC common layer"*
(udc_skeleton.c:370-374).

| Callback | Must do |
|---|---|
| `lock`/`unlock` udc.h:255-256 | Wrap `udc_lock_internal(dev, K_FOREVER)` / `udc_unlock_internal(dev)` (udc_common.h:268-288). Drivers with a driver thread that runs while the lock is held from thread context (rpi_pico, dwc2) add `k_sched_lock()`/`k_sched_unlock()` around it — udc_rpi_pico.c:1092-1102, udc_dwc2.c:2275-2285 — so the driver thread can't be preempted while it holds the mutex. |
| `init` | Called from `udc_init()` (udc_common.c:898-928) after `event_cb`/`event_ctx` stored, under lock. Bring the controller to "can detect VBUS, not visible to host" (udc.h:353-358, skeleton comment udc_skeleton.c:237-241). Enable ctrl EP0 IN/OUT via `udc_ep_enable_internal()` here or in `enable` (rpi_pico does it in `enable`, udc_rpi_pico.c:941-951; skeleton in `init`, udc_skeleton.c:244-254). pico `init` = pinctrl + clock on (udc_rpi_pico.c:1007-1022). |
| `enable` | From `udc_enable()` (udc_common.c:847-874). Make device **visible to host** (attach/pull-up, unmask IRQs). rpi_pico: reset controller, clear regs/DPRAM, mux PHY, force VBUS detect if no pinctrl, enable EP0, write INTE (SOF bit gated on `CONFIG_UDC_ENABLE_SOF`), set DP pull-up if VBUS present, set MAIN_CTRL EN, `irq_enable()` (udc_rpi_pico.c:913-985). |
| `disable` | From `udc_disable()` (udc_common.c:876-896). Opposite of enable; **must keep detecting power-state changes** (udc.h:386-396). Disable ctrl EPs via `udc_ep_disable_internal()`, disable IRQs (udc_rpi_pico.c:987-1005). |
| `shutdown` | From `udc_shutdown()` (udc_common.c:930-955), only reachable when not enabled (-EBUSY otherwise). Full poweroff. pico: `clock_control_off()` (udc_rpi_pico.c:1024-1029). Stack then calls `udc_purge_queues()` (udc_common.c:607-641) to unref leftover ctrl buffers. |
| `device_speed` | Called under lock by `udc_device_speed()` (udc_common.c:823-845); if NULL, common layer assumes `UDC_BUS_SPEED_FS`. Call after reset. Return `enum udc_bus_speed` (udc.h:58-67). |
| `set_address` | `udc_set_address()` inline (udc.h:451-465) checks enabled + locks. Write FADDR. If controller needs address before/after status stage, advertise `caps.addr_before_status` (udc.h:47-48). |
| `host_wakeup` | `udc_host_wakeup()` (udc.h:513-527) checks enabled. Only valid while suspended; generate remote wakeup resume signaling (udc_rpi_pico.c:899-911 sets RESUME bit + `rwu_pending`). |
| `test_mode` | Optional; `udc_test_mode()` returns `-ENOTSUP` when NULL (udc.h:482-501). HS compliance only — fine to omit for a FS Baochip UDC. |
| `ep_enable`/`ep_disable` | Reached via `udc_ep_enable()` (udc_common.c:378-404, rejects ep0 with -EINVAL and requires enabled) or `udc_ep_enable_internal()` (udc_common.c:341-376 — used by the driver itself for ctrl EPs; sets `attributes/mps/interval`, clears `halted`, `data1`, then calls the callback and sets `stat.enabled`). Callback programs the hardware for `cfg`. rpi_pico resets buffer-control PID, allocates DPRAM buffer blocks (`sys_mem_blocks_alloc`), writes EP_CTRL (udc_rpi_pico.c:748-782); disable cancels + frees (udc_rpi_pico.c:784-807). |
| `ep_try_config` | **Not in `struct udc_api`** in this version — the common `udc_ep_try_config()` (udc_common.c:314-339) checks caps generically (`ep_check_config` udc_common.c:212-282) and updates `mps` (`ep_update_mps` udc_common.c:284-312). Declare caps correctly and you get this for free. |
| `ep_enqueue` | From `udc_ep_enqueue()` (udc_common.c:551-605) under lock; requires enabled + ep enabled; special-cases pending cached SETUP on ctrl EPs (udc_common.c:581-596). **Must not block** (skeleton comment udc_skeleton.c:79-85): `udc_buf_put()` the buf, then hand off to driver thread/workqueue. rpi_pico sets a bit in `xfer_new` atomic + `k_event_post` (udc_rpi_pico.c:717-731). Enqueue on a halted ep is legal — must retrigger on halt clear (skeleton udc_skeleton.c:93-106). |
| `ep_dequeue` | From `udc_ep_dequeue()` (udc_common.c:661-698). Must stop any in-flight HW transaction (rpi_pico `rpi_pico_ep_cancel()` udc_rpi_pico.c:204-237) and cancel all queued bufs with `udc_ep_cancel_queued()` (udc_common.h:239, udc_common.c:109-116 — submits each with `-ECONNABORTED`). Wrap in `irq_lock()` (skeleton udc_skeleton.c:118-130). |
| `ep_set_halt` | From `udc_ep_set_halt()` (udc_common.c:452-487; requires enabled, ep enabled, non-ISO). Respond STALL on that pipe. The stack never calls `ep_clear_halt` for ctrl EPs (protocol stall vs functional stall discussion, udc_skeleton.c:163-180); pico arms ep0 STALL + sets STALL bit, OUT needs AVAIL set (udc_rpi_pico.c:809-849). Driver should set `cfg->stat.halted` for non-ep0 (udc_rpi_pico.c:840-842). |
| `ep_clear_halt` | From `udc_ep_clear_halt()` (udc_common.c:489-527; clears `cfg->stat.halted` on success). Must re-arm pending transfers: pico resets `next_pid`, and either calls `rpi_pico_handle_xfer_next()` if busy or posts `XFER_NEW` if bufs are queued (udc_rpi_pico.c:851-886). ep0: return 0 early (udc_rpi_pico.c:859-861). |

### Event delivery & helpers (driver → stack)

- `udc_submit_event(dev, type, status)` (udc_common.h:118, udc_common.c:172-184) —
  builds `struct udc_event` and calls `data->event_cb` **synchronously**. The
  higher layer's callback typically just does `k_msgq_put` (see
  tests/drivers/udc/src/main.c:34-38), so ISR context is fine and expected —
  every rpi_pico ISR branch calls it directly (udc_rpi_pico.c:574-714).
- `udc_submit_ep_event(dev, buf, err)` (udc_common.h:135, udc_common.c:186-205) —
  hardcoded `UDC_EVT_EP_REQUEST`; stores `err` into `bi->err` first. Use to
  complete/cancel requests.
- `udc_submit_sof_event(dev)` (udc_common.h:146-159) — inline, compiled to
  no-op unless `CONFIG_UDC_ENABLE_SOF` (Kconfig:44-48; SOF IRQ can be a CPU
  hog at 8 kHz on HS).
- `udc_setup_received(dev, setup)` (udc_common.h:252-258, udc_common.c:118-170) —
  **thread context only** (takes the UDC lock; comment udc_common.h:252-254).
  Handles: cancel obsolete ctrl data/status bufs with `-ECONNRESET`
  (udc_common.c:128-140), clear busy on both ctrl EPs (142-143), then either
  append the 8-byte setup to the queued setup buffer and submit it (161-167)
  or **cache** it in `data->setup` when the stack hasn't enqueued a setup
  buffer yet (145-160; replayed at next `udc_ep_enqueue`, udc_common.c:581-596).
  Driver copies SETUP out of its FIFO, cancels in-flight ctrl transactions,
  resets ctrl DATA1 toggles, then calls this from its thread (rpi_pico
  `rpi_pico_handle_setup()` udc_rpi_pico.c:453-473).
- Buffer pool: one global `UDC_BUF_POOL_VAR_DEFINE` (udc_common.c:26-28) sized
  by `CONFIG_UDC_BUF_COUNT` / `CONFIG_UDC_BUF_POOL_SIZE` (Kconfig:23-34).
  Drivers/stack allocate with `udc_ep_buf_alloc()` (udc_common.c:700-724) and
  free with `udc_ep_buf_free()` (udc_common.c:811-821). Ctrl-stage helpers:
  `udc_ctrl_setup_alloc` (udc_common.c:734-750, allocates `mps0` bytes, marks
  `bi->setup`), `udc_ctrl_data_alloc` (752-778, OUT rounded up to mps0),
  `udc_ctrl_status_alloc` (780-802). ZLP flag helpers: `udc_ep_buf_set_zlp`
  (udc.h:754-763), `udc_ep_buf_has_zlp`/`clear_zlp` (udc_common.h:219-226) —
  the **driver** must honor `zlp` on IN transfers (rpi_pico does,
  udc_rpi_pico.c:497-501).
- Request FIFO helpers: `udc_buf_get` (remove, udc_common.h:82),
  `udc_buf_peek` (head without removing — used while transfer owns the buf,
  udc_common.h:94), `udc_buf_put` (udc_common.h:104). Endpoint LUT lookup:
  `udc_get_ep_cfg` (udc_common.h:50); index math `USB_EP_LUT_IDX`
  (udc_common.c:30-31) — OUT 0-15, IN 16-31.

### Locking conventions

- One `k_mutex` per instance inside `struct udc_data` (udc.h:284). Common-layer
  entry points lock around callbacks; drivers lock with
  `udc_lock_internal(dev, K_FOREVER)` in their event-thread handlers
  (rpi_pico thread takes it, udc_rpi_pico.c:400/450) and use `irq_lock()`
  for ISR-vs-thread register/queue races (udc_rpi_pico.c:255/290/738/818/863).
- There is **no `udc_trylock`** in this version; the helpers are
  `udc_lock_internal(dev, timeout)` / `udc_unlock_internal(dev)`
  (udc_common.h:268-288).
- `CONFIG_UDC_WORKQUEUE` (Kconfig:50-63) offers a shared driver workqueue
  (`udc_get_work_q()`, udc_common.h:305-317) as the alternative to a
  per-instance thread (skeleton comment udc_skeleton.c:60-65).

---

## (B) Common-layer behavior vs driver responsibilities

| Operation | Common layer (`udc_common.c`) | Driver must |
|---|---|---|
| `udc_init` :898-928 | NULL-check cb/ctx (-EINVAL), -EALREADY, store `event_cb`/`event_ctx`, lock, call `api->init`, set INITIALIZED on success | power minimal logic, pinctrl/clock, optionally enable EP0 via `udc_ep_enable_internal`; be able to raise VBUS events; stay invisible to host |
| `udc_enable` :847-874 | -EPERM if !init, -EALREADY, call `api->enable`, set ENABLED | attach to bus (pull-up), enable IRQs, make EP0 usable |
| `udc_disable` :876-896 | -EALREADY if !enabled, call `api->disable`, clear ENABLED | mask IRQs, detach, keep VBUS detection |
| `udc_shutdown` :930-955 | -EBUSY if enabled, -EALREADY if !init, call `api->shutdown`, clear INITIALIZED | power off; caller does `udc_purge_queues()` afterwards (udc.h:644-651) |
| `udc_ep_enable` :378-404 | rejects ep0 (-EINVAL), -EPERM if !enabled; `_internal` :341-376 validates vs caps, stores attributes/mps/interval, resets halted/data1, sets `stat.enabled` | program endpoint HW in callback; allocate any ep buffer memory |
| `udc_ep_dequeue` :661-698 | -EPERM if !init; skip if FIFO empty | stop HW transaction, call `udc_ep_cancel_queued()` under `irq_lock()` |
| `udc_ep_enqueue` :551-605 | -EPERM if !enabled, -ENODEV no cfg/!enabled; replays cached SETUP for ctrl | `udc_buf_put()` + kick driver thread/work; never block; tolerate halted ep |
| halt :452-527 | enabled checks, ISO rejected (-ENOTSUP), clears `stat.halted` on clear | STALL handshake programming; retrigger queue on clear halt |
| request completion | — | `udc_submit_ep_event(dev, buf, 0)` on success, negative errno (`-ECONNABORTED`, `-ECONNRESET`, `-ECONNREFUSED`, `-ENOBUFS`+`UDC_EVT_ERROR`) otherwise |
| SETUP | `udc_setup_received` :118-170 (cache-or-deliver, cancel stale ctrl stages) | detect SETUP IRQ, snapshot 8 bytes, cancel active ctrl transfers, reset DATA1, call it from thread context |
| bus events | `udc_set_suspended` :33-42 | raise `UDC_EVT_RESET/SUSPEND/RESUME/VBUS_*/SOF` via `udc_submit_event` from ISR; on RESET also reset device address to 0 (udc_rpi_pico.c:642-648) |
| buffers | global pools + ctrl alloc helpers :700-821 | never touch net_buf internals beyond data/len/tailroom + user_data via `udc_get_buf_info()` |

Multi-packet handling is the driver's job: OUT transfers continue
(`rpi_pico_prep_rx` again) until `len < mps` or buffer full
(udc_rpi_pico.c:508-535); IN transfers advance with `net_buf_pull()`
until `buf->len == 0`, honoring `zlp` (udc_rpi_pico.c:475-506).

---

## (C) Best-practice driver structure (distilled)

The canonical template is `drivers/usb/udc/udc_skeleton.c` (comments
udc_skeleton.c:7-24: single .c file, register defs in a separate .h, reuse
`util.h`/`usb_ch9.h` helpers, concise logging). `udc_rpi_pico.c` is the
cleanest full implementation for a simple FIFO/DPRAM-style device controller.
Directory listing (21 drivers): `udc_ambiq, udc_bflb_v1, udc_dwc2 (+per-soc
headers), udc_it82xx2, udc_kinetis, udc_max32, udc_mcux_ehci, udc_mcux_ip3511,
udc_nrf, udc_numaker, udc_renesas_ra, udc_rpi_pico, udc_sam_udp, udc_sam_usbc,
udc_sam_usbhs, udc_sam0, udc_skeleton, udc_smartbond, udc_stm32,
udc_virtual`.

File layout, in order:

1. **Includes**: `"udc_common.h"` first (udc_rpi_pico.c:8), then `<zephyr/kernel.h>`,
   `<zephyr/drivers/usb/udc.h>`, optional pinctrl/clock_control/reset, then
   `LOG_MODULE_REGISTER(udc_baochip, CONFIG_UDC_DRIVER_LOG_LEVEL)`
   (udc_rpi_pico.c:24).
2. **`struct <drv>_config`** (`dev->config`, ROM-able): base reg, any DPRAM
   pointer, `num_of_eps`, `ep_cfg_in`/`ep_cfg_out` array pointers, thread
   stack + size or `make_thread` fn ptr, `irq_enable_func`/`irq_disable_func`
   fn ptrs, clock/reset/pinctrl specs (udc_rpi_pico.c:26-40; skeleton
   udc_skeleton.c:42-49). Note the fn-pointer indirection exists so the
   `DEVICE_DEFINE` macro can close over `IRQ_CONNECT` (needs literals).
3. **Per-endpoint private data**: `next_pid` toggle etc. (udc_rpi_pico.c:42-45).
4. **Driver private data** — *not* `dev->data`; reached via
   `udc_get_private(dev)` (udc_common.h:25-30): driver thread control block,
   `k_event` + atomic ep bitmaps for handoff, per-ep buffers, setup cache
   (udc_rpi_pico.c:56-72; skeleton comment udc_skeleton.c:51-58).
5. **ISR** — only touches registers, event posting, and cheap bookkeeping:
   `rpi_pico_isr_handler` (udc_rpi_pico.c:565-715). Clear each flag via
   write-1-to-clear aliases (udc_rpi_pico.c:100-130), post
   `udc_submit_event`/`udc_submit_sof_event` directly. Handle BUFF_STATUS
   *before* SETUP to preserve ordering (udc_rpi_pico.c:694-699). Log
   unhandled IRQ bits (udc_rpi_pico.c:712-714).
6. **Driver thread** (or UDC workqueue): `k_event_wait`, then
   `udc_lock_internal(dev, K_FOREVER)` … process FINISHED → NEW → SETUP …
   `udc_unlock_internal(dev)` (udc_rpi_pico.c:390-451). This is where
   `udc_setup_received()` runs (thread-only requirement).
7. **Transfer engine**: `handle_xfer_next` (peek; skip control-OUT setup bufs
   udc_rpi_pico.c:364-371; skip halted OUT; prep rx/tx; `udc_ep_set_busy()`;
   on prep failure submit `-ECONNREFUSED`) + completion handlers that
   `udc_buf_get()` (OUT) / `udc_buf_get()` after peek (IN) and
   `udc_submit_ep_event(dev, buf, 0)` (udc_rpi_pico.c:318-351).
8. **`struct udc_api` vtable** (udc_rpi_pico.c:1104-1119; skeleton
   udc_skeleton.c:375-391). `lock` = `k_sched_lock` +
   `udc_lock_internal` (udc_rpi_pico.c:1092-1102).
9. **`<drv>_driver_preinit`** — the `DEVICE_DT_INST_DEFINE` init fn: init
   mutex (`k_mutex_init(&data->mutex)`, udc_rpi_pico.c:1039), init events,
   fill `data->caps`, loop over ep arrays setting caps + `addr =
   USB_EP_DIR_x | i` and `udc_register_ep()` (udc_rpi_pico.c:1044-1085),
   then `config->make_thread(dev)` (udc_rpi_pico.c:1087).
10. **`#define DT_DRV_COMPAT <vendor>,<soc>-usbd`** (udc_rpi_pico.c:1121;
    skeleton udc_skeleton.c:393) — always multi-instance even for single-ctrlr
    SoCs (skeleton comment udc_skeleton.c:395-398).
11. **`DEVICE_DEFINE(n)` macro** (udc_rpi_pico.c:1131-1207):
    `K_THREAD_STACK_DEFINE` (configurable size
    `CONFIG_UDC_<DRV>_STACK_SIZE`, Kconfig.rpi_pico:16-18), optional
    `SYS_MEM_BLOCKS_DEFINE_STATIC_WITH_EXT_BUF` over controller RAM
    (udc_rpi_pico.c:1135-1137), `make_thread` closure (:1146-1159),
    `IRQ_CONNECT`/`irq_enable` + disable closures (:1161-1175),
    `static struct udc_ep_config ep_cfg_out[N]`/`ep_cfg_in[N]` sized from
    `DT_INST_PROP(n, num_bidir_endpoints)` (:1177-1178), const config
    (:1180-1194), `struct <drv>_data priv` + `struct udc_data` with
    `Z_MUTEX_INITIALIZER` and `.priv = &udc_priv_##n` (:1196-1202), then
    `DEVICE_DT_INST_DEFINE(n, <drv>_driver_preinit, NULL, &udc_data_##n,
    &config_##n, POST_KERNEL, CONFIG_KERNEL_INIT_PRIORITY_DEVICE, &api)`
    (:1204-1207). Close with
    `DT_INST_FOREACH_STATUS_OKAY(<DRV>_DEVICE_DEFINE)` (:1209).
    - Quirk warning: both skeleton (:410-411) and rpi_pico (:1185-1186) have
      `.ep_cfg_in = ep_cfg_out, .ep_cfg_out = ep_cfg_in` — swapped in the
      struct initializer. Harmless only because both arrays are declared
      identically; **don't copy blindly** into a driver whose IN/OUT arrays
      differ.
12. **Kconfig**: new `drivers/usb/udc/Kconfig.<drv>` with
    `config UDC_<DRV>` `bool`, `default y`,
    `depends on DT_HAS_<COMPAT_UPPER>_ENABLED`, `select`s for needed
    subsystems (rpi_pico selects `SYS_MEM_BLOCKS`, `EVENTS`, implies
    `PINCTRL`, Kconfig.rpi_pico:4-12), plus `<DRV>_STACK_SIZE` (default 512)
    and `<DRV>_THREAD_PRIORITY` (default 8) :16-24. Source it from
    `drivers/usb/udc/Kconfig` (alphabetical block, Kconfig:72-90). Note:
    HS-capable drivers `select UDC_DRIVER_HAS_HIGH_SPEED_SUPPORT`
    (Kconfig:13-21, Kconfig.skeleton:8).
13. **CMake**: one line
    `zephyr_library_sources_ifdef(CONFIG_UDC_<DRV> udc_<drv>.c)` in
    `drivers/usb/udc/CMakeLists.txt` (rpi_pico at :20).

DMA / cache practice (for controllers with real DMA): `udc_dwc2.c` uses
`sys_cache_data_flush_range()` before IN DMA and
`sys_cache_data_invd_range()` after OUT DMA / before reading TRB lengths
(udc_dwc2.c:1555-1559, 1592, 757, 2645), or the global
`CONFIG_UDC_BUF_FORCE_NOCACHE` (Kconfig:37-43) can place the shared request
pool in nocache memory when the driver can't handle cached buffers. A Baochip
UDC that DMAs from `net_buf` memory needs one of these.

---

## (D) Endpoint capabilities & data structures conventions

- DT binding includes `usb-ep.yaml` (dts/bindings/usb/usb-ep.yaml:1-26) which
  requires `num-bidir-endpoints` (incl. EP0) and optionally
  `num-in-endpoints`/`num-out-endpoints`; base include chain is
  `usb-controller.yaml`. Minimal binding = description + example + compatible
  + `include: [usb-ep.yaml]` (see zephyr,udc-skeleton.yaml:1-8;
  raspberrypi,pico-usbd.yaml:1-33 additionally includes `reset-device.yaml`,
  `pinctrl-device.yaml` and documents the `zephyr_udc0:` label + VBUS-detect
  pinctrl example in its description).
- SoC .dtsi node convention (rp2040.dtsi:351-360): `usbd: usbd@<addr>` with
  `compatible`, `reg`, `resets`, `clocks`, `interrupts` + `interrupt-names`,
  `num-bidir-endpoints = <16>`, `status = "disabled"` — the **board** DTS
  sets `zephyr_udc0: &usbd { status = "okay"; ... };` (20+ examples, e.g.
  boards/pimoroni/tiny2040/tiny2040.dts:168, boards/nordic/thingy53/…:322).
  The `zephyr_udc0` label is what device_next binds to (below), and it lives
  in board files, not SoC dtsi (except a few bindings like sam-udp that
  document relabeling, dts/bindings/usb/atmel,sam-udp.yaml:34).
- Driver pre-init fills `caps` per endpoint: index 0 = control with
  `mps = 64` (`caps.mps0 = UDC_MPS0_64`); others typically set
  `bulk = interrupt = iso = 1` and `mps = 1023` (FS) / `1024` (HS);
  `high_bandwidth` only if HW supports it (udc_rpi_pico.c:1044-1085;
  udc_skeleton.c:298-343 with HS variant at :300-303). `ep_check_config`
  (udc_common.c:212-282) matches requests against exactly these bits plus
  direction, so over-claiming capabilities = subtle enumeration bugs.
- `addr` encoding: `USB_EP_DIR_IN | idx` / `USB_EP_DIR_OUT | idx`;
  common layer LUT is OUT 0-15 / IN 16-31 (udc_common.c:30-31) — max 16
  bidirectional endpoints.

---

## (E) cdc-acm-console snippet integration recipe

The device_next stack and its CDC-ACM glue bind to **the devicetree node
labeled `zephyr_udc0`**: `USBD_DEVICE_DEFINE(cdc_acm_serial,
DEVICE_DT_GET(DT_NODELABEL(zephyr_udc0)), …)` (subsys/usb/device_next/app/cdc_acm_serial.c:23-25;
same in usbd_shell.c:49). So the board must provide that label on an enabled
UDC node — this is the only DT contract.

Snippet `snippets/cdc-acm-console/` (README.rst:27-36):

1. **DT (board)** — enable the controller with the magic label:

   ```dts
   zephyr_udc0: &usbd {          /* node defined in bao1x.dtsi, status disabled */
           status = "okay";
           /* pinctrl/clocks as needed */
   };
   ```

   The snippet then does the rest itself — `cdc-acm-console.overlay:7-18`
   sets `chosen { zephyr,console = &snippet_cdc_acm_console_uart;
   zephyr,shell-uart = &…; }` and adds a child of `&zephyr_udc0`:
   `snippet_cdc_acm_console_uart { compatible = "zephyr,cdc-acm-uart"; };`
   (binding dts/bindings/serial/zephyr,cdc-acm-uart.yaml, `on-bus: usb`).
2. **Kconfig (all provided by the snippet's
   `cdc-acm-console.conf:1-7`)**: `CONFIG_USB_DEVICE_STACK_NEXT=y`,
   `CONFIG_CDC_ACM_SERIAL_INITIALIZE_AT_BOOT=y` (app glue,
   app/Kconfig.cdc_acm_serial:9-13, depends on `USBD_CDC_ACM_CLASS`),
   `CONFIG_SERIAL=y`, `CONFIG_CONSOLE=y`, `CONFIG_UART_CONSOLE=y`,
   `CONFIG_UART_LINE_CTRL=y`. `USBD_CDC_ACM_CLASS` self-defaults to y when a
   `zephyr,cdc-acm-uart` node exists (class/Kconfig.cdc_acm:6-13). The
   legacy `CONFIG_CDC_ACM` symbol is the old stack — not used here.
3. **Usage**: `west build -S cdc-acm-console …` (README.rst:6-8); sample
   equivalent without the snippet is `samples/subsys/usb/console`
   (app.overlay:13-16 + prj.conf:1-10) — good smoke test: prints
   "Hello World!" to `/dev/ttyACM*`.

For bring-up, `CONFIG_UDC_DRIVER_LOG_LEVEL_DBG=y` (log template
Kconfig:68-70) plus `usbd shell` commands give enumeration visibility.

---

## (F) `tests/drivers/udc` usage & applicability

- Layout: `tests/drivers/udc/{CMakeLists.txt, prj.conf, tests.yaml,
  udc_skeleton.overlay, src/main.c}`.
- `prj.conf:1-11`: `CONFIG_LOG`, `CONFIG_ZTEST`, `CONFIG_UDC_DRIVER=y`,
  `CONFIG_UDC_ENABLE_SOF=y`, `CONFIG_UDC_BUF_COUNT=16`,
  `CONFIG_UDC_BUF_POOL_SIZE=16384`, log level INF.
- The suite gets the device via `DEVICE_DT_GET(DT_NODELABEL(zephyr_udc0))`
  (src/main.c:390 and 5 other sites) — **the board must provide the
  `zephyr_udc0` label**. It passes its own event callback into `udc_init()`
  and pumps a `k_msgq` (src/main.c:27-38), exercising API rule conformance,
  `ep_try_config`, alloc/free, queueing and dequeue of buffers *without a
  host connected* (src/main.c:15-19).
- `tests.yaml:1-30`: `drivers.usb.udc` `depends_on: usbd` (harness feature)
  with integration platforms nrf52840dk, frdm_k64f, nucleo_f413zh,
  mimxrt1050_evk, rpi_pico; a `build_only` variant for nrf54h20dk; and
  `drivers.usb.udc.skeleton` which runs on **native_sim/native/64** using
  `udc_skeleton.overlay` (:1-15 — deletes the board's `zephyr_udc0` node and
  instantiates a root-level `zephyr,udc-skeleton` node with
  `num-bidir-endpoints = <8>; maximum-speed = "high-speed";`, then that node
  gets the `zephyr_udc0` label).
- Applicability to Baochip: run `drivers.usb.udc` on the `dabao` board once
  its DTS exposes `zephyr_udc0` (add `depends_on: usbd` to the board's
  board.yml). The skeleton-overlay trick also means we can run the suite on
  `native_sim` for API-conformance testing of *our* driver if we ever add a
  virtual variant — but there is no native_sim model of foreign UDC
  hardware, so on-target run on dabao (or verilator variant) is the real
  test. Also consider `samples/subsys/usb/console` as the first
  host-connected smoke test.

---

## (G) hal_* survey & placement recommendation

Local mirror `~/archive/zephyrproject-rtos/` contains a **partial** set:
`hal_adi, hal_espressif, hal_nxp, hal_renesas, hal_silabs, hal_stm32,
hal_wch` (plus non-hal modules). The rest of the hal_* family (hal_nordic,
hal_ti, hal_atmel, hal_intel, hal_infineon, hal_rpi_pico, hal_gigadevice,
hal_microchip, hal_telink, …) is not checked out; findings below generalize
from the seven present + the module.yml schema in-tree.

### (1) What hal_* modules contain

Uniformly: **vendor SDK sources, closed-source binary blobs, and glue — not
Zephyr drivers.**

- `hal_stm32`: `stm32cube/` (Cube HAL/LL), `dts/` (generated
  `*-pinctrl.dtsi`, see dts/README.rst:1-24), `zephyr/` = only `module.yml`
  + blobs dir. module.yml: `build: cmake: .`, `settings: dts_root: .`, plus
  BLE/802.15.4 blob declarations.
- `hal_silabs`: `gecko/`, `simplicity_sdk/`, `si32/`, `wiseconnect/` SDKs +
  blobs (RAIL, linklayer, openthread .a). module.yml: `cmake-ext: True`,
  `kconfig-ext: True` (glue files live in the zephyr tree's modules dir).
- `hal_espressif`: `components/`, `tools/`, esp-idf subtree; module.yml:
  `cmake: zephyr`, `kconfig: zephyr/Kconfig`, `settings: dts_root: .`, pip
  requirements, blobs (BLE libs).
- `hal_nxp`: MCUX SDK + WiFi/BT/boot firmware blobs.
- `hal_renesas`: FSP under `zephyr/ra|rx|rz`, `smartbond/`; blobs (libcmac,
  dave2d).
- `hal_adi`: `module.yml` is *just* `build: cmake: .` + `dts_root: .` —
  thin wrapper over vendor pack.
- `hal_wch`: `ch32fun` + `cmake-ext/kconfig-ext`.

### (2) Where vendor-IP Zephyr drivers live

**In the zephyr tree, always.** Every UDC driver lives in
`zephyr/drivers/usb/udc/` (28 files incl. 21 drivers), every binding in
`zephyr/dts/bindings/usb/`, every board in `zephyr/boards/`. The module.yml
`settings:` schema (doc/develop/modules.rst:921-945) supports exactly:
`board_root`, `dts_root`, `snippet_root`, `soc_root`, `arch_root`,
`module_ext_root`, `sca_root` — **there is no `drivers_root`** (grep of
cmake/ and the whole tree for `drivers_root` returns nothing). A module can
compile arbitrary `zephyr_library` sources via its `CMakeLists.txt` +
`Kconfig` (that's how hal glue adds vendor SDK code), and modules *can* ship
DT content via `dts_root` — `hal_stm32` ships `dts/st/…/*-pinctrl.dtsi`
include fragments this way. But no hal_* ships drivers through
`drivers_root`; out-of-tree driver code in modules is rare, nonstandard, and
loses the Kconfig/CMake integration points of `drivers/` (e.g. being picked
up by `tests/drivers/udc` conventions, `default y` on DT compat).

### (3) Recommendation for hal_bao / Baochip 1x UDC — now and later

Current state: the fork `~/src/zephyr-baochip` already carries all SoC
support as new files: `soc/baochip/bao1x`, `dts/riscv/baochip/bao1x.dtsi`,
bindings `baochip,bao1x-intc/duart/udma-uart/ticktimer` (vendor prefix
`baochip` registered at dts/bindings/vendor-prefixes.txt:109), drivers
`drivers/{interrupt_controller,serial,timer}/…baochip…`, and
`boards/baochip/dabao`. `hal_bao` is a module with `build: cmake: .` +
`zephyr_include_directories(include)` and `include/bao1x_peri.h` (register
definitions — exactly the "vendor SDK header" role).

**Now — put the UDC driver in the fork, alongside the other Baochip
drivers:**

- `drivers/usb/udc/udc_baochip.c` (+ `udc_baochip.h` for register defs if
  large, per skeleton note udc_skeleton.c:17-19), following (C);
  `Kconfig.baochip` + `CMakeLists.txt` entries, modeled on Kconfig.rpi_pico.
- DT binding `dts/bindings/usb/baochip,bao1x-udc.yaml` including
  `usb-ep.yaml` (and `usb-controller.yaml` chain); node `usbd:` in
  `dts/riscv/baochip/bao1x.dtsi` with `status = "disabled"`; board overlay
  `zephyr_udc0: &usbd { status = "okay"; };` in `boards/baochip/dabao/`.
- Rationale: this matches upstream practice (drivers never come from hal_*
  modules), keeps the driver testable by `tests/drivers/udc` and the console
  snippet with zero extra plumbing, and is consistent with where intc/uart/
  timer already live. File-count discipline (new files only) is satisfied —
  `Kconfig`/`CMakeLists.txt` one-liners are unavoidable shared-file edits.

**Later — moving to `hal_bao` module if/when we de-fork:**

- What module.yml can carry: `soc_root: .` (move `soc/baochip` →
  `hal_bao/soc/baochip`), `board_root: .` (`hal_bao/boards/…`),
  `dts_root: .` (`hal_bao/dts/riscv/baochip/…` **and**
  `hal_bao/dts/bindings/usb/baochip,bao1x-udc.yaml` — dts_root adds a
  bindings search root too), `snippet_root` if needed
  (doc/develop/modules.rst:923-936).
- What it cannot carry natively: the `drivers/usb/udc/` integration. The
  driver .c would become a module-owned `zephyr_library` compiled from
  `hal_bao/CMakeLists.txt` with its own `Kconfig` entry (pattern used by
  module glue everywhere, e.g. hal_renesas/zephyr/CMakeLists.txt:1-5
  `add_subdirectory_ifdef`). `DEVICE_DT_INST_DEFINE` works fine from a module
  as long as the binding is visible via `dts_root`. Cost: our Kconfig symbol
  won't sit under `UDC_DRIVER`'s menu unless we source it manually, CMake
  won't be a one-liner in the zephyr tree, and upstream-style test discovery
  stays fork-only.
- Migration steps, concretely: add `soc_root/dts_root/board_root: .` to
  `hal_bao/zephyr/module.yml`; `git mv` the soc, dts, bindings, boards trees;
  move `udc_baochip.c` into `hal_bao` (e.g. `hal_bao/drivers/usb/udc/`),
  add `hal_bao/Kconfig` with `UDC_BAOCHIP` and `hal_bao/CMakeLists.txt`
  `zephyr_library()` + `zephyr_library_sources()` guarded by the new
  Kconfig; west manifest swap `-baochip` fork → upstream zephyr + hal_bao.
- Bottom line: **driver in the fork now (with the other drivers); move
  driver source to hal_bao only together with the wholesale soc/dts/board
  move, using module CMake+Kconfig, accepting the minor loss of in-tree
  drivers/ integration.** Keep register definitions (`bao1x_peri.h` style)
  in `hal_bao/include` from day one so the eventual split touches only the
  driver .c and build files.

---

## Cross-references

- `/home/rektide/src/hal_bao/.design/` — sibling research dirs (`init/`,
  `bringup/`, `research/`) for Baochip porting context.
- `/home/rektide/src/hal_bao/include/bao1x_peri.h` — register definitions
  home; UDC register block belongs alongside (see (G)).
- `~/src/zephyr-baochip/dts/riscv/baochip/bao1x.dtsi` — where the `usbd`
  node gets added; existing intc/ticktimer nodes show the house DT style
  (`edge-triggered-masks` etc., bao1x.dtsi:50-75).
