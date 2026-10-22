//! The wasm host against a real module.
//!
//! The guest is `guest.wat`, compiled here with the `wat` feature so the shipped
//! binary never carries a text-format parser. The command runner is a recorder,
//! so these tests assert which argv the host actually handed to the system — the
//! property the whole design rests on.

use std::cell::RefCell;

use super::*;
use crate::command::{CommandError, CommandOutput, CommandRunner};
use crate::plugins::manifest::{ArgSpec, ArgType, Capability, GrantedCapability, PluginGrant};

struct Recorder {
    calls: RefCell<Vec<(String, Vec<String>)>>,
    output: CommandOutput,
}

impl Recorder {
    fn ok(stdout: &str) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            output: CommandOutput {
                ok: true,
                stdout: stdout.to_string(),
                stderr: String::new(),
            },
        }
    }

    fn calls(&self) -> Vec<(String, Vec<String>)> {
        self.calls.borrow().clone()
    }
}

impl CommandRunner for Recorder {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandError> {
        self.calls.borrow_mut().push((
            program.to_string(),
            args.iter().map(|arg| (*arg).to_string()).collect(),
        ));
        Ok(self.output.clone())
    }

    fn run_with_stdin(
        &self,
        program: &str,
        args: &[&str],
        _input: &str,
    ) -> Result<CommandOutput, CommandError> {
        self.run(program, args)
    }
}

fn guest() -> Vec<u8> {
    wat::parse_str(include_str!("../guest.wat")).expect("the fixture guest must compile")
}

/// A grant carrying one `wg.status` capability, as `install` would freeze it.
fn grant() -> PluginGrant {
    PluginGrant {
        id: "org.anth.test".to_string(),
        version: "0.1.0".to_string(),
        wasm_sha256: String::new(),
        recipes: vec![GrantedCapability::from(&Capability {
            id: "wg.status".to_string(),
            job: None,
            program: Some("wg".to_string()),
            argv: vec![
                "show".to_string(),
                "{interface}".to_string(),
                "dump".to_string(),
            ],
            args: [(
                "interface".to_string(),
                ArgSpec {
                    kind: ArgType::Ifname,
                    optional: false,
                },
            )]
            .into_iter()
            .collect(),
            accepts_text: false,
        })],
        enabled: true,
    }
}

fn request() -> &'static str {
    r#"{"kind":"http","method":"GET","path":"/status","query":{},"body":null}"#
}

#[test]
fn the_fixture_guest_runs_and_calls_its_granted_capability() {
    let runner = Recorder::ok("wg0\tkey\t...\n");
    let reply = invoke(&guest(), "org.anth.test", &grant(), &runner, request(), 0).unwrap();

    assert_eq!(reply.execs, 1);
    assert_eq!(
        runner.calls(),
        vec![(
            "wg".to_string(),
            vec!["show".to_string(), "wg0".to_string(), "dump".to_string()]
        )],
        "the host must run the frozen recipe, in order"
    );
    assert!(reply.body.contains("\"kind\":\"http\""), "{}", reply.body);
}

#[test]
fn the_guest_can_log_and_the_lines_are_collected() {
    let runner = Recorder::ok("");
    let reply = invoke(&guest(), "org.anth.test", &grant(), &runner, request(), 0).unwrap();
    assert_eq!(reply.log, vec!["wg.status".to_string()]);
}

#[test]
fn an_ungranted_capability_is_refused_without_running_anything() {
    let mut empty = grant();
    empty.recipes.clear();

    let runner = Recorder::ok("");
    let reply = invoke(&guest(), "org.anth.test", &empty, &runner, request(), 0).unwrap();

    assert_eq!(reply.execs, 0);
    assert!(runner.calls().is_empty(), "nothing may run");
}

#[test]
fn a_disabled_grant_runs_nothing() {
    let mut disabled = grant();
    disabled.enabled = false;

    let runner = Recorder::ok("");
    let reply = invoke(&guest(), "org.anth.test", &disabled, &runner, request(), 0).unwrap();
    assert_eq!(reply.execs, 0);
    assert!(runner.calls().is_empty());
}

#[test]
fn a_host_job_cannot_be_called_by_a_guest() {
    let mut job = grant();
    job.recipes[0].job = Some("network.list".to_string());
    job.recipes[0].program = None;
    job.recipes[0].argv.clear();

    let runner = Recorder::ok("");
    let reply = invoke(&guest(), "org.anth.test", &job, &runner, request(), 0).unwrap();

    assert_eq!(reply.execs, 0, "a job is the host's own argv");
    assert!(runner.calls().is_empty());
}

