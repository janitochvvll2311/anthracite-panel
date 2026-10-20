//! Guard matrix for account mutations.
//!
//! These exercise the pure planning functions, which is where every security
//! property lives: each test asserts the exact argument vector that would reach
//! the system tools, or the exact error that stops it. No test runs `useradd`.

use std::path::Path;

use anth_panel::accounts_admin::{
    CreateArgs, Guard, ModifyArgs, PlannedCall, plan_create, plan_delete, plan_group_create,
    plan_group_delete, plan_group_modify, plan_modify, plan_set_password, validate_gecos,
    validate_home, validate_name, validate_shell,
};
use anth_panel::config::AccountsConfig;
use anth_proto::{AccountErrorKind, AccountGroup, AccountSnapshot, AccountSource, AccountUser};

fn user(name: &str, uid: u32) -> AccountUser {
    AccountUser {
        name: name.to_string(),
        uid,
        gid: uid,
        group: name.to_string(),
        gecos: String::new(),
        home: format!("/home/{name}"),
        shell: "/bin/sh".to_string(),
        locked: Some(true),
        system: false,
        source: AccountSource::Local,
        groups: Vec::new(),
    }
}

fn group(name: &str, gid: u32, members: &[&str]) -> AccountGroup {
    AccountGroup {
        name: name.to_string(),
        gid,
        members: members.iter().map(|member| member.to_string()).collect(),
        system: false,
        source: AccountSource::Local,
    }
}

fn snapshot() -> AccountSnapshot {
    AccountSnapshot {
        users: vec![
            AccountUser {
                system: true,
                ..user("root", 0)
            },
            AccountUser {
                system: true,
                ..user("daemon", 1)
            },
            user("alice", 1001),
            user("bob", 1002),
            AccountUser {
                source: AccountSource::Nss,
                ..user("domain", 2001)
            },
        ],
        groups: vec![
            AccountGroup {
                system: true,
                ..group("root", 0, &[])
            },
            group("sudo", 27, &["alice"]),
            group("wheel", 10, &[]),
            group("alice", 1001, &[]),
            group("bob", 1002, &[]),
            group("shared", 1003, &["alice", "bob"]),
            // A system group that is not on the privileged list: the class the
            // allow_system switch exists to protect.
            AccountGroup {
                system: true,
                ..group("shadow", 42, &[])
            },
        ],
        control_enabled: true,
        password_enabled: true,
        allow_system: false,
        allow_privileged_groups: false,
        timestamp_ms: 0,
        total_users: 5,
        total_groups: 7,
        truncated: false,
        warning: None,
    }
}

fn config() -> AccountsConfig {
    AccountsConfig {
        control: true,
        manage_passwords: true,
        home_root: "/tmp".to_string(),
        shells: vec!["/bin/sh".to_string(), "/bin/bash".to_string()],
        default_shell: "/bin/sh".to_string(),
        ..AccountsConfig::default()
    }
}

/// The guard borrows both, so tests build the pair together.
struct Fixture {
    snapshot: AccountSnapshot,
    config: AccountsConfig,
}

impl Fixture {
    fn new() -> Self {
        Self {
            snapshot: snapshot(),
            config: config(),
        }
    }

    fn guard(&self) -> Guard<'_> {
        Guard::new(&self.snapshot, &self.config, "anth")
    }
}

fn kind_of<T: std::fmt::Debug>(result: Result<T, anth_proto::AccountError>) -> AccountErrorKind {
    result.expect_err("expected a refusal").kind
}

#[test]
fn a_system_group_is_refused_until_allow_system_is_set() {
    let fixture = Fixture::new();
    let guard = fixture.guard();

    // `shadow` is below GID_MIN and is not on the privileged list, so only the
    // allow_system switch stands between it and a rename or a delete.
    assert_eq!(
        kind_of(plan_group_modify(&guard, "shadow", Some(1500), None)),
        AccountErrorKind::Protected
    );
    assert_eq!(
        kind_of(plan_group_delete(&guard, "shadow")),
        AccountErrorKind::Protected
    );
}

#[test]
fn allow_system_unlocks_a_system_group() {
    let mut fixture = Fixture::new();
    fixture.config.allow_system = true;
    let guard = fixture.guard();

    let call =
        plan_group_delete(&guard, "shadow").expect("a system group is deletable once allowed");
    assert_eq!(call.args, vec!["shadow"]);
}

