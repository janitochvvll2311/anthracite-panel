(component
  (core module $m
    (func (export "max") (result i64) i64.const -1)
  )
  (core instance $i (instantiate $m))
  (func $max (result u64) (canon lift (core func $i "max")))
  (instance $api (export "max" (func $max)))
  (export "example:test/wide@1.0.0" (instance $api))
)
