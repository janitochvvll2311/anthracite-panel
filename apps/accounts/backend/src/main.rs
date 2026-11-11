//! Host accounts service for the Accounts app.
//!
//! The inventory comes from local `/etc` files; mutations run `useradd`,
//! `userdel` and `usermod` by argv. The engine and the sandboxed view never read
//! `/etc` or spawn a process. Directory accounts and the 1.2.0 contract arrive
//! in the next phase.

mod inventory;
mod mutate;
mod policy;

use std::sync::Arc;

use anth_contract::Error;
use anth_service::Handler;
use serde_json::Value;

use inventory::{HostProbe, SystemProbe};
use mutate::{AccountRunner, SystemAccountRunner};
use policy::Policy;

const ACCOUNTS: &str = "anthracite:accounts/accounts@1.1.0";

struct Service {
    policy: Policy,
    probe: Arc<dyn HostProbe>,
    runner: Arc<dyn AccountRunner>,
    self_name: String,
}

impl Service {
    fn new(
        policy: Policy,
        probe: Arc<dyn HostProbe>,
        runner: Arc<dyn AccountRunner>,
        self_name: String,
    ) -> Self {
        Self {
            policy,
            probe,
            runner,
            self_name,
        }
    }

    fn inventory(&self) -> inventory::Inventory {
        inventory::read(&self.probe.read())
    }
}

impl Handler for Service {
    fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error> {
        if interface != ACCOUNTS {
            return Err(Error::not_found(format!(
                "`{interface}` is not exported by the accounts service"
            )));
        }
        match function {
            "users" => inventory::users_json(&self.inventory()),
            "groups" => Ok(inventory::groups_json(&self.inventory())),
            "create-user" => {
                let inventory = self.inventory();
                let guard = mutate::Guard {
                    inventory: &inventory,
                    policy: &self.policy,
                    self_name: &self.self_name,
                };
                let call = mutate::plan_create_user(&guard, &input)?;
                mutate::run_planned(self.runner.as_ref(), &call)?;
                Ok(Value::Null)
            }
            "delete-user" => {
                let inventory = self.inventory();
                let guard = mutate::Guard {
                    inventory: &inventory,
                    policy: &self.policy,
                    self_name: &self.self_name,
                };
                let call = mutate::plan_delete_user(&guard, &input)?;
                mutate::run_planned(self.runner.as_ref(), &call)?;
                Ok(Value::Null)
            }
            "set-lock" => {
                let inventory = self.inventory();
                let guard = mutate::Guard {
                    inventory: &inventory,
                    policy: &self.policy,
                    self_name: &self.self_name,
                };
                let call = mutate::plan_set_lock(&guard, &input)?;
                mutate::run_planned(self.runner.as_ref(), &call)?;
                Ok(Value::Null)
            }
            other => Err(Error::not_found(format!(
                "`anthracite:accounts/accounts#{other}` is not implemented"
            ))),
        }
    }
}

