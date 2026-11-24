mod cli;
mod daemon;
mod deletion;
mod index;
mod mounts;
mod queries;
mod web;
use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};
use index::open;
use std::sync::{Arc, atomic::AtomicBool};

fn main() {
    if let Err(e) = run() {
        if std::env::args().any(|a| a == "--json") {
            println!("{}", serde_json::json!({ "error": format!("{e:#}") }));
        } else {
            eprintln!("disko: {e:#}");
        }
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Command::Scan { scan, incremental } => daemon::writer(&cli, scan, None, !incremental),
        Command::Watch {
            scan,
            reconcile_secs,
            deep_secs,
            debounce_ms,
            listen,
            no_web,
            password_file,
            trust_proxy,
        } => daemon::writer(
            &cli,
            scan,
            Some(daemon::WatchOptions {
                interval: *reconcile_secs,
                deep_interval: *deep_secs,
                debounce: *debounce_ms,
                web: if *no_web {
                    None
                } else {
                    Some(web::Options {
                        listen: *listen,
                        password_file: web::password_path(&cli.db, password_file.as_deref()),
                        trust_proxy: *trust_proxy,
                    })
                },
            }),
            true,
        ),
        Command::Serve {
            listen,
            password_file,
            trust_proxy,
        } => {
            open(&cli.db, false)?;
            let stop = Arc::new(AtomicBool::new(false));
            signal_hook::flag::register(signal_hook::consts::SIGTERM, stop.clone())?;
            signal_hook::flag::register(signal_hook::consts::SIGINT, stop.clone())?;
            web::serve(
                cli.db.clone(),
                web::Options {
                    listen: *listen,
                    password_file: web::password_path(&cli.db, password_file.as_deref()),
                    trust_proxy: *trust_proxy,
                },
                stop,
                None,
            )
        }
        Command::Password { password_file } => {
            web::set_password(&web::password_path(&cli.db, password_file.as_deref()))
        }
        Command::Skill { install } => skill(install.as_deref()),
        _ => queries::run(&cli),
    }
}

const SKILL: &str = include_str!("../skills/disko/SKILL.md");

/// The agent skill ships inside the binary, so it always matches the installed version.
fn skill(install: Option<&std::path::Path>) -> Result<()> {
    let Some(dir) = install else {
        print!("{SKILL}");
        return Ok(());
    };
    let dir = dir.join("disko");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("SKILL.md"), SKILL)?;
    println!("installed {}", dir.join("SKILL.md").display());
    Ok(())
}
