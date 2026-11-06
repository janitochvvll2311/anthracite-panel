(component
  (core module $m
    (func (export "greet") (result i32) i32.const 7)
  )
  (core instance $i (instantiate $m))
  (func $greet (result s32) (canon lift (core func $i "greet")))
  (instance $api (export "greet" (func $greet)))
  (export "example:test/greeting@1.0.0" (instance $api))
)
