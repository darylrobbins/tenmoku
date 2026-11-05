;; SPDX-License-Identifier: Apache-2.0
;;
;; Asks for more memory on every `run-command`: grows its memory by 64 pages (4 MiB) and logs
;; `refused` when the grow returns -1, `grown` when it succeeds. It never traps on a refusal,
;; so a host that refuses the grow keeps it running. strings.wat's `$libc`/`$main` layout.
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
  (core func $log (canon lower (func $log-log) (memory $memory)))
  (core instance $host (export "log" (func $log)))

  (core module $main
    (import "libc" "memory" (memory 1))
    (import "host" "log" (func $log (param i32 i32 i32)))
    (data (i32.const 256) "refused")
    (data (i32.const 272) "grown")
    (func (export "init") (result i32) (i32.const 0))
    (func (export "shutdown"))
    (func (export "run-command") (param i32 i32 i32 i32) (result i32)
      ;; Level `info`.
      (if (i32.eq (memory.grow (i32.const 64)) (i32.const -1))
        (then (call $log (i32.const 2) (i32.const 256) (i32.const 7)))
        (else (call $log (i32.const 2) (i32.const 272) (i32.const 5))))
      (i32.const 0))
    (func (export "handle-events") (param i32 i32)))
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