#[test]
fn a_privileged_group_needs_its_own_switch_even_with_allow_system() {
    // The two opt-ins are independent: system membership must not be a way to
    // reach a group that grants root.
    let mut fixture = Fixture::new();
    fixture.config.allow_system = true;
    let guard = fixture.guard();

    assert_eq!(
        kind_of(plan_group_modify(
            &guard,
            "sudo",
            None,
            Some(&["alice".to_string()])
        )),
        AccountErrorKind::Protected
    );
}

#[test]
fn an_explicit_system_uid_needs_allow_system() {
    let fixture = Fixture::new();
    let guard = fixture.guard();

    // Passing `--system` is not the only way to land below UID_MIN: an explicit
    // uid does it silently, and it is the documented switch that must stop it.
    let args = CreateArgs {
        name: "svc",
        uid: Some(500),
        gid: Some(1001),
        gecos: None,
        home: Some("/tmp/svc"),
        shell: None,
        system: false,
    };

    assert_eq!(
        kind_of(plan_create(&guard, &args)),
        AccountErrorKind::Protected
    );
}

#[test]
fn an_explicit_system_gid_needs_allow_system() {
    let fixture = Fixture::new();
    let guard = fixture.guard();

    let args = CreateArgs {
        name: "svc",
        uid: Some(1500),
        gid: Some(42),
        gecos: None,
        home: None,
        shell: None,
        system: false,
    };

    assert_eq!(
        kind_of(plan_create(&guard, &args)),
        AccountErrorKind::Protected
    );
}

#[test]
fn allow_system_accepts_an_explicit_system_uid() {
    let mut fixture = Fixture::new();
    fixture.config.allow_system = true;
    let guard = fixture.guard();

    let args = CreateArgs {
        name: "svc",
        uid: Some(500),
        gid: Some(1001),
        gecos: None,
        home: Some("/tmp/svc"),
        shell: None,
        system: false,
    };

    let call = plan_create(&guard, &args).expect("an allowed system uid is planned");
    assert!(call.args.windows(2).any(|pair| pair == ["--uid", "500"]));
}

#[test]
fn a_gid_already_in_use_is_refused_on_modify() {
    let fixture = Fixture::new();
    let guard = fixture.guard();

    // `bob` already holds 1002. `groupmod` does not reject a duplicate gid, so
    // the panel has to: two groups with one gid is the ambiguous ownership the
    // create path already refuses.
    assert_eq!(
        kind_of(plan_group_modify(&guard, "shared", Some(1002), None)),
        AccountErrorKind::Conflict
    );
}

#[test]
fn reusing_a_groups_own_gid_is_not_a_conflict() {
    let fixture = Fixture::new();
    let guard = fixture.guard();

    // Re-sending the current gid alongside a member change must not trip the
    // duplicate check.
    let calls = plan_group_modify(&guard, "shared", Some(1003), Some(&["alice".to_string()]))
        .expect("the group's own gid is not a duplicate");
    assert_eq!(calls.len(), 2);
}

#[test]
fn a_primary_gid_in_use_is_refused_on_modify() {
    let fixture = Fixture::new();
    let guard = fixture.guard();

    let args = ModifyArgs {
        name: "alice",
        gecos: None,
        shell: None,
        home: None,
        primary_gid: Some(1003),
        locked: None,
    };

    assert_eq!(
        kind_of(plan_modify(&guard, &args)),
        AccountErrorKind::Conflict
    );
}

#[test]
fn a_group_both_system_and_privileged_reports_the_root_warning() {
    // `sudo` is gid 27, so it trips both switches. The message must name the
    // root-equivalence, not merely the id range: it is the one that explains
    // why the action is dangerous rather than where the id sits.
    let fixture = Fixture::new();
    let guard = fixture.guard();

    let args = CreateArgs {
        name: "svc",
        uid: Some(1500),
        gid: Some(27),
        gecos: None,
        home: Some("/tmp/svc"),
        shell: None,
        system: false,
    };

    let error = plan_create(&guard, &args).expect_err("both switches are off");
    assert!(error.detail.contains("sudo"), "{}", error.detail);
}

#[test]
fn names_must_be_safe_for_a_shell_and_for_passwd() {
    assert!(validate_name("alice").is_ok());
    assert!(validate_name("alice-2").is_ok());
    assert!(validate_name("_svc").is_ok());

    for bad in [
        "", "Alice", "2alice", "-rf", "a b", "a:b", "a/b", "a\nb", "alice;id",
    ] {
        assert!(validate_name(bad).is_err(), "`{bad}` should be refused");
    }
    assert!(validate_name(&"a".repeat(33)).is_err());
    assert!(validate_name(&"a".repeat(32)).is_ok());
}

