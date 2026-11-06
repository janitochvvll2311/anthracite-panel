(component
  (core module $m
    (func (export "run") (result i32)
      (loop $l
        br $l)
      i32.const 0)
  )
  (core instance $i (instantiate $m))
  (func $run (result s32) (canon lift (core func $i "run")))
  (instance $api (export "run" (func $run)))
  (export "example:test/forever@1.0.0" (instance $api))
)
