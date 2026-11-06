(component
  (import "anth:env/time@1.0.0" (instance $time
    (export "now-ms" (func (result u64)))
    (export "monotonic-ms" (func (result u64)))
  ))
  (alias export $time "now-ms" (func $now))
  (core func $now_lowered (canon lower (func $now)))
  (core module $m
    (import "host" "now" (func $now (result i64)))
    (func (export "run") (result i64) call $now)
  )
  (core instance $i (instantiate $m
    (with "host" (instance (export "now" (func $now_lowered))))
  ))
  (func $run (result u64) (canon lift (core func $i "run")))
  (instance $api (export "run" (func $run)))
  (export "example:test/clock@1.0.0" (instance $api))
)
