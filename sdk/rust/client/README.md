# anth-client

The transport primitives (`Transport`, `ClientError`, `query_value`) for
Anthracite apps, plus the client modules generated from `apps/hello` by
`scripts/codegen/generate.ts`. Do not edit the generated sources by hand.

The `anth` CLI does not depend on this crate: its command table is generated on
the engine side. `anth-client` is the library an external tool embeds; the
`hello` modules are the checked-in example of the generator.
