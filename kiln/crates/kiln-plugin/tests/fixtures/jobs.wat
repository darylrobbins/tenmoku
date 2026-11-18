;; SPDX-License-Identifier: Apache-2.0
;;
;; A plugin that runs background jobs, for `tests/jobs.rs`. Its interactive instance's
;; `run-command(id, args)`:
;;
;; - `cancel` cancels the job whose id `args` spells in decimal, and returns what `jobs.cancel`
;;   returned;
;; - `state` logs `jobs.state` of the job `args` spells, as the case's index: `0` queued, `1`
;;   running, `2` succeeded, `3` failed, `4` cancelled, `5` timed-out, `6` unknown;
;; - any other id starts a job named and titled `id`, with `args`, kind `other`, cancellable,
;;   whose root is `args` when that starts with `/`; it returns what `jobs.start` returned,
;;   without the id.
;;
;; Its worker's `run-job` by name (its post-return overwrites the bytes of the string it
;; returned, result or message, with `X`, as a guest freeing it may):
;;
;; - `echo` returns its root if it has one, else its args;
;; - `loop` loops without entering the host;
;; - `spin` loops asking `job-context.cancelled`, and never stops for it;
;; - `big` returns as many zero bytes as its args spell in decimal, 12 MiB without args;
;; - `oops` fails with its args as the message;
;; - `deep` calls itself until the stack overflows;
;; - `who` returns `init ran` if `init` ran in its instance, else `no init`;
;; - `grow` loops on `memory.grow 0`, entering wasmtime's code but no host import;
;; - `huge` returns 20 MiB of zero bytes, and `fat` fails with a 20 MiB message;
;; - `invoke` invokes `x.never` with `{}`, then, whatever that returned, does as `echo` does;
;; - `net` fetches `https://example.com/`, and `process` spawns `sleep` with no arguments, then
;;   each does as `echo` does;
;; - `tardy` waits for `job-context.cancelled`, then does as `invoke` does.
;;
;; `job-finished` logs how the job ended: `o` finished ok, `e` finished with an error, `c`
;; cancelled, `t` timed out; then, for `o` and `e`, the first 64 bytes of the result or the
;; error's message. A job that finished with `chain` starts an `echo` of `chain`, and waits
;; for it to succeed before returning.
(component $C
  (import "kiln:extension/types@1.1.0" (instance $types
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

  (import "kiln:extension/log@1.1.0" (instance $log
    (type $level (enum "trace" "debug" "info" "warn" "error"))
    (export "level" (type $level' (eq $level)))
    (export "log" (func (param "level" $level') (param "message" string)))))

  (import "kiln:extension/events@1.1.0" (instance $events
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

  (import "kiln:extension/jobs@1.1.0" (instance $jobs
    (alias outer $C $error (type $error))
    (type $job-kind (enum "build" "test" "index" "other"))
    (export "job-kind" (type $job-kind' (eq $job-kind)))
    (type $job-spec (record
      (field "name" string)
      (field "args" string)
      (field "title" string)
      (field "kind" $job-kind')
      (field "cancellable" bool)
      (field "root" (option string))))
    (export "job-spec" (type $job-spec' (eq $job-spec)))
    (type $waiting (enum "concurrency" "memory"))
    (export "waiting" (type $waiting' (eq $waiting)))
    (type $progress (record
      (field "fraction" (option f32))
      (field "message" (option string))))
    (export "progress" (type $progress' (eq $progress)))
    (type $job-status (variant
      (case "queued" $waiting')
      (case "running" $progress')
      (case "succeeded")
      (case "failed")
      (case "cancelled")
      (case "timed-out")
      (case "unknown")))
    (export "job-status" (type $job-status' (eq $job-status)))
    (type $job-state (variant
      (case "finished" (result string (error $error)))
      (case "cancelled")
      (case "timed-out")))
    (export "job-state" (type (eq $job-state)))
    (export "start" (func (param "spec" $job-spec') (result (result u64 (error $error)))))
    (export "cancel" (func (param "job" u64) (result (result (error $error)))))
    (export "state" (func (param "job" u64) (result $job-status')))))
  (alias export $jobs "job-state" (type $job-state))

  (import "kiln:extension/commands@1.1.0" (instance $commands
    (alias outer $C $error (type $error))
    (export "invoke" (func (param "id" string) (param "args" string) (result (result (error $error)))))))

  (import "kiln:extension/net@1.1.0" (instance $net
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

  (import "kiln:extension/process@1.1.0" (instance $process
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

  (import "kiln:extension/job-context@1.1.0" (instance $job-context
    (export "cancelled" (func (result bool)))))

  (core module $libc
    (memory (export "memory") 1)
    ;; Bytes 0..8 stay zero: a `result` that is `ok`. 64..88 hold `start`'s result, 96..120
    ;; `state`'s, 128..144 `run-command`'s error, 160..176 `run-job`'s result, 200 a logged
    ;; character, 208 whether `init` ran; strings start at 256, and 400..432 and 432..464 hold
    ;; `fetch`'s and `spawn`'s results; the heap starts at 1024, and
    ;; grows the memory as it needs.
    (global $heap (mut i32) (i32.const 1024))
    (func (export "realloc") (param i32 i32) (param $align i32) (param $size i32) (result i32)
      (local $p i32)
      (local $end i32)
      (local $have i32)
      (local.set $p
        (i32.and
          (i32.add (global.get $heap) (i32.sub (local.get $align) (i32.const 1)))
          (i32.sub (i32.const 0) (local.get $align))))
      (local.set $end (i32.add (local.get $p) (local.get $size)))
      (local.set $have (i32.mul (memory.size) (i32.const 65536)))
      (if (i32.gt_u (local.get $end) (local.get $have))
        (then
          (if (i32.eq
                (memory.grow
                  (i32.shr_u
                    (i32.add (i32.sub (local.get $end) (local.get $have)) (i32.const 65535))
                    (i32.const 16)))
                (i32.const -1))
            (then unreachable))))
      (global.set $heap (local.get $end))
      (local.get $p)))
  (core instance $libc (instantiate $libc))
  (alias core export $libc "memory" (core memory $memory))
  (alias core export $libc "realloc" (core func $realloc))

  (alias export $log "log" (func $log-log))
  (alias export $jobs "start" (func $jobs-start))
  (alias export $jobs "cancel" (func $jobs-cancel))
  (alias export $jobs "state" (func $jobs-state))
  (alias export $job-context "cancelled" (func $context-cancelled))
  (alias export $commands "invoke" (func $commands-invoke))
  (alias export $net "fetch" (func $net-fetch))
  (alias export $process "spawn" (func $process-spawn))
  (core func $log (canon lower (func $log-log) (memory $memory)))
  (core func $start (canon lower (func $jobs-start) (memory $memory) (realloc $realloc)))
  (core func $cancel (canon lower (func $jobs-cancel) (memory $memory) (realloc $realloc)))
  (core func $state (canon lower (func $jobs-state) (memory $memory) (realloc $realloc)))
  (core func $cancelled (canon lower (func $context-cancelled)))
  (core func $invoke (canon lower (func $commands-invoke) (memory $memory) (realloc $realloc)))
  (core func $fetch (canon lower (func $net-fetch) (memory $memory) (realloc $realloc)))
  (core func $spawn (canon lower (func $process-spawn) (memory $memory) (realloc $realloc)))
  (core instance $host
    (export "log" (func $log))
    (export "start" (func $start))
    (export "cancel" (func $cancel))
    (export "state" (func $state))
    (export "cancelled" (func $cancelled))
    (export "invoke" (func $invoke))
    (export "fetch" (func $fetch))
    (export "spawn" (func $spawn)))

  (core module $main
    (import "libc" "memory" (memory 1))
    (import "host" "log" (func $log (param i32 i32 i32)))
    (import "host" "start" (func $start (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)))
    (import "host" "cancel" (func $cancel (param i64 i32)))
    (import "host" "state" (func $state (param i64 i32)))
    (import "host" "cancelled" (func $cancelled (result i32)))
    (import "host" "invoke" (func $invoke (param i32 i32 i32 i32 i32)))
    (import "host" "fetch" (func $fetch (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)))
    (import "host" "spawn" (func $spawn (param i32 i32 i32 i32 i32 i32 i32 i32)))

    ;; `args`, spelling a job id in decimal.
    (func $parse (param $p i32) (param $n i32) (result i64)
      (local $id i64)
      (block $done
        (loop $next
          (br_if $done (i32.eqz (local.get $n)))
          (local.set $id
            (i64.add
              (i64.mul (local.get $id) (i64.const 10))
              (i64.extend_i32_u (i32.sub (i32.load8_u (local.get $p)) (i32.const 48)))))
          (local.set $p (i32.add (local.get $p) (i32.const 1)))
          (local.set $n (i32.sub (local.get $n) (i32.const 1)))
          (br $next)))
      (local.get $id))

    ;; Logs the character `c` at level `info`.
    (func $say (param $c i32)
      (i32.store8 (i32.const 200) (local.get $c))
      (call $log (i32.const 2) (i32.const 200) (i32.const 1)))

    ;; Logs at most 64 bytes of a string at level `info`.
    (func $say-text (param $p i32) (param $n i32)
      (if (i32.gt_u (local.get $n) (i32.const 64)) (then (local.set $n (i32.const 64))))
      (call $log (i32.const 2) (local.get $p) (local.get $n)))

    (data (i32.const 256) "init ran")
    (data (i32.const 272) "no init")
    (data (i32.const 288) "x.never")
    (data (i32.const 296) "{}")
    (data (i32.const 304) "echo")
    (data (i32.const 312) "chain")
    (data (i32.const 336) "https://example.com/")
    (data (i32.const 360) "sleep")

    ;; Invokes `x.never` once; its result goes to 176.
    (func $invoke-once
      (call $invoke (i32.const 288) (i32.const 7) (i32.const 296) (i32.const 2) (i32.const 176)))

    ;; 160 as `ok` or `err(failed)` of `n` new zero bytes.
    (func $grown (param $failed i32) (param $n i32) (result i32)
      (local $pages i32)
      (local.set $pages
        (memory.grow (i32.shr_u (i32.add (local.get $n) (i32.const 65535)) (i32.const 16))))
      (if (i32.eq (local.get $pages) (i32.const -1)) (then unreachable))
      (if (local.get $failed)
        (then
          (i32.store8 (i32.const 160) (i32.const 1))
          (i32.store8 (i32.const 164) (i32.const 5))
          (i32.store (i32.const 168) (i32.mul (local.get $pages) (i32.const 65536)))
          (i32.store (i32.const 172) (local.get $n)))
        (else
          (i32.store (i32.const 164) (i32.mul (local.get $pages) (i32.const 65536)))
          (i32.store (i32.const 168) (local.get $n))))
      (i32.const 160))

    (func (export "init") (result i32)
      (i32.store8 (i32.const 208) (i32.const 1))
      (i32.const 0))
    (func (export "shutdown"))
    (func (export "handle-events") (param i32 i32))

    (func (export "run-command") (param $id i32) (param $id-len i32) (param $args i32) (param $args-len i32) (result i32)
      (local $rooted i32)
      ;; cancel
      (if (i32.and
            (i32.eq (local.get $id-len) (i32.const 6))
            (i32.eq (i32.load8_u (local.get $id)) (i32.const 99)))
        (then
          (call $cancel (call $parse (local.get $args) (local.get $args-len)) (i32.const 128))
          (return (i32.const 128))))
      ;; state
      (if (i32.and
            (i32.eq (local.get $id-len) (i32.const 5))
            (i32.eq (i32.load8_u (local.get $id)) (i32.const 115)))
        (then
          (call $state (call $parse (local.get $args) (local.get $args-len)) (i32.const 96))
          (call $say (i32.add (i32.load8_u (i32.const 96)) (i32.const 48)))
          (return (i32.const 0))))
      ;; Any other: start it, rooted at `args` when that starts with `/`.
      (local.set $rooted
        (i32.and
          (i32.ne (local.get $args-len) (i32.const 0))
          (i32.eq (i32.load8_u (local.get $args)) (i32.const 47))))
      (call $start
        (local.get $id) (local.get $id-len)
        (local.get $args) (local.get $args-len)
        (local.get $id) (local.get $id-len)
        (i32.const 3) (i32.const 1)
        (local.get $rooted) (local.get $args) (local.get $args-len)
        (i32.const 64))
      (if (i32.eqz (i32.load8_u (i32.const 64))) (then (return (i32.const 0))))
      ;; `start`'s error is at 72; `run-command`'s goes at 132.
      (i32.store8 (i32.const 128) (i32.const 1))
      (i32.store8 (i32.const 132) (i32.load8_u (i32.const 72)))
      (i32.store (i32.const 136) (i32.load (i32.const 76)))
      (i32.store (i32.const 140) (i32.load (i32.const 80)))
      (i32.const 128))

    (func (export "run-job")
      (param $job i64) (param $name i32) (param $name-len i32) (param $args i32) (param $args-len i32)
      (param $rooted i32) (param $root i32) (param $root-len i32)
      (result i32)
      (local $first i32)
      (local $pages i32)
      (local.set $first (i32.load8_u (local.get $name)))
      ;; loop
      (if (i32.eq (local.get $first) (i32.const 108))
        (then (loop $forever (br $forever))))
      ;; spin
      (if (i32.eq (local.get $first) (i32.const 115))
        (then (loop $forever (drop (call $cancelled)) (br $forever))))
      (i32.store8 (i32.const 160) (i32.const 0))
      ;; big
      (if (i32.eq (local.get $first) (i32.const 98))
        (then
          (if (i32.eqz (local.get $args-len))
            (then (return (call $grown (i32.const 0) (i32.const 12582912)))))
          (return (call $grown
            (i32.const 0)
            (i32.wrap_i64 (call $parse (local.get $args) (local.get $args-len)))))))
      ;; oops
      (if (i32.eq (local.get $first) (i32.const 111))
        (then
          (i32.store8 (i32.const 160) (i32.const 1))
          (i32.store8 (i32.const 164) (i32.const 5))
          (i32.store (i32.const 168) (local.get $args))
          (i32.store (i32.const 172) (local.get $args-len))
          (return (i32.const 160))))
      ;; deep
      (if (i32.eq (local.get $first) (i32.const 100))
        (then (call $deep)))
      ;; grow
      (if (i32.eq (local.get $first) (i32.const 103))
        (then (loop $forever (drop (memory.grow (i32.const 0))) (br $forever))))
      ;; huge, fat
      (if (i32.eq (local.get $first) (i32.const 104))
        (then (return (call $grown (i32.const 0) (i32.const 20971520)))))
      (if (i32.eq (local.get $first) (i32.const 102))
        (then (return (call $grown (i32.const 1) (i32.const 20971520)))))
      ;; invoke
      (if (i32.eq (local.get $first) (i32.const 105))
        (then (call $invoke-once)))
      ;; net: a GET, no headers, body or timeout; its result goes to 400.
      (if (i32.eq (local.get $first) (i32.const 110))
        (then
          (call $fetch
            (i32.const 0) (i32.const 336) (i32.const 20)
            (i32.const 0) (i32.const 0)
            (i32.const 0) (i32.const 0) (i32.const 0)
            (i32.const 0) (i32.const 0)
            (i32.const 400))))
      ;; process: no arguments or stdin; its result goes to 432.
      (if (i32.eq (local.get $first) (i32.const 112))
        (then
          (call $spawn
            (i32.const 360) (i32.const 5)
            (i32.const 0) (i32.const 0)
            (i32.const 0) (i32.const 0) (i32.const 0)
            (i32.const 432))))
      ;; tardy
      (if (i32.eq (local.get $first) (i32.const 116))
        (then
          (block $cancelled (loop $waiting (br_if $cancelled (call $cancelled)) (br $waiting)))
          (call $invoke-once)))
      ;; who
      (if (i32.eq (local.get $first) (i32.const 119))
        (then
          (if (i32.load8_u (i32.const 208))
            (then
              (i32.store (i32.const 164) (i32.const 256))
              (i32.store (i32.const 168) (i32.const 8)))
            (else
              (i32.store (i32.const 164) (i32.const 272))
              (i32.store (i32.const 168) (i32.const 7))))
          (return (i32.const 160))))
      ;; echo
      (if (local.get $rooted)
        (then
          (i32.store (i32.const 164) (local.get $root))
          (i32.store (i32.const 168) (local.get $root-len)))
        (else
          (i32.store (i32.const 164) (local.get $args))
          (i32.store (i32.const 168) (local.get $args-len))))
      (i32.const 160))

    ;; Starts an `echo` of `chain` and waits until `jobs.state` says it succeeded.
    (func $chain
      (local $job i64)
      (call $start
        (i32.const 304) (i32.const 4)
        (i32.const 312) (i32.const 5)
        (i32.const 304) (i32.const 4)
        (i32.const 3) (i32.const 1)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 64))
      (if (i32.load8_u (i32.const 64)) (then (return)))
      (local.set $job (i64.load (i32.const 72)))
      (block $succeeded
        (loop $waiting
          (call $state (local.get $job) (i32.const 96))
          (br_if $succeeded (i32.eq (i32.load8_u (i32.const 96)) (i32.const 2)))
          (br $waiting))))

    ;; Calls itself until the stack overflows.
    (func $deep (call $deep))

    ;; `run-job`'s post-return: `X` over the bytes of the string at `ret`, the result or the
    ;; error's message.
    (func (export "post-run-job") (param $ret i32)
      (if (i32.eqz (i32.load8_u (local.get $ret)))
        (then
          (memory.fill
            (i32.load offset=4 (local.get $ret)) (i32.const 88) (i32.load offset=8 (local.get $ret))))
        (else
          (memory.fill
            (i32.load offset=8 (local.get $ret)) (i32.const 88) (i32.load offset=12 (local.get $ret))))))

    (func (export "job-finished")
      (param $job i64) (param $case i32) (param $ok i32) (param $a i32) (param $b i32) (param $c i32)
      ;; cancelled
      (if (i32.eq (local.get $case) (i32.const 1)) (then (call $say (i32.const 99)) (return)))
      ;; timed-out
      (if (i32.eq (local.get $case) (i32.const 2)) (then (call $say (i32.const 116)) (return)))
      ;; finished: ok(string at a, b), or err(error case a, message at b, c).
      (if (i32.eqz (local.get $ok))
        (then
          (call $say (i32.const 111))
          (call $say-text (local.get $a) (local.get $b))
          (if (i32.and
                (i32.eq (local.get $b) (i32.const 5))
                (i32.eq (i32.load8_u (local.get $a)) (i32.const 99)))
            (then (call $chain))))
        (else
          (call $say (i32.const 101))
          (call $say-text (local.get $b) (local.get $c))))))
  (core instance $main (instantiate $main
    (with "libc" (instance $libc))
    (with "host" (instance $host))))

  (alias core export $main "post-run-job" (core func $post-run-job))

  (func (export "init") (result (result (error $error)))
    (canon lift (core func $main "init") (memory $memory)))
  (func (export "shutdown")
    (canon lift (core func $main "shutdown")))

  (func $run-command (param "id" string) (param "args" string) (result (result (error $error)))
    (canon lift (core func $main "run-command") (memory $memory) (realloc $realloc)))
  (instance $command-handler (export "run-command" (func $run-command)))
  (export "kiln:extension/command-handler@1.1.0" (instance $command-handler))

  (func $handle-events (param "events" (list $event))
    (canon lift (core func $main "handle-events") (memory $memory) (realloc $realloc)))
  (instance $event-handler (export "handle-events" (func $handle-events)))
  (export "kiln:extension/event-handler@1.1.0" (instance $event-handler))

  (func $run-job (param "job" u64) (param "name" string) (param "args" string) (param "root" (option string))
    (result (result string (error $error)))
    (canon lift (core func $main "run-job") (memory $memory) (realloc $realloc)
      (post-return $post-run-job)))
  (instance $job-runner (export "run-job" (func $run-job)))
  (export "kiln:extension/job-runner@1.1.0" (instance $job-runner))

  (func $job-finished (param "job" u64) (param "state" $job-state)
    (canon lift (core func $main "job-finished") (memory $memory) (realloc $realloc)))
  (instance $job-handler (export "job-finished" (func $job-finished)))
  (export "kiln:extension/job-handler@1.1.0" (instance $job-handler)))
