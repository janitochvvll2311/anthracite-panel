(component
  (core module $m
    (func (export "run") (result i32)
      (local $i i32)
      (loop $l
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        local.get $i
        i32.const 300000000
        i32.lt_s
        br_if $l)
      local.get $i)
  )
  (core instance $i (instantiate $m))
  (func $run (result s32) (canon lift (core func $i "run")))
  (instance $api (export "run" (func $run)))
  (export "example:test/slow@1.0.0" (instance $api))
)
