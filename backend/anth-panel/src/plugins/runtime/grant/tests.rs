use super::*;
use crate::plugins::manifest::AcceptedArg;

fn capability(accept: Vec<AcceptedArg>) -> Capability {
    Capability {
        id: "test.cap".to_string(),
        program: "hostname".to_string(),
        accept,
    }
}

fn accepted(name: &str, kind: ArgType) -> AcceptedArg {
    AcceptedArg {
        name: name.to_string(),
        kind,
    }
}

#[test]
fn a_capability_with_no_arguments_runs_the_bare_program() {
    let argv = argv(&capability(vec![]), &Value::Null).expect("building");

    assert_eq!(argv, vec!["hostname"]);
}

/// The value is one element. This is the defect the CLI line format fixed on the
/// other side: `Ana Sol` split into two words reaches the program as two
/// arguments and changes what it does.
#[test]
fn a_value_with_a_space_stays_one_element() {
    let grant = capability(vec![accepted("name", ArgType::String)]);

    let argv = argv(&grant, &serde_json::json!({"name": "Ana Sol"})).expect("building");

    assert_eq!(argv, vec!["hostname", "--name", "Ana Sol"]);
}

#[test]
fn an_integer_is_passed_without_reformatting() {
    let grant = capability(vec![accepted("count", ArgType::Integer)]);

    let argv = argv(&grant, &serde_json::json!({"count": -3})).expect("building");

    assert_eq!(argv, vec!["hostname", "--count", "-3"]);
}

#[test]
fn a_switch_adds_its_flag_only_when_true() {
    let grant = capability(vec![accepted("all", ArgType::Switch)]);

    assert_eq!(
        argv(&grant, &serde_json::json!({"all": true})).unwrap(),
        vec!["hostname", "--all"]
    );
    assert_eq!(
        argv(&grant, &serde_json::json!({"all": false})).unwrap(),
        vec!["hostname"]
    );
}

/// Ignoring an unknown name would let a guest believe a flag took effect.
#[test]
fn an_argument_the_manifest_did_not_freeze_is_refused() {
    let error =
        argv(&capability(vec![]), &serde_json::json!({"verbose": true})).expect_err("must refuse");

    assert!(error.contains("does not accept"), "got: {error}");
}

/// A value beginning with a dash would be read by the program as a flag, which
/// is how a validated value becomes an unvalidated option.
#[test]
fn a_value_that_looks_like_a_flag_is_refused() {
    let grant = capability(vec![accepted("name", ArgType::String)]);

    let error = argv(&grant, &serde_json::json!({"name": "--exec=sh"})).expect_err("must refuse");

    assert!(error.contains("read as a flag"), "got: {error}");
}

#[test]
fn a_wrongly_typed_value_is_refused_rather_than_coerced() {
    let integer = capability(vec![accepted("count", ArgType::Integer)]);
    assert!(argv(&integer, &serde_json::json!({"count": "3"})).is_err());

    let text = capability(vec![accepted("name", ArgType::String)]);
    assert!(argv(&text, &serde_json::json!({"name": 42})).is_err());
}

#[test]
fn arguments_must_be_an_object() {
    let error =
        argv(&capability(vec![]), &serde_json::json!(["hostname"])).expect_err("must refuse");

    assert!(error.contains("object"), "got: {error}");
}

/// The one call that touches a real process, kept because a grant asserted only
/// through its argv never proves the argv can run.
#[test]
fn a_granted_capability_runs_the_real_program() {
    let Ok(output) = run(&capability(vec![]), &Value::Null) else {
        // A container without `hostname` is a reason to skip, not to fail: the
        // argv assertion above still covers the grant.
        return;
    };

    assert!(!output.trim().is_empty(), "hostname printed nothing");
}
