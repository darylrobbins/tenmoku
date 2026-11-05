;; SPDX-License-Identifier: Apache-2.0
;;
;; A plugin with commands, loaded as `acme.tool`. Its `init` registers three, stopping at the
;; first error and returning it:
;;
;; - `acme.tool.hello`, with an args schema and a context predicate: `run-command` logs its id,
;;   then its args;
;; - `acme.tool.spin`: `run-command` never returns;
;; - `acme.tool.invoke`, which mutates the buffer: `run-command` invokes `test.read` with `{}`
;;   and returns what that returned.
;;
;; A test can make `$early` the start function, registering `acme.tool.early` before `init`.
;; `run-command` tells them apart by the id's length. `try-register` registers any id, titled
;; `Hello`, and `try-invoke` invokes any command; each returns what the host import returned.
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

  (import "kiln:extension/commands@1.0.0" (instance $commands
    (alias outer $C $error (type $error))
    (type $command (record
      (field "id" string)
      (field "title" string)
      (field "args-schema" (option string))
      (field "when" (option string))
      (field "mutates-buffer" bool)))
    (export "command" (type $command' (eq $command)))
    (export "register" (func (param "command" $command') (result (result (error $error)))))
    (export "invoke" (func (param "id" string) (param "args" string) (result (result (error $error)))))))

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
    ;; Bytes 0..8 stay zero: a `result` that is `ok`. 64..76 hold import results;
    ;; `$main`'s strings start at 256; the heap at 1024.
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
  (alias export $commands "register" (func $commands-register))
  (alias export $commands "invoke" (func $commands-invoke))
  (core func $log (canon lower (func $log-log) (memory $memory)))
  (core func $register (canon lower (func $commands-register) (memory $memory) (realloc $realloc)))
  (core func $invoke (canon lower (func $commands-invoke) (memory $memory) (realloc $realloc)))
  (core instance $host
    (export "log" (func $log))
    (export "register" (func $register))
    (export "invoke" (func $invoke)))

  (core module $main
    (import "libc" "memory" (memory 1))
    (import "host" "log" (func $log (param i32 i32 i32)))
    (import "host" "register" (func $register (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)))
    (import "host" "invoke" (func $invoke (param i32 i32 i32 i32 i32)))

    (data (i32.const 256) "acme.tool.hello")
    (data (i32.const 272) "Hello")
    (data (i32.const 288) "{\"type\":\"object\"}")
    (data (i32.const 320) "editor && !read_only")
    (data (i32.const 352) "acme.tool.spin")
    (data (i32.const 368) "Spin")
    (data (i32.const 384) "acme.tool.invoke")
    (data (i32.const 400) "Invoke")
    (data (i32.const 416) "test.read")
    (data (i32.const 432) "{}")
    (data (i32.const 448) "acme.tool.early")

    ;; Registers `acme.tool.early` while the component is instantiated, before `init`, when a
    ;; test makes it the start function.
    (func $early
      (call $register
        (i32.const 448) (i32.const 15) (i32.const 272) (i32.const 5)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 64)))
    ;; (start $early)

    ;; Whether the import's `result` at 64 is an error.
    (func $failed (result i32)
      (i32.ne (i32.load8_u (i32.const 64)) (i32.const 0)))

    (func (export "init") (result i32)
      ;; hello: args-schema and when some, mutates-buffer false.
      (call $register
        (i32.const 256) (i32.const 15) (i32.const 272) (i32.const 5)
        (i32.const 1) (i32.const 288) (i32.const 17)
        (i32.const 1) (i32.const 320) (i32.const 20)
        (i32.const 0) (i32.const 64))
      (if (call $failed) (then (return (i32.const 64))))
      ;; spin: args-schema and when none, mutates-buffer false.
      (call $register
        (i32.const 352) (i32.const 14) (i32.const 368) (i32.const 4)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 64))
      (if (call $failed) (then (return (i32.const 64))))
      ;; invoke: args-schema and when none, mutates-buffer true.
      (call $register
        (i32.const 384) (i32.const 16) (i32.const 400) (i32.const 6)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 1) (i32.const 64))
      (if (call $failed) (then (return (i32.const 64))))
      (i32.const 0))
    (func (export "shutdown"))

    (func (export "run-command") (param $id i32) (param $id-len i32) (param $args i32) (param $args-len i32) (result i32)
      ;; acme.tool.spin
      (if (i32.eq (local.get $id-len) (i32.const 14))
        (then (loop $forever (br $forever))))
      ;; acme.tool.invoke
      (if (i32.eq (local.get $id-len) (i32.const 16))
        (then
          (call $invoke (i32.const 416) (i32.const 9) (i32.const 432) (i32.const 2) (i32.const 64))
          (return (i32.const 64))))
      ;; Level `info`.
      (call $log (i32.const 2) (local.get $id) (local.get $id-len))
      (call $log (i32.const 2) (local.get $args) (local.get $args-len))
      (i32.const 0))
    (func (export "handle-events") (param i32 i32))

    (func (export "try-register") (param $id i32) (param $id-len i32) (result i32)
      ;; Titled `Hello`, args-schema none, when none, mutates-buffer false.
      (call $register
        (local.get $id) (local.get $id-len) (i32.const 272) (i32.const 5)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 64))
      (i32.const 64))
    (func (export "try-invoke") (param $id i32) (param $id-len i32) (param $args i32) (param $args-len i32) (result i32)
      (call $invoke (local.get $id) (local.get $id-len) (local.get $args) (local.get $args-len) (i32.const 64))
      (i32.const 64)))
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
  (export "kiln:extension/event-handler@1.0.0" (instance $event-handler))

  (func (export "try-register") (param "id" string) (result (result (error $error)))
    (canon lift (core func $main "try-register") (memory $memory) (realloc $realloc)))
  (func (export "try-invoke") (param "id" string) (param "args" string) (result (result (error $error)))
    (canon lift (core func $main "try-invoke") (memory $memory) (realloc $realloc))))
