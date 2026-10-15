use anth_proto::{Request, Response};
use clap::{Parser, Subcommand};

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

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn parse() -> Self {
        Parser::parse()
    }
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Echo {
        #[arg(help = "Message to echo")]
        message: String,
    },
}

pub fn request_for(command: &Command) -> Request {
    match command {
        Command::Echo { message } => Request::Echo {
            message: message.clone(),
        },
    }
}

pub fn handle_response(response: &Response) {
    match response {
        Response::Ok { message } => {
            println!("[OK] {}", message);
        }
        Response::Error { detail } => {
            eprintln!("[ERROR] {}", detail);
        }
    }
}
