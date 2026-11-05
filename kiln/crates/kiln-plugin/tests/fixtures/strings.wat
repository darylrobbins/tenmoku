;; SPDX-License-Identifier: Apache-2.0
;;
;; Passes strings both ways across the kiln:extension@1.0.0 boundary, the pattern later
;; fixtures copy. A `$libc` core module owns the memory and a bump `realloc`; it is
;; instantiated first so the host imports can be lowered against its memory, and `$main`
;; imports both. `init` logs a fixed non-ASCII message. Each `try-*` export calls one
;; stubbed host import and hands the `error` it got back to the host, so the host sees the
;; error value the guest saw, string and all; an `ok` traps.
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
    (alias outer $C $error (type $error))
    (type $event-kinds (flags
      "buffer-opened" "buffer-changed" "buffer-saved" "buffer-closed"
      "focus-changed" "selection-changed" "config-reloaded"))
    (export "event-kinds" (type $event-kinds' (eq $event-kinds)))
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
    (export "event" (type (eq $event)))
    (export "subscribe" (func (param "kinds" $event-kinds') (result (result (error $error)))))))
  (alias export $events "event" (type $event))

  (import "kiln:extension/process@1.0.0" (instance $process
    (alias outer $C $error (type $error))
    (type $output (record
      (field "status" (option s32))
      (field "stdout" (list u8))
      (field "stderr" (list u8))))
    (export "output" (type $output' (eq $output)))
    (export "spawn" (func
      (param "program" string)
      (param "args" (list string))
      (param "stdin" (option (list u8)))
      (result (result $output' (error $error)))))))

  (import "kiln:extension/net@1.0.0" (instance $net
    (alias outer $C $error (type $error))
    (type $method (enum "get" "head" "post" "put" "patch" "delete"))
    (export "method" (type $method' (eq $method)))
    (type $request (record
      (field "method" $method')
      (field "url" string)
      (field "headers" (list (tuple string string)))
      (field "body" (option (list u8)))
      (field "timeout-ms" (option u32))))
    (export "request" (type $request' (eq $request)))
    (type $response (record
      (field "status" u16)
      (field "headers" (list (tuple string string)))
      (field "body" (list u8))))
    (export "response" (type $response' (eq $response)))
    (export "fetch" (func (param "request" $request') (result (result $response' (error $error)))))))

  (import "kiln:extension/secrets@1.0.0" (instance $secrets
    (alias outer $C $error (type $error))
    (export "get" (func (param "name" string) (result (result (option string) (error $error)))))
    (export "set" (func (param "name" string) (param "value" string) (result (result (error $error)))))
    (export "delete" (func (param "name" string) (result (result (error $error)))))))

  (core module $libc
    (memory (export "memory") 1)
    ;; Bytes 0..8 stay zero: a `result` that is `ok`. 64..128 hold import results;
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
  (alias export $events "subscribe" (func $events-subscribe))
  (alias export $process "spawn" (func $process-spawn))
  (alias export $net "fetch" (func $net-fetch))
  (alias export $secrets "get" (func $secrets-get))
  (alias export $secrets "set" (func $secrets-set))
  (alias export $secrets "delete" (func $secrets-delete))
  (core func $log (canon lower (func $log-log) (memory $memory)))
  (core func $register (canon lower (func $commands-register) (memory $memory) (realloc $realloc)))
  (core func $invoke (canon lower (func $commands-invoke) (memory $memory) (realloc $realloc)))
  (core func $subscribe (canon lower (func $events-subscribe) (memory $memory) (realloc $realloc)))
  (core func $spawn (canon lower (func $process-spawn) (memory $memory) (realloc $realloc)))
  (core func $fetch (canon lower (func $net-fetch) (memory $memory) (realloc $realloc)))
  (core func $get (canon lower (func $secrets-get) (memory $memory) (realloc $realloc)))
  (core func $set (canon lower (func $secrets-set) (memory $memory) (realloc $realloc)))
  (core func $delete (canon lower (func $secrets-delete) (memory $memory) (realloc $realloc)))
  (core instance $host
    (export "log" (func $log))
    (export "register" (func $register))
    (export "invoke" (func $invoke))
    (export "subscribe" (func $subscribe))
    (export "spawn" (func $spawn))
    (export "fetch" (func $fetch))
    (export "get" (func $get))
    (export "set" (func $set))
    (export "delete" (func $delete)))

  (core module $main
    (import "libc" "memory" (memory 1))
    (import "host" "log" (func $log (param i32 i32 i32)))
    (import "host" "register" (func $register (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)))
    (import "host" "invoke" (func $invoke (param i32 i32 i32 i32 i32)))
    (import "host" "subscribe" (func $subscribe (param i32 i32)))
    (import "host" "spawn" (func $spawn (param i32 i32 i32 i32 i32 i32 i32 i32)))
    (import "host" "fetch" (func $fetch (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)))
    (import "host" "get" (func $get (param i32 i32 i32)))
    (import "host" "set" (func $set (param i32 i32 i32 i32 i32)))
    (import "host" "delete" (func $delete (param i32 i32 i32)))

    (data (i32.const 256) "strings cross: h\c3\a9llo, w\c3\b6rld \e2\9c\93 \e6\97\a5\e6\9c\ac\e8\aa\9e")
    (data (i32.const 512) "fixture.hello")
    (data (i32.const 528) "Hello")
    (data (i32.const 544) "{}")
    (data (i32.const 560) "git")
    (data (i32.const 576) "https://example.com")
    (data (i32.const 608) "token")
    (data (i32.const 624) "value")

    ;; The `error` in the `result` an import wrote at 64; traps on `ok`. Every result
    ;; here has its payload at 4: `error` aligns to 4 and no ok type aligns wider.
    (func $err (result i32)
      (if (i32.ne (i32.load8_u (i32.const 64)) (i32.const 1)) (then unreachable))
      (i32.const 68))

    (func (export "init") (result i32)
      ;; Level `info`, then the 43-byte message.
      (call $log (i32.const 2) (i32.const 256) (i32.const 43))
      (i32.const 0))
    (func (export "shutdown"))
    (func (export "run-command") (param i32 i32 i32 i32) (result i32) (i32.const 0))
    (func (export "handle-events") (param i32 i32))

    (func (export "try-register") (result i32)
      ;; id, title, args-schema none, when none, mutates-buffer false.
      (call $register
        (i32.const 512) (i32.const 13) (i32.const 528) (i32.const 5)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 64))
      (call $err))
    (func (export "try-invoke") (result i32)
      (call $invoke (i32.const 512) (i32.const 13) (i32.const 544) (i32.const 2) (i32.const 64))
      (call $err))
    (func (export "try-subscribe") (result i32)
      (call $subscribe (i32.const 1) (i32.const 64))
      (call $err))
    (func (export "try-spawn") (result i32)
      ;; `git` with no args and stdin none.
      (call $spawn
        (i32.const 560) (i32.const 3) (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 64))
      (call $err))
    (func (export "try-fetch") (result i32)
      ;; GET the url, no headers, body none, timeout none.
      (call $fetch
        (i32.const 0) (i32.const 576) (i32.const 19) (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 0) (i32.const 64))
      (call $err))
    (func (export "try-get") (result i32)
      (call $get (i32.const 608) (i32.const 5) (i32.const 64))
      (call $err))
    (func (export "try-set") (result i32)
      (call $set (i32.const 608) (i32.const 5) (i32.const 624) (i32.const 5) (i32.const 64))
      (call $err))
    (func (export "try-delete") (result i32)
      (call $delete (i32.const 608) (i32.const 5) (i32.const 64))
      (call $err)))
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

  (func (export "try-register") (result $error) (canon lift (core func $main "try-register") (memory $memory)))
  (func (export "try-invoke") (result $error) (canon lift (core func $main "try-invoke") (memory $memory)))
  (func (export "try-subscribe") (result $error) (canon lift (core func $main "try-subscribe") (memory $memory)))
  (func (export "try-spawn") (result $error) (canon lift (core func $main "try-spawn") (memory $memory)))
  (func (export "try-fetch") (result $error) (canon lift (core func $main "try-fetch") (memory $memory)))
  (func (export "try-get") (result $error) (canon lift (core func $main "try-get") (memory $memory)))
  (func (export "try-set") (result $error) (canon lift (core func $main "try-set") (memory $memory)))
  (func (export "try-delete") (result $error) (canon lift (core func $main "try-delete") (memory $memory))))
