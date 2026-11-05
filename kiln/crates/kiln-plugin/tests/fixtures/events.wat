;; SPDX-License-Identifier: Apache-2.0
;;
;; Receives events slowly. Each `handle-events` logs one `e` per event it was handed (up to
;; 16), and each `run-command` logs `r`; then each waits `$busy-ns` nanoseconds on the monotonic
;; clock: in a WASI host import, as a plugin waiting on I/O would. Tests rewrite the wait, the
;; `(i64.const 0)` on the line marked `busy-ns`; at 0 they return at once. Otherwise empty.wat.
(component $C
  (import "kiln:extension/types@1.0.0" (instance $types
    (type $range (record (field "start" u64) (field "end" u64)))
    (export "range" (type (eq $range)))
    (type $error (variant
      (case "unimplemented" string)
      (case "denied" string)
      (case "not-found" string)
      (case "invalid-argument" string)
      (case "conflict" string)
      (case "failed" string)))
    (export "error" (type (eq $error)))))
  (alias export $types "range" (type $range))
  (alias export $types "error" (type $error))

  (import "kiln:extension/log@1.0.0" (instance $log
    (type $level (enum "trace" "debug" "info" "warn" "error"))
    (export "level" (type $level' (eq $level)))
    (export "log" (func (param "level" $level') (param "message" string)))))

  (import "kiln:extension/events@1.0.0" (instance $events
    (alias outer $C $range (type $range))
    (type $buffer-opened (record
      (field "buffer" u64)
      (field "version" u64)
      (field "path" (option string))
      (field "language" (option string))))
    (export "buffer-opened" (type $buffer-opened' (eq $buffer-opened)))
    (type $buffer-changed (record
      (field "buffer" u64) (field "version" u64) (field "ranges" (list $range))))
    (export "buffer-changed" (type $buffer-changed' (eq $buffer-changed)))
    (type $buffer-saved (record
      (field "buffer" u64) (field "version" u64) (field "path" string)))
    (export "buffer-saved" (type $buffer-saved' (eq $buffer-saved)))
    (type $selection-changed (record
      (field "buffer" u64) (field "version" u64) (field "ranges" (list $range))))
    (export "selection-changed" (type $selection-changed' (eq $selection-changed)))
    (type $event (variant
      (case "buffer-opened" $buffer-opened')
      (case "buffer-changed" $buffer-changed')
      (case "buffer-saved" $buffer-saved')
      (case "buffer-closed" u64)
      (case "focus-changed" (option u64))
      (case "selection-changed" $selection-changed')
      (case "config-reloaded")
      (case "lagged" u64)))
    (export "event" (type (eq $event)))))
  (alias export $events "event" (type $event))

  (import "wasi:io/poll@0.2.0" (instance $poll
    (export "pollable" (type $pollable (sub resource)))
    (export "[method]pollable.block" (func (param "self" (borrow $pollable))))))
  (alias export $poll "pollable" (type $pollable))
  (import "wasi:clocks/monotonic-clock@0.2.0" (instance $clock
    (alias outer $C $pollable (type $pollable))
    (export "subscribe-duration" (func (param "when" u64) (result (own $pollable))))))

  (core module $libc
    (memory (export "memory") 1)
    ;; Bytes 0..8 stay zero: a `result` that is `ok`. `$main`'s strings start at 256; the
    ;; heap at 1024.
    (global $heap (mut i32) (i32.const 1024))
    (func (export "realloc") (param i32 i32) (param $align i32) (param $size i32) (result i32)
      (local $p i32)
      (local.set $p
        (i32.and
          (i32.add (global.get $heap) (i32.sub (local.get $align) (i32.const 1)))
          (i32.sub (i32.const 0) (local.get $align))))
      (global.set $heap (i32.add (local.get $p) (local.get $size)))
      (local.get $p)))
  (core instance $libc (instantiate $libc))
  (alias core export $libc "memory" (core memory $memory))
  (alias core export $libc "realloc" (core func $realloc))

  (alias export $log "log" (func $log-log))
  (alias export $clock "subscribe-duration" (func $subscribe-duration))
  (alias export $poll "[method]pollable.block" (func $pollable-block))
  (core func $log (canon lower (func $log-log) (memory $memory)))
  (core func $subscribe (canon lower (func $subscribe-duration)))
  (core func $block (canon lower (func $pollable-block)))
  (core func $drop (canon resource.drop $pollable))
  (core instance $host
    (export "log" (func $log))
    (export "subscribe-duration" (func $subscribe))
    (export "block" (func $block))
    (export "drop" (func $drop)))

  (core module $main
    (import "libc" "memory" (memory 1))
    (import "host" "log" (func $log (param i32 i32 i32)))
    (import "host" "subscribe-duration" (func $subscribe-duration (param i64) (result i32)))
    (import "host" "block" (func $block (param i32)))
    (import "host" "drop" (func $drop (param i32)))
    (global $busy-ns i64 (i64.const 0)) ;; busy-ns
    (data (i32.const 256) "eeeeeeeeeeeeeeee")
    (data (i32.const 272) "r")
    (func $wait
      (local $pollable i32)
      (if (i64.ne (global.get $busy-ns) (i64.const 0))
        (then
          (local.set $pollable (call $subscribe-duration (global.get $busy-ns)))
          (call $block (local.get $pollable))
          (call $drop (local.get $pollable)))))
    (func (export "init") (result i32) (i32.const 0))
    (func (export "shutdown"))
    (func (export "run-command") (param i32 i32 i32 i32) (result i32)
      ;; Level `info`.
      (call $log (i32.const 2) (i32.const 272) (i32.const 1))
      (call $wait)
      (i32.const 0))
    (func (export "handle-events") (param $events i32) (param $count i32)
      (call $log (i32.const 2) (i32.const 256)
        (select (local.get $count) (i32.const 16)
          (i32.lt_u (local.get $count) (i32.const 16))))
      (call $wait)))
  (core instance $main (instantiate $main
    (with "libc" (instance $libc))
    (with "host" (instance $host))))

  (func (export "init") (result (result (error $error)))
    (canon lift (core func $main "init") (memory $memory)))
  (func (export "shutdown")
    (canon lift (core func $main "shutdown")))

  (func $run-command (param "id" string) (param "args" string) (result (result (error $error)))
    (canon lift (core func $main "run-command") (memory $memory) (realloc $realloc)))
  (instance $command-handler (export "run-command" (func $run-command)))
  (export "kiln:extension/command-handler@1.0.0" (instance $command-handler))

  (func $handle-events (param "events" (list $event))
    (canon lift (core func $main "handle-events") (memory $memory) (realloc $realloc)))
  (instance $event-handler (export "handle-events" (func $handle-events)))
  (export "kiln:extension/event-handler@1.0.0" (instance $event-handler)))
