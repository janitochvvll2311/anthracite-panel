# Appendix A — the smallest vertical slice

> Part of the [design index](../../DESIGN.md).

What "one app composed over the engine" looks like end to end. It is the M3 demo,
and the shape every later app repeats.

The `anth` commands below come from the SDK, which builds the `anth` binary; the
official distribution ships it beside the panel, and a custom panel can ship the
same one.

```bash
# 1. install (content only; nothing is granted)
anth app install ./notes.zip
#    installed org.example.notes 0.1.0 (1 export, 2 imports, 1 route, 1 slot)
#    not approved: run `anth app approve org.example.notes`

# 2. approve (authority is frozen; risk is computed, not declared)
anth app approve org.example.notes
#    approved org.example.notes 0.1.0 (risk read)
#    export  example:notes/list@0.1.0   wasm
#    import  anthracite:fs/list@1.0.0
#    import  anth:store/kv@1.0.0

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
# removing a provider that others need is a configuration error, not a warning
anth compose --remove org.anth.fs
#    refused: org.example.notes imports anthracite:fs/list@1.0.0 and would be
#             unresolved; remove the consumer first
```

```bash
# a composed app that is not approved degrades instead of blocking the boot
anth app upgrade ./fs.zip         # the authority changed
anth restart
#    the engine is up; 1 app degraded

curl --unix-socket /run/anth/engine.sock \
     http://localhost/api/apps/org.example.notes/notes
#    HTTP/1.1 503 Service Unavailable
#    {"error":{"code":"unavailable",
#              "message":"anthracite:fs/list@1.0.0 is not available",
#              "app":"org.example.notes"}}
```

```bash
# an action that breaks consumers asks first, and obeys when acknowledged
anth app disable org.anth.fs
#    warning: org.anth.fs is required by org.example.notes
#    rerun with --acknowledge dependents to proceed
anth app disable org.anth.fs --acknowledge dependents
#    disabled org.anth.fs (1 dependent broken, acknowledged)
```

```bash
# a component that asks for more than its manifest declares
anth app install ./sneaky.zip
#    refused: the component imports anthracite:fs/write@1.0.0 but the manifest
#             declares only anthracite:fs/list@1.0.0
```
