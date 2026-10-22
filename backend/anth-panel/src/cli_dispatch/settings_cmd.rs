use super::super::main_prelude::*;

/// Runs one `auth` subcommand against the config file.
pub fn run_auth(config_path: &str, args: &AuthArgs) -> ExitCode {
    let config = match Config::load_or_create(config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("[ERROR] {error}");
            return ExitCode::FAILURE;
        }
    };

    let updated = match &args.command {
        AuthCommand::Status => {
            anth_panel::settings::report_status(&config);
            return ExitCode::SUCCESS;
        }
        AuthCommand::Enable(toggle) => anth_panel::settings::toggle_auth(config, toggle),
        AuthCommand::SetPassword(password_args) => {
            let password = match anth_panel::settings::read_password(password_args) {
                Ok(password) => password,
                Err(error) => {
                    eprintln!("[ERROR] {error}");
                    return ExitCode::FAILURE;
                }
            };
            anth_panel::settings::set_password(config, password.expose())
        }
    };

    match updated {
        Ok(config) => match config.save(config_path) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("[ERROR] {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("[ERROR] {error}");
            ExitCode::FAILURE
        }
    }
}

/// Runs one `config` subcommand against the config file.
pub fn run_config(config_path: &str, args: &ConfigArgs) -> ExitCode {
    match &args.command {
        ConfigCommand::Get => {
            let config = match Config::load_or_create(config_path) {
                Ok(config) => config,
                Err(error) => {
                    eprintln!("[ERROR] {error}");
                    return ExitCode::FAILURE;
                }
            };
            match config.to_toml() {
                Ok(text) => {
                    print!("{text}");
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("[ERROR] {error}");
                    ExitCode::FAILURE
                }
            }
        }
        ConfigCommand::Set(set) => {
            let mut config = match Config::load_or_create(config_path) {
                Ok(config) => config,
                Err(error) => {
                    eprintln!("[ERROR] {error}");
                    return ExitCode::FAILURE;
                }
            };
            if let Err(error) = anth_panel::settings::set_key(&mut config, set) {
                eprintln!("[ERROR] {error}");
                return ExitCode::FAILURE;
            }
            match config.save(config_path) {
                Ok(()) => {
                    println!("{} = {}", set.key, set.value);
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("[ERROR] {error}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