#[test]
fn a_narrowed_recipe_refuses_the_argument_it_no_longer_declares() {
    let runner = Recorder::ok("");
    let mut narrow = grant();
    narrow.recipes[0].argv = vec!["show".to_string()];
    narrow.recipes[0].args.clear();

    let reply = invoke(&guest(), "org.anth.test", &narrow, &runner, request(), 0).unwrap();

    // The guest passed `{"interface":"wg0"}`, which the narrowed recipe does not
    // declare, so the call is refused rather than silently dropping the value.
    assert_eq!(reply.execs, 0);
    assert!(runner.calls().is_empty());
}

#[test]
fn a_failed_program_is_reported_to_the_guest_as_a_value() {
    let runner = Recorder {
        calls: RefCell::new(Vec::new()),
        output: CommandOutput {
            ok: false,
            stdout: String::new(),
            stderr: "Unable to modify interface: Operation not permitted".to_string(),
        },
    };

    let reply = invoke(&guest(), "org.anth.test", &grant(), &runner, request(), 0).unwrap();
    assert_eq!(reply.execs, 0, "a non-zero exit is not a successful exec");
    assert!(reply.body.contains("http"));
}

#[test]
fn a_guest_that_loops_is_stopped_by_fuel() {
    let spinning = wat::parse_str(
        r#"(module
             (memory (export "memory") 1)
             (func (export "anth_alloc") (param i32) (result i32) (i32.const 1024))
             (func (export "anth_free") (param i32) (param i32))
             (func (export "anth_abi_version") (result i32) (i32.const 1))
             (func (export "anth_handle") (param i32) (param i32) (result i64)
               (loop $forever (br $forever))
               (i64.const 0)))"#,
    )
    .unwrap();

    let runner = Recorder::ok("");
    let error = invoke(
        &spinning,
        "org.anth.test",
        &grant(),
        &runner,
        request(),
        10_000,
    )
    .unwrap_err();
    assert_eq!(error.kind, GuestErrorKind::OutOfFuel, "{}", error.detail);
}

#[test]
fn a_module_declaring_another_abi_is_refused() {
    let future = wat::parse_str(
        r#"(module
             (memory (export "memory") 1)
             (func (export "anth_alloc") (param i32) (result i32) (i32.const 1024))
             (func (export "anth_free") (param i32) (param i32))
             (func (export "anth_abi_version") (result i32) (i32.const 2))
             (func (export "anth_handle") (param i32) (param i32) (result i64) (i64.const 0)))"#,
    )
    .unwrap();

    let runner = Recorder::ok("");
    let error = invoke(&future, "org.anth.test", &grant(), &runner, request(), 0).unwrap_err();
    assert_eq!(error.kind, GuestErrorKind::Invalid);
    assert!(error.detail.contains("ABI 2"), "{}", error.detail);
}

#[test]
fn a_module_missing_an_export_is_refused_by_name() {
    let incomplete = wat::parse_str(
        r#"(module
             (memory (export "memory") 1)
             (func (export "anth_abi_version") (result i32) (i32.const 1)))"#,
    )
    .unwrap();

    let runner = Recorder::ok("");
    let error = invoke(
        &incomplete,
        "org.anth.test",
        &grant(),
        &runner,
        request(),
        0,
    )
    .unwrap_err();
    assert_eq!(error.kind, GuestErrorKind::Invalid);
    assert!(error.detail.contains("anth_alloc"), "{}", error.detail);
}

#[test]
fn garbage_bytes_are_not_wasm() {
    let runner = Recorder::ok("");
    let error = invoke(
        b"not wasm at all",
        "org.anth.test",
        &grant(),
        &runner,
        request(),
        0,
    )
    .unwrap_err();
    assert_eq!(error.kind, GuestErrorKind::Invalid);
}

