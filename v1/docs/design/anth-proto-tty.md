# Making `anth-proto` carry a TTY

Status: implemented on `feat/web-terminal`, using **Option 1** (base64 inside the
existing line protocol), not the Option 4 recommendation recorded below. The
recommendation is left as the design history that led to the final choice.

This document answers one question: what has to change in `anth-proto` for a
terminal to travel over the Unix socket. Everything below was measured against
this repository, not recalled.

## 1. What the current transport actually does to terminal bytes

`anth-proto` frames with `LinesCodec::new()`. Three properties of that codec
decide the answer, and all three are hostile to a TTY. I ran each case through
the real codec rather than reasoning about it:

| Input | `LinesCodec` result | Why it breaks a terminal |
| --- | --- | --- |
| `b"abc\rdef\n"` | `"abc\rdef"` | A lone `\r` is only stripped at end of line, so a progress bar arrives as one line instead of a redraw |
| `b"\r"` | *nothing* | The update is lost until a newline follows — this is exactly what `curl`'s progress bar and `npm install` emit |
| `[0x80, b'\n']` | *error* (`InvalidData`) | Terminal output is bytes, not UTF-8. `str::from_utf8` rejects it and the session dies |
| `[0x1b, b'[', 0xff, b'm', b'\n']` | *error* | One bad byte in an ANSI sequence kills the whole frame |

And one property that is a problem even without a TTY today:
`LinesCodec::new()` sets `max_length: usize::MAX`. A client that opens the
socket and never sends `\n` makes the daemon buffer without bound. That is a
pre-existing bug, cheap to fix, and it becomes urgent once a terminal can drive
that path.

So the answer is not "add some variants". **The line framing itself cannot pass
terminal bytes.** Any option below has to say how it gets around that.

## 2. The constraint nobody can negotiate

`docs/socket-protocol.md` states the contract: newline-delimited JSON, UTF-8,
one complete message per line, `\n` always a frame boundary. That document is
the reason `anth-proto` is a separate crate — it exists so third-party clients
and alternative daemons can interoperate.

Two consequences:

1. **Raw binary cannot be a frame.** Any byte `0x0a` inside a terminal payload
   would be read as a frame boundary, and the UTF-8 requirement rules out half
   of what a program writes to a TTY.
2. **Whatever carries bytes must be text-safe and newline-free.** That leaves
   base64 (or hex, or an escaping scheme) inside the existing line framing.

Measured cost of base64-in-JSON, in release mode, on this machine:

| Payload | Raw | Framed | Throughput |
| --- | --- | --- | --- |
| 4 KiB chunk | 4096 B | 5500 B (**+34%**) | 542 MiB/s |
| 64 KiB chunk | 65536 B | 87420 B (**+33%**) | 486 MiB/s |
| 1 byte (keystroke) | 1 B | **40 B (40x)** | — |

Interpretation, and this is the key finding: **CPU is not the bottleneck.** At
~500 MiB/s, a terminal that moves a few hundred KiB/s while `tail -f` scrolls
uses well under 1% of a core. The real costs are the 33% bandwidth and, above
all, the fixed 40 bytes per keystroke. That matters on a WAN, not on loopback,
and it argues for batching keystrokes rather than for a different transport.

## 3. Options

### Option 1 — Base64 inside the existing line protocol (no framing change)

Keep everything. Add variants that carry opaque bytes as base64 text.

```rust
pub enum Request {
    Echo { message: String },
    HostInfo,
    TerminalOpen(Box<TerminalOpen>),
    TerminalInput(TerminalInput),
    TerminalResize { id: u32, cols: u16, rows: u16 },
    TerminalClose { id: u32 },
}

pub enum Response {
    Ok { message: String },
    Error { detail: String },
    HostInfo(Box<HostInfo>),
    TerminalOpened(TerminalOpened),
    TerminalOutput(TerminalOutput),   // base64 bytes
    TerminalExited { id: u32, status: i32 },
}

pub struct TerminalOutput {
    pub id: u32,
    #[serde(with = "base64_bytes")]
    pub bytes: Vec<u8>,
}
```

- **Protocol doc changes:** add a section saying byte-carrying members are base64
  and that `data` may be a stream (`TerminalOutput`) rather than one reply.
  The framing rules survive intact. This is the only option that keeps the
  document's central promise true.
- **Effort:** low. No codec work, no `Framed` changes, every existing client
  stays correct.
- **Cost:** +33% bandwidth; 40 B per keystroke; and the daemon must read two
  directions concurrently — the current `serve_connection` loop is strictly
  request-then-response and would need restructuring to push `TerminalOutput`
  while also accepting `TerminalInput`.