#[tokio::main]
async fn main() {
    let socket = match anth_service::socket_argument() {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let policy = match Policy::from_args(std::env::args().skip(1)) {
        Ok(policy) => policy,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let probe = SystemProbe;
    let self_name = inventory::process_user(&probe.read());
    let service = Service::new(
        policy,
        Arc::new(probe),
        Arc::new(SystemAccountRunner),
        self_name,
    );
    if let Err(error) = anth_service::serve(&socket, Arc::new(service)).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use serde_json::json;

    use inventory::LocalFiles;

    struct FixtureProbe {
        files: LocalFiles,
    }

    impl HostProbe for FixtureProbe {
        fn read(&self) -> LocalFiles {
            self.files.clone()
        }
    }

    #[derive(Default)]
    struct RecordingRunner {
        calls: Mutex<Vec<mutate::PlannedCall>>,
    }

    impl AccountRunner for RecordingRunner {
        fn run(
            &self,
            program: &str,
            args: &[String],
            stdin: Option<&str>,
        ) -> Result<mutate::AccountOutput, String> {
            self.calls.lock().expect("calls").push(mutate::PlannedCall {
                program: match program {
                    "useradd" => "useradd",
                    "userdel" => "userdel",
                    "usermod" => "usermod",
                    other => panic!("unexpected program {other}"),
                },
                args: args.to_vec(),
                stdin: stdin.map(str::to_string),
            });
            Ok(mutate::AccountOutput {
                ok: true,
                stderr: String::new(),
            })
        }
    }

    fn files() -> LocalFiles {
        LocalFiles {
            passwd: "root:x:0:0:root:/root:/bin/bash\ndaemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin\noperator:x:1000:1000:Operator:/home/operator:/bin/bash\nalice:x:1001:1001:Alice:/home/alice:/bin/bash\n".to_string(),
            group: "root:x:0:\ndaemon:x:1:\noperator:x:1000:\nalice:x:1001:\nsudo:x:27:operator\n".to_string(),
            shadow: Some("root:!:19000:0:99999:7:::\ndaemon:*:19000:0:99999:7:::\noperator:$6$hash:19000:0:99999:7:::\nalice:$6$hash:19000:0:99999:7:::\n".to_string()),
            gshadow: Some("sudo:!::operator\n".to_string()),
            login_defs: Some("UID_MIN 1000\nGID_MIN 1000\n".to_string()),
        }
    }

    fn service() -> (Service, Arc<RecordingRunner>) {
        let runner = Arc::new(RecordingRunner::default());
        let policy = Policy {
            control: true,
            allow_system: false,
            allow_privileged_groups: false,
            home_root: std::path::PathBuf::from("/home"),
            shells: vec!["/bin/sh".to_string(), "/bin/bash".to_string()],
            default_shell: "/bin/sh".to_string(),
        };
        let service = Service::new(
            policy,
            Arc::new(FixtureProbe { files: files() }),
            runner.clone(),
            "operator".to_string(),
        );
        (service, runner)
    }

    #[test]
    fn users_and_groups_answer_the_contract() {
        let (service, _) = service();
        let users = service.call(ACCOUNTS, "users", Value::Null).expect("users");
        assert_eq!(users[2]["name"], "operator");
        assert_eq!(users[2]["locked"], false);
        assert_eq!(users[3]["system"], false);
        let groups = service
            .call(ACCOUNTS, "groups", Value::Null)
            .expect("groups");
        assert_eq!(groups[4]["name"], "sudo");
        assert_eq!(groups[4]["members"][0], "operator");
    }

    #[test]
    fn create_delete_and_lock_reach_the_runner() {
        let (service, runner) = service();
        service
            .call(
                ACCOUNTS,
                "create-user",
                json!({"name": "review", "uid": 1500, "gid": 1001, "home": "/home/review", "shell": "/bin/sh", "system": false}),
            )
            .expect("create");
        service
            .call(
                ACCOUNTS,
                "set-lock",
                json!({"name": "alice", "locked": true}),
            )
            .expect("lock");
        service
            .call(ACCOUNTS, "delete-user", json!({"name": "alice"}))
            .expect("delete");
        let calls = runner.calls.lock().expect("calls");
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].program, "useradd");
        assert!(calls[0].args.contains(&"review".to_string()));
        assert!(calls[0].stdin.is_none());
        assert_eq!(calls[1].program, "usermod");
        assert_eq!(calls[1].args, vec!["--lock", "alice"]);
        assert_eq!(calls[2].program, "userdel");
        assert_eq!(calls[2].args, vec!["alice"]);
    }

    #[test]
    fn an_unknown_interface_or_function_is_not_found() {
        let (service, _) = service();
        assert_eq!(
            service
                .call("anthracite:accounts/other@1.0.0", "users", Value::Null)
                .expect_err("interface")
                .kind(),
            anth_contract::ErrorKind::NotFound
        );
        assert_eq!(
            service
                .call(ACCOUNTS, "nope", Value::Null)
                .expect_err("function")
                .kind(),
            anth_contract::ErrorKind::NotFound
        );
    }
}
