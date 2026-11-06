(component
  (core module $m
    (func (export "max") (result i64) i64.const -1)
    (func (export "echo") (param i64) (result i64) local.get 0)
  )
  (core instance $i (instantiate $m))
  (func $max (result u64) (canon lift (core func $i "max")))
  (func $echo (param "value" u64) (result u64) (canon lift (core func $i "echo")))
  (instance $api (export "max" (func $max)) (export "echo" (func $echo)))
  (export "example:test/wide@1.0.0" (instance $api))
)
