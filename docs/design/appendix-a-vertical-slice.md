# Appendix A — the smallest vertical slice

> Part of the [design index](../../DESIGN.md).

What "one app composed over the engine" looks like end to end. It is the M3 demo,
and the shape every later app repeats.

The `anth` commands below come from the SDK; `panel/backend` wires them into the
official panel binary, and a custom panel can wire the same ones.

```bash
# 1. install (content only; nothing is granted)
anth app install ./notes.zip
#    installed org.example.notes 0.1.0 (1 export, 2 imports, 1 route, 1 slot)
#    not approved: run `anth app approve org.example.notes`

# 2. approve (authority is frozen; risk is computed, not declared)
anth app approve org.example.notes --accept-read
#    approved org.example.notes 0.1.0 (risk read)
#    export  example:notes/list@1.0.0      wasm
#    import  anthracite:fs/list@1.0.0            -> org.anth.fs
#    import  anth:store/kv@1.0.0           -> @engine

# 3. compose (what runs is data)
anth compose --add org.example.notes

# 4. use it
curl --unix-socket /run/anth/engine.sock \
     http://localhost/api/apps/org.example.notes/notes?dir=/tmp
#    {"entries":[{"name":"…","kind":"dir","size":4096,"modified-ms":…}]}

anth app notes list --dir /tmp
#    same data, same types, another rendering
```

And the failures the platform must produce, not crash on:

```bash
# a dependency left the composition
anth compose --remove org.anth.fs
#    warning: org.example.notes is degraded: anthracite:fs/list@1.0.0 has no provider
#    the engine is up; 1 app degraded

curl --unix-socket /run/anth/engine.sock \
     http://localhost/api/apps/org.example.notes/notes
#    HTTP/1.1 503 Service Unavailable
#    {"error":"unavailable","message":"anthracite:fs/list@1.0.0 has no provider in this composition","app":"org.example.notes"}
```

```bash
# a component that asks for more than it was granted
anth app install ./sneaky.zip
#    refused: the component imports anthracite:fs/write@1.0.0 but the grant lists
#             only anthracite:fs/list@1.0.0
```
