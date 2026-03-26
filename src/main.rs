mod cli;
mod config;
mod gist;
mod manifest;
mod sync;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command};
use config::{get_github_token, ProjectConfig};
use gist::GistClient;

fn main() -> Result<()> {
    let cli = Cli::parse();

    let config = ProjectConfig::new(cli.name, cli.root, cli.private)?;
    let token = get_github_token()?;
    let client = GistClient::new(token)?;

    let gist_id = client.find_gist(&config.gist_description())?;

    match cli.command {
        Some(Command::Add { files }) => {
            sync::do_add(&client, &config, &files, gist_id.as_deref())?;
        }
        Some(Command::Remove { files }) => {
            sync::do_remove(&client, &config, &files, gist_id.as_deref())?;
        }
        Some(Command::Delete) => {
            sync::do_delete(&client, gist_id.as_deref())?;
        }
        Some(Command::Sync { files }) => {
            sync::do_sync(&client, &config, &files, gist_id.as_deref())?;
        }
        None => {
            sync::do_sync(&client, &config, &[], gist_id.as_deref())?;
        }
    }

    Ok(())
}
