;; The core modules of kiln/crates/kiln-plugin/tests/fixtures/hostile/loop_call.wat,
;; out of the (component …) that holds them; then a module and script written
;; for the sample.

(module $libc
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

(module $main
  (import "libc" "memory" (memory 1))
  (import "host" "log" (func $log (param i32 i32 i32)))
  (data (i32.const 256) "looping")
  (func (export "init") (result i32) (i32.const 0))
  (func (export "shutdown"))
  (func (export "run-command") (param i32 i32 i32 i32) (result i32)
    ;; Level `info`: tells the host the loop is about to start.
    (call $log (i32.const 2) (i32.const 256) (i32.const 7))
    (loop $forever (br $forever))
    (i32.const 0))
  (func (export "handle-events") (param i32 i32)))

(; A block comment, (; nested ;) inside another. ;)
(module $counter
  (type $binop (func (param i32 i32) (result i32)))
  (type $unop (func (param f64) (result f64)))
  (import "env" "print" (func $print (param i32)))
  (import "env" "table" (table 2 funcref))
  (import "env" "limit" (global $limit i64))

  (memory $mem (export "mem") 1 4)
  (data (offset (i32.const 16)) "hello\n" "tab\there \u{1F600}")
  (global $count (mut i32) (i32.const 0))
  (global $pi f64 (f64.const 3.14159))
  (global $mask i64 (i64.const 0xff_ff))
  (table $ops 2 funcref)
  (elem (table $ops) (i32.const 0) func $add $sub)

  (func $add (type $binop) (param $a i32) (param $b i32) (result i32)
    local.get $a
    local.get $b
    i32.add)

  (func $sub (type $binop)
    (i32.sub (local.get 0) (local.get 1)))

  (func $half (param $x f64) (result f64)
    (f64.mul (local.get $x) (f64.const 0.5)))

  ;; Flat control flow: block, loop and if with end.
  (func $sum_to (export "sum_to") (param $n i32) (result i32)
    (local $i i32) (local $acc i32)
    block $done
      loop $next
        local.get $i
        local.get $n
        i32.ge_u
        br_if $done
        local.get $acc
        local.get $i
        i32.add
        local.set $acc
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $next
      end
    end
    local.get $acc)

  ;; Folded control flow, memory and an indirect call.
  (func $step (export "step") (param $by i32) (result i32)
    (if (result i32) (i32.eqz (local.get $by))
      (then (global.get $count))
      (else
        (global.set $count (i32.add (global.get $count) (local.get $by)))
        (i32.store offset=4 align=4 (i32.const 0) (global.get $count))
        (call $print (i32.load offset=4 (i32.const 0)))
        (call_indirect (type $binop)
          (global.get $count) (i32.const -1) (i32.const 1)))))

  (func $grow (result i32)
    (drop (memory.grow (i32.const 1)))
    (select (memory.size) (i32.const -1) (i32.const 1)))

  (func $init (@note "runs once, at instantiation")
    (global.set $count (i32.const 0)))

  (func $trap
    unreachable)

  (start $init)
  (export "trap" (func $trap))
  (export "add" (func $add))
  (export "count" (global $count)))

(register "counter" $counter)

;; SIMD lanes, typed references and element segments.
(module $lanes
  (type $cmp (func (param i32) (result i32)))
  (table $fns funcref (elem $first $second))
  (elem $segment func $first $second)
  (elem $legacy (i32.const 0) $first)
  (elem $placed (table $fns) (i32.const 1) $second $first)
  (elem (i32.const 1) $second)
  (func $first (type $cmp) (local.get 0))
  (func $second (type $cmp) (i32.const 2))
  (func $nothing (param $r (ref null $cmp)) (result (ref null $cmp))
    (ref.null $cmp))
  (func $lane (export "lane") (result i32)
    (i32x4.extract_lane 1 (v128.const i32x4 1 2 3 4)))
  (func $signed (result i32)
    (i8x16.extract_lane_s 0 (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15)))
  (func $replace (result v128)
    (f32x4.replace_lane 2 (v128.const f32x4 0.5 1.5 2.5 3.5) (f32.const 9)))
  (func $nan (export "nan") (result f32)
    (@note ;; a line comment in an annotation
      (; and a block one, (; nested ;) ;) "kept")
    (f32.const nan)))

(module $bytes binary "\00asm" "\01\00\00\00")
(module $text quote "(func (export \"f\"))")
(assert_return (invoke $lanes "lane") (i32.const 2))
(assert_return (invoke $lanes "nan") (f32.const nan:canonical))
(assert_return (invoke "ref") (ref.func))
(assert_return (invoke "ref") (ref.extern))
(assert_return (invoke "ref") (ref.null))
(input $other "other.wast")
(output $other "out.wast")
(assert_return (invoke "sum_to" (i32.const 4)) (i32.const 6))
(assert_return (invoke "step" (i32.const 0)) (i32.const 0))
(assert_trap (invoke "trap") "unreachable")
(assert_invalid
  (module (func (result i32) (i64.const 1)))
  "type mismatch")