#[test]
fn a_leading_dash_can_never_be_read_as_a_flag() {
    // The reason the leading-character rule exists, spelled out.
    assert!(validate_name("-rf").is_err());
    assert!(validate_name("--system").is_err());
}

#[test]
fn gecos_cannot_add_a_passwd_field_or_break_the_line() {
    assert!(validate_gecos("Alice Example").is_ok());
    for bad in ["a:b", "a\nb", "a\rb", "a\tb", "\u{0}"] {
        assert!(validate_gecos(bad).is_err(), "`{bad:?}` should be refused");
    }
}

#[test]
fn home_must_stay_inside_home_root() {
    assert!(validate_home("/tmp/alice", Path::new("/tmp")).is_ok());
    // `..` is rejected by the shared normaliser.
    assert!(validate_home("/tmp/../../etc/passwd", Path::new("/tmp")).is_err());
    assert!(validate_home("/etc/alice", Path::new("/tmp")).is_err());
    assert!(validate_home("relative/home", Path::new("/tmp")).is_err());
    // The root itself would make `useradd` treat it as the home.
    assert!(validate_home("/tmp", Path::new("/tmp")).is_err());
}

#[test]
fn a_symlinked_parent_cannot_escape_home_root() {
    let root = scratch("home-root");
    let outside = scratch("home-outside");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let link = root.join("escape");
    std::os::unix::fs::symlink(&outside, &link).unwrap();

    let requested = format!("{}/escape/alice", root.display());
    assert!(
        validate_home(&requested, &root).is_err(),
        "a symlinked parent must not escape home_root"
    );
}

#[test]
fn the_shell_must_be_on_the_allowlist() {
    let allowed = vec!["/bin/sh".to_string(), "/bin/bash".to_string()];
    assert!(validate_shell("/bin/sh", &allowed).is_ok());
    assert!(validate_shell("/bin/bash", &allowed).is_ok());
    assert!(validate_shell("/usr/bin/python3", &allowed).is_err());
    assert!(validate_shell("sh", &allowed).is_err());
}

#[test]
fn create_plans_the_expected_argument_vector() {
    let fixture = Fixture::new();
    let home = format!("{}/alice", std::env::temp_dir().display());
    let call = plan_create(
        &fixture.guard(),
        &CreateArgs {
            name: "alice2",
            uid: Some(1500),
            gid: Some(1003),
            gecos: Some("Alice Two"),
            home: Some(&home),
            shell: Some("/bin/bash"),
            system: false,
        },
    )
    .unwrap();

    assert_eq!(call.program, "useradd");
    assert_eq!(
        call.args,
        vec![
            "--create-home",
            "--uid",
            "1500",
            "--gid",
            "1003",
            "--comment",
            "Alice Two",
            "--home-dir",
            &home,
            "--shell",
            "/bin/bash",
            "alice2",
        ]
    );
    // A new account is locked by construction: the protocol has no password.
    assert!(call.stdin.is_none());
    assert!(!call.args.iter().any(|arg| arg.contains("password")));
}

#[test]
fn create_refuses_a_name_already_taken_by_a_group() {
    let fixture = Fixture::new();
    let error = plan_create(
        &fixture.guard(),
        &CreateArgs {
            name: "alice",
            uid: None,
            gid: None,
            gecos: None,
            home: None,
            shell: None,
            system: false,
        },
    )
    .unwrap_err();
    assert_eq!(error.kind, AccountErrorKind::Conflict);
}

#[test]
fn create_refuses_uid_zero() {
    let fixture = Fixture::new();
    let error = plan_create(
        &fixture.guard(),
        &CreateArgs {
            name: "newroot",
            uid: Some(0),
            gid: None,
            gecos: None,
            home: None,
            shell: None,
            system: false,
        },
    )
    .unwrap_err();
    assert_eq!(error.kind, AccountErrorKind::Protected);
}

#[test]
fn create_refuses_an_id_already_in_use() {
    let fixture = Fixture::new();
    let error = plan_create(
        &fixture.guard(),
        &CreateArgs {
            name: "clone",
            uid: Some(1001),
            gid: None,
            gecos: None,
            home: None,
            shell: None,
            system: false,
        },
    )
    .unwrap_err();
    assert_eq!(error.kind, AccountErrorKind::Conflict);
}

