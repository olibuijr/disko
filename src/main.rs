mod cli;
mod daemon;
mod deletion;
mod index;
mod queries;
mod web;
use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};
use index::open;
use std::sync::{Arc, atomic::AtomicBool};

fn main() {
    if let Err(e) = run() {
        eprintln!("disko: {e:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Command::Scan(s) => daemon::writer(&cli, s, None),
        Command::Watch {
            scan,
            reconcile_secs,
            debounce_ms,
            listen,
            no_web,
            password_file,
        } => daemon::writer(
            &cli,
            scan,
            Some(daemon::WatchOptions {
                interval: *reconcile_secs,
                debounce: *debounce_ms,
                web: if *no_web {
                    None
                } else {
                    Some(web::Options {
                        listen: *listen,
                        password_file: web::password_path(&cli.db, password_file.as_deref()),
                    })
                },
            }),
        ),
        Command::Serve {
            listen,
            password_file,
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
                },
                stop,
                None,
            )
        }
        Command::Password { password_file } => {
            web::set_password(&web::password_path(&cli.db, password_file.as_deref()))
        }
        _ => queries::run(&cli),
    }
}
