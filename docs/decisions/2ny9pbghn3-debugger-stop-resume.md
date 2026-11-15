# fe-dap stops and resumes: design calls (debugger D2b)

Card T654 (Mac T1842), under `docs/decisions/r1b9dw3g0g-debugger.md` §2 items 4-5,
§11 D2 C2-C5 and §13 (F2, F11). §13 wins over §§1-12 where they disagree.

## Uncertain after a failed resume, and the probe (§13 F2)

§2 item 4 had a failed resume return to Stopped, on the reasoning that a failure
means the program never resumed. F2 rejects that: an adapter can fail a step after
it has let the program go (a `cancelled` that came too late, an internal error
half-way), so a failure proves nothing. So:

- a resume's failure that passes the guard moves to **Uncertain**, the error shown
  (`DapEvent::Failed`);
- unless the adapter said `notStopped` (DAP's short `message` for "the debuggee
  isn't stopped"): the program is running, so the session moves to **Running**;
- in Uncertain the session sends a **probe**: `threads`, then a one-frame
  `stackTrace` on the resumed thread (or the first listed). Either answered
  `notStopped` moves to Running; any other failure or a timeout leaves the session
  Uncertain, where Continue, Pause and Stop still work;
- a resume that **timed out** gets no probe: the adapter isn't answering, so its
  late reply (expected, since the session `cancel`s it) or a `stopped`/`continued`
  is the way out. Its late success moves to Running; its late failure, `cancelled`
  included, is a failure like any other: Uncertain, and a probe.

**What proves a stop.** F2 counts "a successful `threads`/`stackTrace` that the
adapter answers while stopped". `threads` alone never does: most adapters list
threads while running. A stack does only from an adapter that gives one for
suspended threads alone, and that isn't every adapter: debugpy (pydevd's
`InternalGetThreadStack`) waits half a second for the thread to suspend and then
answers with the running thread's stack. So a successful stack proves a stop only
when the session is told it can (`SessionOptions::stack_proves_stop`, off by
default), and only a well-formed stack with at least one frame; anything else
proves nothing, and the session stays Uncertain until a `stopped` event (or the
user's Pause brings one). A proven stop moves to a new epoch, takes the probed
thread as suspended, and fetches the stop's data afresh (`StopData::stopped` is
`None` for it); the probe's frame isn't kept. Which built-in adapters can set the
option is for D16's real-adapter tests to settle, adapter by adapter; until then
every adapter is treated as debugpy is.

The probe is the only `stackTrace` the session sends outside Stopped.

## Epochs, stale ids and stop data

`Epoch` is session-long and only grows: `stopped`, `continued`, sending a resume,
sending `restart`, shutting down and each generation's start and end (an adapter
that exits by itself included) move it. `StopRef { epoch, id }` carries a frame id
or a variables reference; `Session::scopes`, `variables`, `frames` and
`evaluate(_, Some(frame))` take one. A request whose epoch isn't the current one,
or that comes when the session isn't Stopped, answers `Stale` without a round trip.

Every stop-data request (`stackTrace`, `scopes`, `variables`, `evaluate`,
`setVariable`, `setExpression`, `completions`, `exceptionInfo`) goes only while
Stopped, and is of one stop: the one its `StopRef` names, or, for a typed
`Session::request` or a raw one, the epoch the handle's view shows **when the host
asks**. The binding is made on the handle, under the view's lock, before the
command is queued, not when the actor takes it: a stop that ends while the request
waits in the queue answers it `Stale` instead of sending its old ids under the new
epoch. A request bound to the current epoch while the session isn't Stopped is
refused `RequestError::NotStopped`; one of another epoch, `Stale`. A `stackTrace`
for a thread that isn't suspended is refused. The one exception is a frameless
`evaluate` asked while not Stopped and outside Resuming and Uncertain (some adapters
evaluate while running): it carries no epoch, goes in Running and at startup, and is
of whatever stop is current when it goes (it names no ids of its own).

The commands the session sends itself (`initialize`, `launch`, `attach`,
`configurationDone`, `restart`, `terminate`, `disconnect`) are refused as raw
requests (`RequestError::SessionOwned`): sent behind the session's back they would
restart or end the program without moving the epoch or passing the restart guard.
The host uses `Session::restart` and `Session::stop`.

A bound request in flight answers `Stale` the moment the epoch moves (not when its
reply comes: a `repl` evaluation has no timeout), and its reply is dropped when it
comes. No `cancel` is sent for it. The session's own fetches carry their epoch too;
a reply under another epoch is dropped, so a `variables` reply from before a step
never fills the new stop.

## Suspended threads

