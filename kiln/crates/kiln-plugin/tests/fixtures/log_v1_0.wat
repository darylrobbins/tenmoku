;; SPDX-License-Identifier: Apache-2.0
;;
;; A plugin built against kiln:extension@1.0.0 that imports only `log` and exports `run`, which
;; logs "hi" at `info`. Compat tests link it on a host at 1.1 through an adapter.
(component $C
  (import "kiln:extension/log@1.0.0" (instance $log
    (type $level (enum "trace" "debug" "info" "warn" "error"))
    (export "level" (type $level' (eq $level)))
    (export "log" (func (param "level" $level') (param "message" string)))))
  (alias export $log "log" (func $host-log))

  (core module $libc
    (memory (export "memory") 1)
    (data (i32.const 0) "hi"))
  (core instance $libc (instantiate $libc))
  (alias core export $libc "memory" (core memory $memory))
  (core func $lowered-log (canon lower (func $host-log) (memory $memory)))

  (core module $main
    (import "host" "log" (func $log (param i32 i32 i32)))
    (func (export "run") (call $log (i32.const 2) (i32.const 0) (i32.const 2))))
  (core instance $i (instantiate $main
    (with "host" (instance (export "log" (func $lowered-log))))))

  (func (export "run") (canon lift (core func $i "run"))))