#[test]
fn create_refuses_a_system_account_without_the_opt_in() {
    let fixture = Fixture::new();
    let error = plan_create(
        &fixture.guard(),
        &CreateArgs {
            name: "svc",
            uid: None,
            gid: None,
            gecos: None,
            home: None,
            shell: None,
            system: true,
        },
    )
    .unwrap_err();
    assert_eq!(error.kind, AccountErrorKind::Protected);

    let mut fixture = Fixture::new();
    fixture.config.allow_system = true;
    let call = plan_create(
        &fixture.guard(),
        &CreateArgs {
            name: "svc",
            uid: None,
            gid: None,
            gecos: None,
            home: Some("/tmp/svc"),
            shell: None,
            system: true,
        },
    )
    .unwrap();
    assert!(call.args.contains(&"--system".to_string()));
}

#[test]
fn create_cannot_join_a_privileged_group_without_the_opt_in() {
    let fixture = Fixture::new();
    let error = plan_create(
        &fixture.guard(),
        &CreateArgs {
            name: "attacker",
            uid: None,
            gid: Some(27),
            gecos: None,
            home: None,
            shell: None,
            system: false,
        },
    )
    .unwrap_err();
    assert_eq!(error.kind, AccountErrorKind::Protected);
    assert!(error.detail.contains("sudo"));
}

#[test]
fn modify_builds_flags_only_for_the_fields_that_were_set() {
    let fixture = Fixture::new();
    let call = plan_modify(
        &fixture.guard(),
        &ModifyArgs {
            name: "bob",
            gecos: Some("Bob B"),
            shell: None,
            home: None,
            primary_gid: None,
            locked: None,
        },
    )
    .unwrap();

    assert_eq!(call.program, "usermod");
    assert_eq!(call.args, vec!["--comment", "Bob B", "bob"]);
}

#[test]
fn modify_with_no_fields_is_refused_rather_than_touching_the_target() {
    let fixture = Fixture::new();
    let error = plan_modify(
        &fixture.guard(),
        &ModifyArgs {
            name: "bob",
            gecos: None,
            shell: None,
            home: None,
            primary_gid: None,
            locked: None,
        },
    )
    .unwrap_err();
    assert_eq!(error.kind, AccountErrorKind::Invalid);
}

#[test]
fn modify_refuses_the_naim_read_only_and_system_accounts() {
    let fixture = Fixture::new();

    let nss = plan_modify(
        &fixture.guard(),
        &ModifyArgs {
            name: "domain",
            gecos: Some("x"),
            shell: None,
            home: None,
            primary_gid: None,
            locked: None,
        },
    )
    .unwrap_err();
    assert_eq!(nss.kind, AccountErrorKind::Unsupported);

    let root = plan_modify(
        &fixture.guard(),
        &ModifyArgs {
            name: "root",
            gecos: Some("x"),
            shell: None,
            home: None,
            primary_gid: None,
            locked: None,
        },
    )
    .unwrap_err();
    assert_eq!(root.kind, AccountErrorKind::Protected);

    let daemon = plan_modify(
        &fixture.guard(),
        &ModifyArgs {
            name: "daemon",
            gecos: Some("x"),
            shell: None,
            home: None,
            primary_gid: None,
            locked: None,
        },
    )
    .unwrap_err();
    assert_eq!(daemon.kind, AccountErrorKind::Protected, "system account");
}

#[test]
fn the_daemon_cannot_modify_itself() {
    let fixture = Fixture::new();
    let error = plan_modify(
        &fixture.guard(),
        &ModifyArgs {
            name: "anth",
            gecos: None,
            shell: None,
            home: None,
            primary_gid: None,
            locked: Some(true),
        },
    )
    .unwrap_err();
    // `anth` is not in the snapshot, so the not-found check fires first; the
    // self guard is what protects an account that *is* listed.
    assert_eq!(error.kind, AccountErrorKind::NotFound);
}

#[test]
fn the_daemon_cannot_delete_or_lock_its_own_listed_account() {
    let mut fixture = Fixture::new();
    fixture.snapshot.users.push(AccountUser {
        source: AccountSource::Local,
        ..user("anth", 1100)
    });
    let snapshot = fixture.snapshot.clone();
    let config = fixture.config.clone();
    let guard = Guard::new(&snapshot, &config, "anth");

    let delete = plan_delete(&guard, "anth", false).unwrap_err();
    assert_eq!(delete.kind, AccountErrorKind::Protected);

    let lock = plan_modify(
        &guard,
        &ModifyArgs {
            name: "anth",
            gecos: None,
            shell: None,
            home: None,
            primary_gid: None,
            locked: Some(true),
        },
    )
    .unwrap_err();
    assert_eq!(lock.kind, AccountErrorKind::Protected);
}