#[test]
fn a_guest_cannot_import_what_the_host_does_not_offer() {
    // No WASI, no filesystem: an import the host does not define fails at
    // instantiation rather than at first use.
    let wasi = wat::parse_str(
        r#"(module
             (import "wasi_snapshot_preview1" "fd_write"
               (func $w (param i32 i32 i32 i32) (result i32)))
             (memory (export "memory") 1)
             (func (export "anth_alloc") (param i32) (result i32) (i32.const 1024))
             (func (export "anth_free") (param i32) (param i32))
             (func (export "anth_abi_version") (result i32) (i32.const 1))
             (func (export "anth_handle") (param i32) (param i32) (result i64) (i64.const 0)))"#,
    )
    .unwrap();

    let runner = Recorder::ok("");
    let error = invoke(&wasi, "org.anth.test", &grant(), &runner, request(), 0).unwrap_err();
    assert_eq!(error.kind, GuestErrorKind::Invalid);
    assert!(
        error.detail.contains("wasi") || error.detail.contains("import"),
        "{}",
        error.detail
    );
}

#[test]
fn a_guest_cannot_smuggle_an_option_through_a_value() {
    // The guest writes `{"interface":"--force"}` instead of a name. The validator
    // is the host's, not the plugin's, so the value never reaches argv.
    let sneaky = wat::parse_str(
        r#"(module
             (import "env" "anth_exec" (func $exec (param i32 i32 i32 i32) (result i64)))
             (memory (export "memory") 1)
             (func (export "anth_alloc") (param i32) (result i32) (i32.const 1024))
             (func (export "anth_free") (param i32) (param i32))
             (func (export "anth_abi_version") (result i32) (i32.const 1))
             (data (i32.const 0) "wg.status")
             (data (i32.const 64) "{\"interface\":\"--force\"}")
             (func (export "anth_handle") (param i32) (param i32) (result i64)
               (drop (call $exec (i32.const 0) (i32.const 9) (i32.const 64) (i32.const 23)))
               (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 0))))"#,
    )
    .unwrap();

    let runner = Recorder::ok("");
    let reply = invoke(&sneaky, "org.anth.test", &grant(), &runner, request(), 0).unwrap();

    assert_eq!(reply.execs, 0, "`--force` is an option, not a name");
    assert!(
        runner.calls().is_empty(),
        "the host must not run anything: {:?}",
        runner.calls()
    );
}

#[test]
fn probe_accepts_the_fixture_and_rejects_garbage() {
    assert!(probe(&guest()).is_ok());
    assert!(probe(b"\0asm").is_err());
}

/// A guest that calls `wg.status` in a loop until the host stops it.
fn looping_guest() -> Vec<u8> {
    wat::parse_str(
        r#"(module
             (import "env" "anth_exec" (func $exec (param i32 i32 i32 i32) (result i64)))
             (memory (export "memory") 1)
             (func (export "anth_alloc") (param i32) (result i32) (i32.const 1024))
             (func (export "anth_free") (param i32) (param i32))
             (func (export "anth_abi_version") (result i32) (i32.const 1))
             (data (i32.const 0) "wg.status")
             (data (i32.const 64) "{\"interface\":\"wg0\"}")
             (func (export "anth_handle") (param i32) (param i32) (result i64)
               (local $i i32)
               (block $done
                 (loop $again
                   (br_if $done (i32.ge_u (local.get $i) (i32.const 100000)))
                   (drop (call $exec (i32.const 0) (i32.const 9) (i32.const 64) (i32.const 19)))
                   (local.set $i (i32.add (local.get $i) (i32.const 1)))
                   (br $again)))
               (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 0))))"#,
    )
    .expect("the looping guest must compile")
}

#[test]
fn a_guest_cannot_amplify_one_call_into_unbounded_spawns() {
    // Fuel counts guest instructions, and one `anth_exec` costs a handful of
    // them. Without the ceiling this guest produced 100k subprocesses in 7s and
    // never ran out of fuel.
    let runner = Recorder::ok("");
    let reply = invoke(
        &looping_guest(),
        "org.anth.test",
        &grant(),
        &runner,
        request(),
        DEFAULT_FUEL,
    )
    .unwrap();

    assert_eq!(
        runner.calls().len(),
        MAX_EXECS_PER_INVOCATION as usize,
        "the host must stop spawning at the ceiling"
    );
    assert_eq!(reply.attempts, MAX_EXECS_PER_INVOCATION);
    assert_eq!(reply.execs, MAX_EXECS_PER_INVOCATION);
}

