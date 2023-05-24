use clap::{command, Parser, Subcommand};
use symfony_dev_proxy::{config::MyConfig, http::start_server, provider::Mapping};

use anyhow::{anyhow, Result};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Run { port: Option<u16> },
    Add { host: String, target: String },
    Remove { host: String },
    List {},
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut config: MyConfig =
        confy::load("symfony-dev-proxy", None).unwrap_or(MyConfig::default());

    match &cli.command {
        Some(Commands::Run { port }) => {
            let configured_port: u16 = port.unwrap_or_else(|| 7040);
            if cli.verbose {
                println!("Starting proxy server with port {}", configured_port);
            }

            start_server(configured_port, cli.verbose).await;
        }
        Some(Commands::Add { host, target }) => {
            if cli.verbose {
                println!("Adding new mapping {} --> {}", host, target);
            }

            let mapping = Mapping::new(host.clone(), target.clone());

            if config.mappings.contains(&mapping) {
                let index = config
                    .mappings
                    .iter()
                    .position(|m| m == &mapping)
                    .expect("Found mapping but no position");
                config.mappings.remove(index);
            }

            config.mappings.push(mapping);

            let result = confy::store("symfony-dev-proxy", None, &config);
            if result.is_err() {
                return Err(anyhow!("Could not save mapping"));
            }
        }

        Some(Commands::Remove { host }) => {
            if cli.verbose {
                println!("Remove mapping {}", host);
            }

            let index = config.mappings.iter().position(|m| &m.host == host);

            if let Some(i) = index {
                config.mappings.remove(i);
                let result = confy::store("symfony-dev-proxy", None, &config);
                if result.is_err() {
                    return Err(anyhow!("Could not save mapping"));
                }
            } else {
                return Err(anyhow!("Did not find mapping with host {}", host));
            }
        }
        Some(Commands::List {}) => {
            if config.mappings.is_empty() {
                println!("No mapping present");
                return Ok(());
            }
            for entry in config.mappings {
                println!("{} --> {}", entry.host, entry.target);
            }
        }
        None => {}
    }

    Ok(())
}