- **Risk:** the concurrent read/write restructure touches every connection, so a
  bug there breaks `HostInfo` too, not just the terminal.

### Option 2 — Base64 plus binary-epoch handshake (framing change, opt-in)

Same payload as option 1, but a client may ask for a different framing *on its
own connection*, leaving old clients untouched:

```json
{"type":"Upgrade","data":{"to":"binary-v1"}}
```

After this one line the daemon switches that connection's codec to a binary one:
a small header (kind, stream id, length) plus a raw payload. Line-JSON is
negotiated only at the start, so the framing guarantee holds for every client
that does not ask.

- **Protocol doc changes:** a new section defining the handshake and the binary
  frame layout; the "newline-delimited JSON" promise is scoped to "until
  upgraded".
- **Effort:** medium-high. A custom `Decoder`/`Encoder`, a length limit per frame,
  and a documented byte layout that third parties must reimplement. Two framings
  in one codec type.
- **Cost:** removes the 33% tax and the per-keystroke overhead.
- **Risk:** this is a protocol *version*, and the doc's promise becomes
  conditional. Every alternative client now has to implement two codecs to be
  fully compatible.

### Option 3 — Separate socket for binary streams

Keep the JSON socket exactly as it is. Add a second, binary-only Unix socket
(config: `terminal_socket_path`) that carries PTY frames for a session.

- **Protocol doc changes:** none to the existing contract. A new document
  describes the binary socket.
- **Effort:** medium. A second listener, its own authentication by file mode
  (which is genuinely better than a token), and its own tests.
- **Cost:** two sockets, two lifecycles, and the CLI/panel must know which one to
  dial.
- **Risk:** low for compatibility — the JSON contract is untouched. The cost is
  operational surface: another path to configure, another file to permission.

### Option 4 — Don't put the TTY in the protocol at all

The panel opens the PTY itself (option A from the previous design). `anth-proto`
does not change.

- **Protocol doc changes:** none.
- **Effort:** lowest, and confined to `anth-panel`.
- **Cost:** no terminal for third-party clients that speak only to the daemon.
- **Risk:** none to existing users.

## 4. Compatibility: what breaks, measured

I checked the assumption that adding variants is safe, because it is not
automatic with `#[serde(tag, content)]`.

- **Old client, new daemon:** a message the old enum does not know fails to
  parse (`unknown variant 'TerminalOpen', expected 'Echo' or 'HostInfo'`). It
  surfaces as `Response::Error`, which is what the daemon already does for
  unparseable input — acceptable, not silent corruption.
- **`#[serde(other)]` does not rescue it.** I tried: it rejects a variant that
  *has* a payload (`invalid type: map, expected unit variant`). It only tolerates
  a payload-less unknown tag. So forward-compat cannot be purchased with one
  attribute; it needs a deliberate design.
- **New client, old daemon:** `TerminalOpen` is an unknown variant to the old
  daemon, which answers `Error`. A client must treat that as "terminal not
  supported" rather than a crash. That is a documented behaviour, not a shim.

Therefore any variant-adding option should also do two small things while we are
in the file: give `LinesCodec` a real `max_length`, and let the daemon answer
unknown variants with a typed `Error` instead of a parse failure string.

## 5. Recommendation

**Option 4 now, and Option 1 written down as the extension path.**

Reasoning, in order of weight:

1. This branch's goal is a working web terminal. Option 4 delivers it with the
   least new surface and zero risk to the published contract.
2. The previous design already put the PTY behind a `Session` trait precisely so
   the transport could change later. That decision pays off here: moving from
   panel-local PTY to daemon-owned is a new `Session` implementation plus new
   protocol variants, not a rewrite.
3. If and when third-party clients need terminals, **Option 1 is the right
   shape** — it is the only one that keeps "newline-delimited JSON" literally
   true. Option 2 renegotiates the central promise of a published contract,
   which I would not do to save 33% on a loopback socket. Option 3 is the right
   answer only if the PTY must live behind a *privilege* boundary, not a
   transport preference.

If you want the daemon to own the PTY now, say so and I will build Option 1,
including the concurrent read/write restructure in `daemon.rs` — but I would
land it as its own PR ahead of the UI, because it changes the connection loop
that every existing request depends on.

## 6. What I would change in `anth-proto` under the recommendation

Nothing structural. Two fixes that are worth doing regardless, both already
justified above:

- `LinesCodec::new()` → `LinesCodec::new_with_max_length(MAX_LINE)` with a
  documented cap, closing the unbounded-memory path.
- A typed error for unknown variants, so an old daemon tells a new client
  "unknown request" rather than echoing a serde message.

Both are small, testable, and independent of whether a terminal ever moves into
the daemon.
