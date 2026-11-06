(component
  (core module $m
    (func $start (loop $l br $l))
    (start $start)
  )
  (core instance (instantiate $m))
)