#[test]
fn the_last_administrator_cannot_be_deleted() {
    let mut fixture = Fixture::new();
    // `sudo` has alice; wheel is empty. Removing alice empties the only
    // administrative group that has a member. root exists but is not a member,
    // which is exactly the case the guard has to reason about.
    fixture.snapshot.groups = vec![
        group("sudo", 27, &["alice"]),
        group("wheel", 10, &[]),
        group("alice", 1001, &[]),
        group("shared", 1003, &["alice", "bob"]),
    ];
    let snapshot = fixture.snapshot.clone();
    let config = fixture.config.clone();
    let guard = Guard::new(&snapshot, &config, "anth");

    let error = plan_delete(&guard, "alice", false).unwrap_err();
    assert_eq!(error.kind, AccountErrorKind::Protected);
    assert!(
        error.detail.contains("administrative group"),
        "{}",
        error.detail
    );
}

#[test]
fn an_administrator_can_be_deleted_when_another_remains() {
    let mut fixture = Fixture::new();
    fixture.snapshot.groups = vec![
        group("sudo", 27, &["alice"]),
        group("wheel", 10, &["bob"]),
        group("alice", 1001, &[]),
        group("bob", 1002, &[]),
    ];
    let snapshot = fixture.snapshot.clone();
    let config = fixture.config.clone();
    let guard = Guard::new(&snapshot, &config, "anth");

    let call = plan_delete(&guard, "alice", false).unwrap();
    assert_eq!(call.args, vec!["alice"]);
}

#[test]
fn delete_refuses_root_and_system_accounts() {
    let fixture = Fixture::new();
    assert_eq!(
        kind_of(plan_delete(&fixture.guard(), "root", false)),
        AccountErrorKind::Protected
    );
    assert_eq!(
        kind_of(plan_delete(&fixture.guard(), "daemon", false)),
        AccountErrorKind::Protected
    );
}

#[test]
fn delete_only_removes_a_home_when_asked_and_only_inside_home_root() {
    let fixture = Fixture::new();
    let call = plan_delete(&fixture.guard(), "bob", false).unwrap();
    assert!(!call.args.contains(&"--remove".to_string()));

    let mut fixture = Fixture::new();
    fixture.config.home_root = "/tmp".to_string();
    // bob's home is /home/bob, outside /tmp, so removing it is refused.
    let error = plan_delete(&fixture.guard(), "bob", true).unwrap_err();
    assert_eq!(error.kind, AccountErrorKind::Protected);
}

#[test]
fn the_password_never_reaches_argv_and_goes_to_stdin() {
    let fixture = Fixture::new();
    let call = plan_set_password(&fixture.guard(), true, "bob", "correct-horse").unwrap();

    assert_eq!(call.program, "chpasswd");
    assert!(
        call.args.is_empty(),
        "argv must stay empty: {:?}",
        call.args
    );
    assert_eq!(
        call.stdin.as_ref().map(|input| input.expose()),
        Some("bob:correct-horse\n")
    );
    assert!(!call.args.iter().any(|arg| arg.contains("correct-horse")));
}

#[test]
fn password_control_is_a_separate_switch() {
    let fixture = Fixture::new();
    let error = plan_set_password(&fixture.guard(), false, "bob", "correct-horse").unwrap_err();
    assert_eq!(error.kind, AccountErrorKind::Disabled);
}

#[test]
fn a_password_cannot_inject_a_second_chpasswd_record() {
    let fixture = Fixture::new();
    for bad in ["short\nroot:newpass", "has:colon", "has\nnewline"] {
        assert!(
            plan_set_password(&fixture.guard(), true, "bob", bad).is_err(),
            "`{bad:?}` should be refused"
        );
    }
}

#[test]
fn a_short_password_is_refused() {
    let fixture = Fixture::new();
    let error = plan_set_password(&fixture.guard(), true, "bob", "short").unwrap_err();
    assert_eq!(error.kind, AccountErrorKind::Invalid);
}

#[test]
fn group_create_plans_the_expected_vector() {
    let fixture = Fixture::new();
    let call = plan_group_create(&fixture.guard(), "newgroup", Some(2000), false).unwrap();
    assert_eq!(call.program, "groupadd");
    assert_eq!(call.args, vec!["--gid", "2000", "newgroup"]);
}

