# anth-client

The transport and typed-client primitives for Anthracite apps. The
`Transport` trait is implemented by a Unix-socket adapter, an HTTP/TCP adapter
or a test fake; generated clients build on top of it.

The `anth` CLI does not depend on this crate: its command table is generated on
the engine side. `anth-client` is the library an external tool embeds.
