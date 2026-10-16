use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "anthd")]
#[command(version = "1.0", about = "Anthracite Panel Daemon", long_about = None)]
pub struct Cli {
    #[arg(
        name = "config",
        short,
        long,
        value_name = "FILE",
        help = "Path to the configuration file"
    )]
    pub config_path: Option<String>,
}

impl Cli {
    pub fn parse() -> Self {
        Parser::parse()
    }
}
