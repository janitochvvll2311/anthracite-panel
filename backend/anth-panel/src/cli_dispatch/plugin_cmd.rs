use super::super::main_prelude::*;

/// `plugin` acts on the bundle directory and the per-plugin grant files, so it
/// runs locally, like `auth` and `config`. A running server reads the grants at
/// start, and the command says so when it changes anything.
pub fn run_plugin(config_path: &str, args: &PluginArgs) -> ExitCode {
    let mut config = match Config::load_or_create(config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("[ERROR] {error}");
            return ExitCode::FAILURE;
        }
    };
    let plugins = config.plugins.clone();

    match &args.command {
        PluginCommand::List => {
            let (installed, problems) =
                anth_panel::plugins::bundle::list(&plugins.root_path(), &plugins.grants);
            print!(
                "{}",
                anth_panel::render::render_plugin_listing(&installed, &problems)
            );
            if !problems.is_empty() && installed.is_empty() {
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        PluginCommand::Install(install) => {
            let path = std::path::Path::new(&install.bundle);
            let result = match anth_panel::plugins::install_bundle(path, &plugins) {
                Ok(result) => result,
                Err(error) => {
                    eprintln!("[ERROR] {error}");
                    return ExitCode::FAILURE;
                }
            };
            let installed = result.installed;
            let grant = result.grant;
            println!(
                "Installed {} {} with {} capabilit{}",
                installed.plugin.id,
                installed.plugin.version,
                grant.allowed_ids().len(),
                if grant.allowed_ids().len() == 1 {
                    "y"
                } else {
                    "ies"
                }
            );
            for capability in grant.allowed_ids() {
                println!("  {capability}");
            }
            if let Some(warning) = result.warning {
                eprintln!("[WARN] {warning}");
            }
            config.plugins.upsert_grant(grant);
            finish_plugin_change(&config, config_path)
        }
        PluginCommand::Remove(remove) => {
            if let Err(error) =
                anth_panel::plugins::bundle::remove(&plugins.root_path(), &remove.id)
            {
                eprintln!("[ERROR] {error}");
                return ExitCode::FAILURE;
            }
            match config.plugins.remove_grant(&remove.id) {
                Some(_) => println!("Removed {}", remove.id),
                None => {
                    println!("Removed the directory of {}, which had no grant", remove.id);
                }
            }
            finish_plugin_change(&config, config_path)
        }
        PluginCommand::Enable(toggle) => {
            let Some(grant) = config.plugins.grant(&toggle.id).cloned() else {
                eprintln!("[ERROR] plugin `{}` is not installed", toggle.id);
                return ExitCode::FAILURE;
            };
            let mut updated = grant;
            updated.enabled = !toggle.disable;
            let state = if updated.enabled {
                "enabled"
            } else {
                "disabled"
            };
            config.plugins.upsert_grant(updated);
            println!("{} is {state}", toggle.id);
            finish_plugin_change(&config, config_path)
        }
        PluginCommand::Approve(approve) => {
            if !approve.accept_capabilities {
                eprintln!(
                    "[ERROR] pass --accept-capabilities to confirm the capabilities of `{}`",
                    approve.id
                );
                return ExitCode::FAILURE;
            }
            let root =
                match anth_panel::plugins::bundle::plugin_dir(&plugins.root_path(), &approve.id) {
                    Ok(root) => root,
                    Err(error) => {
                        eprintln!("[ERROR] {error}");
                        return ExitCode::FAILURE;
                    }
                };
            let text = match std::fs::read_to_string(root.join("plugin.toml")) {
                Ok(text) => text,
                Err(error) => {
                    eprintln!("[ERROR] could not read the manifest: {error}");
                    return ExitCode::FAILURE;
                }
            };
            let manifest = match anth_panel::plugins::Manifest::parse(&text) {
                Ok(manifest) => manifest,
                Err(error) => {
                    eprintln!("[ERROR] {error}");
                    return ExitCode::FAILURE;
                }
            };
            // Approving must not freeze a digest the file does not have: once
            // `load` trusts the grant's hash, an unchecked approve would write a
            // grant that makes the plugin permanently unloadable.
            if let Err(error) = anth_panel::plugins::bundle::verify(&root, &manifest.wasm.sha256) {
                eprintln!("[ERROR] {error}");
                return ExitCode::FAILURE;
            }
            // The hash proves the manifest and the module agree; only the
            // signature proves the pair came from the publisher the admin
            // trusts. Approving widens authority, so it applies the same rule as
            // install instead of trusting a rewritten bundle.
            match anth_panel::plugins::bundle::verify_signature(
                &root,
                plugins.require_signature,
                &plugins.public_key,
            ) {
                Ok(Some(warning)) => eprintln!("[WARN] {warning}"),
                Ok(None) => {}
                Err(error) => {
                    eprintln!("[ERROR] {error}");
                    return ExitCode::FAILURE;
                }
            }
            let grant = anth_panel::plugins::PluginGrant::from_manifest(&manifest);
            println!(
                "Approved {} {} with {} capabilit{}",
                grant.id,
                grant.version,
                grant.allowed_ids().len(),
                if grant.allowed_ids().len() == 1 {
                    "y"
                } else {
                    "ies"
                }
            );
            for capability in grant.allowed_ids() {
                println!("  {capability}");
            }
            config.plugins.upsert_grant(grant);
            finish_plugin_change(&config, config_path)
        }
        PluginCommand::Call(call) => run_plugin_call(&plugins, &config, call),
    }
}

/// Runs one plugin capability through its wasm module.
///
/// This is the CLI anchor: the core dispatcher declines a subcommand a plugin
/// answers, and the plugin gets a request envelope. It can only act through
/// `anth_exec`, against the recipe frozen in the grant.
pub fn run_plugin_call(
    plugins: &anth_panel::plugins::PluginsConfig,
    config: &Config,
    call: &anth_panel::cli::PluginCallArgs,
) -> ExitCode {
    let request = match call.request.as_str() {
        "-" => {
            let mut text = String::new();
            if let Err(error) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text) {
                eprintln!("[ERROR] could not read the request from stdin: {error}");
                return ExitCode::FAILURE;
            }
            text
        }
        other => other.to_string(),
    };
    // Parse before loading anything: a malformed envelope is the caller's
    // mistake and should not look like a broken plugin.
    if let Err(error) = serde_json::from_str::<serde_json::Value>(&request) {
        eprintln!("[ERROR] the request is not valid JSON: {error}");
        return ExitCode::FAILURE;
    }

    // `enabled` is the master switch: the registry loads nothing and `install`
    // denies when it is off, so this path must not become a way around it.
    if !plugins.enabled {
        eprintln!("[ERROR] plugins are disabled in the configuration");
        return ExitCode::FAILURE;
    }
    let Some(grant) = config
        .plugins
        .grants
        .iter()
        .find(|grant| grant.id == call.id)
        .cloned()
    else {
        eprintln!("[ERROR] no plugin `{}` is installed", call.id);
        return ExitCode::FAILURE;
    };
    let root = match anth_panel::plugins::bundle::plugin_dir(&plugins.root_path(), &call.id) {
        Ok(root) => root,
        Err(error) => {
            eprintln!("[ERROR] {error}");
            return ExitCode::FAILURE;
        }
    };
    // `load` re-hashes the module against the grant, so a swapped file is caught
    // here as well as at the daemon.
    if let Err(error) = anth_panel::plugins::bundle::load(&root, &grant) {
        eprintln!("[ERROR] {error}");
        return ExitCode::FAILURE;
    }
    let wasm = match std::fs::read(root.join("plugin.wasm")) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("[ERROR] could not read the module: {error}");
            return ExitCode::FAILURE;
        }
    };

    let runner = anth_panel::command::SystemRunner::new(config.sudo);
    match anth_panel::plugins::runtime::invoke_with_jobs(
        &wasm,
        &grant.id,
        &grant,
        &runner,
        &anth_panel::plugins::jobs::SystemJobs::default(),
        &request,
        config.plugins.fuel,
    ) {
        Ok(reply) => {
            for line in &reply.log {
                eprintln!("[plugin] {line}");
            }
            if reply.log_dropped > 0 {
                eprintln!(
                    "[WARN] the plugin logged {} more line(s) than the host accepts",
                    reply.log_dropped
                );
            }
            println!("{}", reply.body);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("[ERROR] {error}");
            ExitCode::FAILURE
        }
    }
}

pub fn finish_plugin_change(config: &Config, config_path: &str) -> ExitCode {
    match config.save(config_path) {
        Ok(()) => {
            println!("Restart the server to load the change.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("[ERROR] {error}");
            ExitCode::FAILURE
        }
    }
}
