(component
  (core module $m
    (func (export "spin") (result i32)
      (local $i i32)
      (loop $l
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        local.get $i
        i32.const 1000000
        i32.lt_s
        br_if $l)
      local.get $i)
  )
  (core instance $i (instantiate $m))
  (func $spin (result s32) (canon lift (core func $i "spin")))
  (instance $api (export "spin" (func $spin)))
  (export "example:test/spin@1.0.0" (instance $api))
)
