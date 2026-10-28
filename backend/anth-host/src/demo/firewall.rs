//! In-memory `inet anthracite` firewall for the demo build.

use std::sync::Mutex;

use anth_wire::{
    ErrorKind, FirewallAction, FirewallChain, FirewallChainState, FirewallProtocol, FirewallRule,
    FirewallSnapshot, WireError,
};

use crate::backend::FirewallBackend;
use crate::now_ms;

pub struct DemoFirewall {
    control: bool,
    state: Mutex<DemoState>,
}

struct DemoState {
    rules: Vec<FirewallRule>,
    input: FirewallAction,
    output: FirewallAction,
    next_handle: u64,
}

impl DemoFirewall {
    pub fn new(control: bool) -> Self {
        Self {
            control,
            state: Mutex::new(DemoState {
                rules: Vec::new(),
                input: FirewallAction::Allow,
                output: FirewallAction::Allow,
                next_handle: 10,
            }),
        }
    }

    fn policy(state: &DemoState, chain: FirewallChain) -> FirewallAction {
        match chain {
            FirewallChain::Input => state.input,
            FirewallChain::Output => state.output,
        }
    }

    fn set_state_policy(state: &mut DemoState, chain: FirewallChain, policy: FirewallAction) {
        match chain {
            FirewallChain::Input => state.input = policy,
            FirewallChain::Output => state.output = policy,
        }
    }
}

impl FirewallBackend for DemoFirewall {
    fn snapshot(&self) -> Result<FirewallSnapshot, WireError> {
        let state = self.state.lock().expect("demo firewall lock");
        let rules = state.rules.clone();
        let ruleset = render_ruleset(&state);
        Ok(FirewallSnapshot {
            initialized: true,
            control_enabled: self.control,
            rules,
            chains: vec![
                FirewallChainState {
                    chain: FirewallChain::Input,
                    initialized: true,
                    policy: state.input,
                },
                FirewallChainState {
                    chain: FirewallChain::Output,
                    initialized: true,
                    policy: state.output,
                },
            ],
            ruleset,
            timestamp_ms: now_ms(),
        })
    }

    fn add(
        &self,
        chain: FirewallChain,
        action: FirewallAction,
        protocol: FirewallProtocol,
        port: u16,
        comment: Option<&str>,
    ) -> Result<String, WireError> {
        self.require_control()?;
        validate_port(port)?;
        let comment = normalize_comment(comment)?;
        let mut state = self.state.lock().expect("demo firewall lock");
        if let Some(rule) = state.rules.iter_mut().find(|rule| {
            rule.chain == chain && rule.protocol == Some(protocol) && rule.port == Some(port)
        }) {
            rule.action = action.as_str().to_string();
            rule.comment = comment;
        } else {
            let handle = state.next_handle;
            state.next_handle += 1;
            state.rules.push(FirewallRule {
                handle,
                chain,
                action: action.as_str().to_string(),
                protocol: Some(protocol),
                port: Some(port),
                comment,
            });
        }
        Ok(format!(
            "{} {protocol}/{port} on {chain}",
            if matches!(action, FirewallAction::Allow) {
                "allowed"
            } else {
                "denied"
            }
        ))
    }

    fn update(
        &self,
        handle: u64,
        chain: FirewallChain,
        action: FirewallAction,
        protocol: FirewallProtocol,
        port: u16,
        comment: Option<&str>,
    ) -> Result<String, WireError> {
        self.require_control()?;
        if handle == 0 {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "firewall_handle",
                "rule handle must be greater than zero",
            ));
        }
        validate_port(port)?;
        let comment = normalize_comment(comment)?;
        let mut state = self.state.lock().expect("demo firewall lock");
        let rule = state
            .rules
            .iter_mut()
            .find(|rule| rule.handle == handle)
            .ok_or_else(|| not_found(handle))?;
        rule.chain = chain;
        rule.action = action.as_str().to_string();
        rule.protocol = Some(protocol);
        rule.port = Some(port);
        rule.comment = comment;
        Ok(format!(
            "updated firewall rule {handle} to {} {protocol}/{port} on {chain}",
            action.as_str()
        ))
    }

    fn delete(&self, handle: u64) -> Result<String, WireError> {
        self.require_control()?;
        let mut state = self.state.lock().expect("demo firewall lock");
        let before = state.rules.len();
        state.rules.retain(|rule| rule.handle != handle);
        if state.rules.len() == before {
            return Err(not_found(handle));
        }
        Ok(format!("deleted firewall rule {handle}"))
    }

    fn flush(&self, chain: FirewallChain) -> Result<String, WireError> {
        self.require_control()?;
        let mut state = self.state.lock().expect("demo firewall lock");
        let count = state
            .rules
            .iter()
            .filter(|rule| rule.chain == chain)
            .count();
        state.rules.retain(|rule| rule.chain != chain);
        Ok(format!(
            "flushed {count} firewall rule{} from {chain}",
            if count == 1 { "" } else { "s" }
        ))
    }

    fn set_policy(
        &self,
        chain: FirewallChain,
        policy: FirewallAction,
    ) -> Result<String, WireError> {
        self.require_control()?;
        let mut state = self.state.lock().expect("demo firewall lock");
        if Self::policy(&state, chain) == policy {
            return Ok(format!("{chain} policy is already {}", policy.as_str()));
        }
        Self::set_state_policy(&mut state, chain, policy);
        Ok(format!("set {chain} policy to {}", policy.as_str()))
    }
}