#[test]
fn a_guest_that_floods_the_log_is_cut_off_and_told_so() {
    // The log is a host-side Vec, outside StoreLimits, so it needs its own cap.
    // 1000 lines of 140 bytes is ~137 KiB: past MAX_LOG_BYTES (64 KiB), and the
    // guest is told some lines were dropped rather than the caller printing a
    // log that looks complete.
    let guest = wat::parse_str(
        r#"(module
             (import "env" "anth_log" (func $log (param i32 i32)))
             (memory (export "memory") 1)
             (data (i32.const 16) "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")
             (func (export "anth_alloc") (param i32) (result i32) (i32.const 1024))
             (func (export "anth_free") (param i32) (param i32))
             (func (export "anth_abi_version") (result i32) (i32.const 1))
             (func (export "anth_handle") (param i32) (param i32) (result i64)
               (local $i i32)
               (block $done
                 (loop $again
                   (br_if $done (i32.ge_u (local.get $i) (i32.const 1000)))
                   (call $log (i32.const 16) (i32.const 140))
                   (local.set $i (i32.add (local.get $i) (i32.const 1)))
                   (br $again)))
               (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 0))))"#,
    )
    .unwrap();

    let runner = Recorder::ok("");
    let reply = invoke(&guest, "org.anth.test", &grant(), &runner, request(), 0).unwrap();

    let bytes: usize = reply.log.iter().map(String::len).sum();
    assert!(
        bytes <= MAX_LOG_BYTES,
        "the log held {bytes} bytes, past the {MAX_LOG_BYTES} cap"
    );
    assert!(reply.log_dropped > 0, "the truncation must be reported");
}

#[test]
fn a_failing_program_still_counts_against_the_ceiling() {
    // The counter used to move only after a success, so a guest looping on a
    // capability whose program exits non-zero was bounded by nothing: measured
    // at 502,795 spawns against a promised ceiling of 32. A failing program is
    // also the cheapest thing for a guest to drive.
    let runner = Recorder {
        calls: RefCell::new(Vec::new()),
        output: CommandOutput {
            ok: false,
            stdout: String::new(),
            stderr: "nope".to_string(),
        },
    };

    let reply = invoke(
        &looping_guest(),
        "org.anth.test",
        &grant(),
        &runner,
        request(),
        DEFAULT_FUEL,
    )
    .unwrap();

    assert_eq!(
        runner.calls().len(),
        MAX_EXECS_PER_INVOCATION as usize,
        "a failing program must not buy extra spawns"
    );
    assert_eq!(reply.attempts, MAX_EXECS_PER_INVOCATION);
    assert_eq!(reply.execs, 0, "nothing succeeded, so nothing ran");
}

#[test]
fn the_deadline_traps_the_guest_instead_of_answering_every_call() {
    // A refusal is a value, and the guest answers a refusal by asking again.
    // Returning `Err` here kept the invocation alive for 110 s against a 30 s
    // deadline, spinning on refusals. The deadline has to end the invocation.
    let expiry = std::time::Instant::now() - MAX_INVOCATION;
    let mut engine_config = wasmi::Config::default();
    engine_config.consume_fuel(true);
    let engine = wasmi::Engine::new(&engine_config);
    let module = wasmi::Module::new(&engine, looping_guest()).unwrap();

    let limits = wasmi::StoreLimitsBuilder::new()
        .memory_size(MAX_GUEST_MEMORY)
        .build();
    let runner = Recorder::ok("");
    let grant = grant();
    let mut store = wasmi::Store::new(
        &engine,
        Host {
            plugin: "org.anth.test".to_string(),
            grant: &grant,
            runner: &runner,
            log: Vec::new(),
            log_bytes: 0,
            log_dropped: 0,
            execs: 0,
            attempts: 0,
            // Already past the deadline, so the first `anth_exec` traps.
            deadline: expiry,
            limits,
        },
    );
    store.limiter(|host| &mut host.limits);
    store.set_fuel(DEFAULT_FUEL).unwrap();
    let mut linker = wasmi::Linker::new(&engine);
    define_host_functions(&mut linker).unwrap();
    let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
    let handle: wasmi::TypedFunc<(i32, i32), i64> =
        instance.get_typed_func(&store, "anth_handle").unwrap();

    let error = handle.call(&mut store, (0, 0)).unwrap_err();
    let classified = classify_name("org.anth.test", error);
    assert_eq!(
        classified.kind,
        GuestErrorKind::Deadline,
        "{}",
        classified.detail
    );
    assert!(
        runner.calls().is_empty(),
        "the deadline must stop the first spawn, not the second"
    );
}
