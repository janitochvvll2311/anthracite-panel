;; A complete plugin guest, and the ABI's reference implementation.
;;
;; It exists so the host side has something real to run in tests, and so a third
;; party writing a plugin has a working example of all four exports and both
;; imports. It answers one HTTP request by calling one granted capability and
;; returning the program's stdout.
;;
;; Not a build artifact: the host compiles it with `wat` in tests only, so the
;; shipped binary never carries a text-format parser.
(module
  (import "env" "anth_exec" (func $exec (param i32 i32 i32 i32) (result i64)))
  (import "env" "anth_log" (func $log (param i32 i32)))

  (memory (export "memory") 1)
  (global $bump (mut i32) (i32.const 1024))

  ;; Bump allocator. The host uses it to write a request; the guest uses it for
  ;; the capability payload. Never frees, which is fine for a process that
  ;; handles one request, and the reason `anth_abi_version` exists: a host that
  ;; wanted long-lived guests would need a real allocator and a different ABI.
  (func (export "anth_alloc") (param $len i32) (result i32)
    (local $ptr i32)
    (local.set $ptr (global.get $bump))
    (global.set $bump (i32.add (global.get $bump) (local.get $len)))
    (global.set $bump (i32.and (i32.add (global.get $bump) (i32.const 7)) (i32.const -8)))
    (local.get $ptr))

  (func (export "anth_free") (param $ptr i32) (param $len i32)
    ;; No-op: the guest is one-shot.
    (drop (local.get $ptr))
    (drop (local.get $len)))

  (func (export "anth_abi_version") (result i32) (i32.const 1))

  (data (i32.const 0) "{\"interface\":\"wg0\"}")
  (data (i32.const 64) "wg.status")
  (data (i32.const 128) "{\"kind\":\"http\",\"status\":200,\"body\":\"ok\"}")

  ;; Ignores the request bytes on purpose: this guest demonstrates the call
  ;; path, not request parsing. It returns a fixed envelope written at 128.
  (func (export "anth_handle") (param $ptr i32) (param $len i32) (result i64)
    (local $out i64)
    (drop (local.get $ptr))
    (drop (local.get $len))
    (drop (call $exec
      (i32.const 64) (i32.const 9)
      (i32.const 0) (i32.const 19)))
    (call $log (i32.const 64) (i32.const 9))
    (local.set $out (i64.or
      (i64.shl (i64.const 128) (i64.const 32))
      (i64.const 40)))
    (local.get $out))
)