#[test]
fn group_modify_replaces_members_instead_of_appending() {
    let fixture = Fixture::new();
    let members = vec!["bob".to_string()];
    let calls = plan_group_modify(&fixture.guard(), "shared", None, Some(&members)).unwrap();

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "gpasswd");
    // `--members` replaces the list; `--add` would append and break retries.
    assert_eq!(calls[0].args, vec!["--members", "bob", "shared"]);
    assert!(!calls[0].args.iter().any(|arg| arg == "--add"));
}

#[test]
fn a_group_change_can_need_two_calls() {
    let fixture = Fixture::new();
    let members = vec!["alice".to_string(), "bob".to_string()];
    let calls = plan_group_modify(&fixture.guard(), "shared", Some(2000), Some(&members)).unwrap();

    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].program, "groupmod");
    assert_eq!(calls[1].program, "gpasswd");
}

#[test]
fn group_modify_refuses_an_unknown_member() {
    let fixture = Fixture::new();
    let members = vec!["ghost".to_string()];
    let error = plan_group_modify(&fixture.guard(), "shared", None, Some(&members)).unwrap_err();
    assert_eq!(error.kind, AccountErrorKind::NotFound);
}

#[test]
fn group_modify_refuses_a_duplicate_member() {
    let fixture = Fixture::new();
    let members = vec!["bob".to_string(), "bob".to_string()];
    let error = plan_group_modify(&fixture.guard(), "shared", None, Some(&members)).unwrap_err();
    assert_eq!(error.kind, AccountErrorKind::Invalid);
}

#[test]
fn emptying_the_only_administrative_group_is_refused() {
    let mut fixture = Fixture::new();
    fixture.snapshot.groups = vec![
        group("sudo", 27, &["alice"]),
        group("wheel", 10, &[]),
        group("shared", 1003, &["alice"]),
    ];
    let snapshot = fixture.snapshot.clone();
    let config = fixture.config.clone();
    let guard = Guard::new(&snapshot, &config, "anth");

    let error = plan_group_modify(&guard, "sudo", None, Some(&[])).unwrap_err();
    assert_eq!(error.kind, AccountErrorKind::Protected);
}

#[test]
fn a_privileged_group_needs_the_opt_in() {
    let fixture = Fixture::new();
    let members = vec!["bob".to_string()];
    let error = plan_group_modify(&fixture.guard(), "sudo", None, Some(&members)).unwrap_err();
    assert_eq!(error.kind, AccountErrorKind::Protected);

    let mut fixture = Fixture::new();
    fixture.config.allow_privileged_groups = true;
    assert!(plan_group_modify(&fixture.guard(), "sudo", None, Some(&members)).is_ok());
}

#[test]
fn a_group_that_is_someones_primary_group_cannot_be_deleted() {
    let fixture = Fixture::new();
    let error = plan_group_delete(&fixture.guard(), "bob").unwrap_err();
    assert_eq!(error.kind, AccountErrorKind::Conflict);
}

#[test]
fn an_unused_group_can_be_deleted() {
    let mut fixture = Fixture::new();
    fixture.snapshot.groups.push(group("temp", 2000, &[]));
    let snapshot = fixture.snapshot.clone();
    let config = fixture.config.clone();
    let guard = Guard::new(&snapshot, &config, "anth");

    let call = plan_group_delete(&guard, "temp").unwrap();
    assert_eq!(call.program, "groupdel");
    assert_eq!(call.args, vec!["temp"]);
}

#[test]
fn gid_zero_is_never_touched() {
    let fixture = Fixture::new();
    assert_eq!(
        kind_of(plan_group_delete(&fixture.guard(), "root")),
        AccountErrorKind::Protected
    );
}

#[test]
fn a_planned_call_debug_is_safe_only_because_the_password_is_never_in_argv() {
    // Documents the invariant the design relies on: `PlannedCall` derives
    // `Debug`, so the password must not be in `args`. `stdin` is the carrier.
    let fixture = Fixture::new();
    let call: PlannedCall =
        plan_set_password(&fixture.guard(), true, "bob", "correct-horse").unwrap();
    let debug = format!("{call:?}");
    assert!(debug.contains("chpasswd"));
    assert!(debug.contains("<redacted>"), "{debug}");
    assert!(!debug.contains("correct-horse"), "{debug}");
}

fn scratch(label: &str) -> std::path::PathBuf {
    let unique = format!(
        "anth-accounts-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    );
    let path = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&path).unwrap();
    path
}