impl DemoFirewall {
    fn require_control(&self) -> Result<(), WireError> {
        if self.control {
            Ok(())
        } else {
            Err(WireError::new(
                ErrorKind::Forbidden,
                "firewall_control_disabled",
                "firewall control is disabled; set `[firewall] control = true`",
            ))
        }
    }
}

fn validate_port(port: u16) -> Result<(), WireError> {
    if port == 0 {
        return Err(WireError::new(
            ErrorKind::Invalid,
            "firewall_port",
            "a firewall port must be between 1 and 65535",
        ));
    }
    Ok(())
}

fn normalize_comment(comment: Option<&str>) -> Result<Option<String>, WireError> {
    let Some(comment) = comment else {
        return Ok(None);
    };
    if comment.len() > 32
        || !comment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(WireError::new(
            ErrorKind::Invalid,
            "firewall_comment",
            "a firewall comment is at most 32 letters, digits, `-`, `_` or `.`",
        ));
    }
    Ok(Some(comment.to_string()))
}

fn not_found(handle: u64) -> WireError {
    WireError::new(
        ErrorKind::NotFound,
        "firewall_rule",
        format!("no firewall rule with handle {handle} in the managed chains"),
    )
}

fn render_ruleset(state: &DemoState) -> String {
    let mut out = format!(
        "table inet anthracite {{\n\tchain input {{\n\t\ttype filter hook input priority filter; policy {};\n",
        state.input.verdict()
    );
    for rule in state
        .rules
        .iter()
        .filter(|rule| rule.chain == FirewallChain::Input)
    {
        out.push_str(&format!(
            "\t\t{} {} dport {} {}\n",
            rule.protocol.map_or("ip", FirewallProtocol::as_str),
            "dport",
            rule.port.unwrap_or(0),
            rule.action
        ));
    }
    out.push_str("\t}\n\tchain output {\n");
    out.push_str(&format!(
        "\t\ttype filter hook output priority filter; policy {};\n",
        state.output.verdict()
    ));
    for rule in state
        .rules
        .iter()
        .filter(|rule| rule.chain == FirewallChain::Output)
    {
        out.push_str(&format!(
            "\t\t{} dport {} {}\n",
            rule.protocol.map_or("ip", FirewallProtocol::as_str),
            rule.port.unwrap_or(0),
            rule.action
        ));
    }
    out.push_str("\t}\n}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(demo: &DemoFirewall, port: u16) {
        demo.add(
            FirewallChain::Input,
            FirewallAction::Allow,
            FirewallProtocol::Tcp,
            port,
            Some("web"),
        )
        .expect("add");
    }

    #[test]
    fn rules_and_policies_live_in_memory() {
        let demo = DemoFirewall::new(true);
        add(&demo, 443);
        add(&demo, 80);
        let snapshot = demo.snapshot().expect("snapshot");
        assert_eq!(snapshot.rules.len(), 2);
        assert_eq!(snapshot.chains[0].policy, FirewallAction::Allow);

        demo.update(
            10,
            FirewallChain::Output,
            FirewallAction::Deny,
            FirewallProtocol::Tcp,
            443,
            Some("blocked"),
        )
        .expect("update");
        demo.set_policy(FirewallChain::Input, FirewallAction::Deny)
            .expect("policy");
        let snapshot = demo.snapshot().expect("snapshot");
        assert_eq!(snapshot.chains[0].policy, FirewallAction::Deny);
        assert_eq!(snapshot.rules[0].chain, FirewallChain::Output);
        assert_eq!(snapshot.rules[0].action, "deny");

        demo.flush(FirewallChain::Input).expect("flush");
        assert_eq!(demo.snapshot().expect("snapshot").rules.len(), 1);
        demo.delete(10).expect("delete");
        assert!(demo.snapshot().expect("snapshot").rules.is_empty());
    }

    #[test]
    fn a_disabled_control_refuses_every_mutation() {
        let demo = DemoFirewall::new(false);
        assert_eq!(
            demo.add(
                FirewallChain::Input,
                FirewallAction::Allow,
                FirewallProtocol::Tcp,
                443,
                None
            )
            .expect_err("add")
            .code,
            "firewall_control_disabled"
        );
    }

    #[test]
    fn ports_and_comments_are_validated() {
        let demo = DemoFirewall::new(true);
        assert_eq!(
            demo.add(
                FirewallChain::Input,
                FirewallAction::Allow,
                FirewallProtocol::Tcp,
                0,
                None
            )
            .expect_err("port")
            .code,
            "firewall_port"
        );
        assert_eq!(
            demo.add(
                FirewallChain::Input,
                FirewallAction::Allow,
                FirewallProtocol::Tcp,
                443,
                Some("bad comment!")
            )
            .expect_err("comment")
            .code,
            "firewall_comment"
        );
    }
}