The session keeps which threads are suspended: all of them (a `stopped` with
`allThreadsStopped: true`, or naming no thread), or a set (each `stopped` naming
its thread). Sending a resume clears it, and its success decides what is left: a
`continue` answered `allThreadsContinued: false` resumed only its thread, every
other resume all of them (v1 never sends `singleThread`). A `continued` does the
same with its own flag; one that comes while a `continue` is still pending applies
to that continue's snapshot of the suspended threads (a pending step runs them all),
so a partial `continued` before the answer keeps the others stopped, and the late
answer then fails the guard. A `stopped` that comes before a pending resume's answer
counts only the threads it names: until the answer says otherwise a resume ran
every thread, which is DAP's default for `allThreadsContinued`.

Threads that go are taken out everywhere: a `thread` event's `exited`, or a newer
`threads` list that leaves a thread out, removes it from the suspended threads and
from a pending resume's snapshot. A stop whose shown thread goes moves, at a new
epoch, to another suspended thread and fetches it; with none left the stop is over
and the session is Running at a new epoch. The stop's fetch checks the same thing
against its own `threads` reply before asking for a stack, so a stop never asks for
the stack of a thread its list doesn't have. If some threads are left suspended, the session is Stopped
at the new epoch on one of them (the one that last stopped, if still suspended)
and fetches its data again, as §2 item 5 asks; otherwise it is Running. The session
state stays one of the two: the toolbar acts on the stop the data is of.

## Thread lists

A `threads` reply replaces the list only if nothing has changed since its request
went: it must have been sent at the current epoch (no stop, resume, `continued`,
restart or new run since), and after the last applied reply and the last `thread`
event. It is checked before it changes anything. So a held baseline, or a Pause's
`threads`, that comes after a stop and before the stop's own list changes nothing
(and can't turn Pause off with an obsolete empty list, or take a thread out of the
suspended ones). Each list applied also forgets a remembered stopped
thread, and a suspended one, that it leaves out: `thread` events are optional, so a
thread can go silently.

## The control-state query, and controls off from the call on

`Session::controls()` computes `Controls` (Continue, Pause, step, Restart, Stop:
each `Enabled` or `Disabled(reason)`, the reason's `Display` the tooltip text from
the catalogue) from the session's view, so D8's toolbar and commands hold no logic.
It is a pure function of the view, with no event of its own: every change it
depends on already reaches the sink as a `State` or `Threads` event.

`Session::resume` and `Session::pause` check the controls themselves and refuse
(`false`) what is disabled. An accepted resume marks the view at once, under its
lock, so Continue and stepping are disabled from the call on (C1's "from the send
on"), before the actor has taken the command: a double click can't send two steps.
The command carries the epoch the user saw; if a stop or `continued` came in
between, the actor drops it rather than resume a stop the user never saw.

Raw resume requests (`Session::raw_request` with `continue`, `next`, `stepIn`,
`stepOut`, `goto`, `restartFrame`, `stepBack`, `reverseContinue`) go through the
same machinery: they become the pending resume, move the epoch and are answered
both to the host and through the guard. A dropped future for one sends `cancel`
(with `supportsCancelRequest`) at once, as any dropped request does, but keeps the
pending resume, so its answer still goes through the guard and the session never
sits in Resuming for a request nobody tracks.

A resume asked for and not yet taken reads as Resuming only while the state is
Stopped or Uncertain, the states it can be asked in: an end, a shutdown or a
restart that comes first wins, and the mark is cleared when a run ends.

Per state: starting, only Stop; Running, Pause, Restart, Stop; Stopped, Continue,
step, Restart, Stop; Resuming, Pause and Stop; Uncertain, Continue, Pause and Stop
(Restart off, as D2's restart is from Running or Stopped only); Restarting, Stop.

## No control after Stop (§13 F11)

Once `Session::stop` has been called, every control is disabled, whatever state a
late event would have moved to. This rests on the handle's `stopping` flag, which
is set before the command goes and never cleared, rather than on the state alone:
F11's concern is exactly a late event re-enabling something. The actor also ignores
`stopped` and `continued` once shutting down (D2), and a pending resume is dropped
when the shutdown starts, so its late reply fails the guard.

## Pause

`Session::pause(thread)` targets the given thread, else the one that last stopped
(while a list still has it), else the first known thread (the startup baseline's,
kept current by `thread` events and later `threads` replies). With none known it
sends `threads` first and pauses the first listed, but only if nothing changed
while it asked: a stop, a resume or a shutdown in between gives the pause up. Pause
is disabled with "the program reported no threads" only once a `threads` reply this
run has listed none; before any reply (or after a failed baseline) it stays enabled
and asks. Pause is enabled while Running, Resuming and Uncertain.

## Not modelled in v1

- Pausing the threads that still run during a partial stop. The session is
  Stopped and Pause reads "already stopped", as the toolbar acts on the stop shown;
  v1's adapters (lldb-dap, codelldb, delve, debugpy) stop every thread at once.
- Announcements (`kiln_ui::a11y::announce`): fe-dap is GPUI-free and has no UI.
  The host announces from the `State` events (D8).
