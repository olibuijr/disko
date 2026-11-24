use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "Disko — find out where your bytes are partying")]
pub struct Cli {
    #[arg(
        long,
        global = true,
        env = "DISKO_DB",
        default_value = "/var/lib/disko/index.db"
    )]
    pub db: PathBuf,
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}
#[derive(Subcommand)]
pub enum Command {
    /// Build/reconcile the index once. Metadata only; never reads file contents.
    Scan {
        #[command(flatten)]
        scan: ScanArgs,
        /// Reread only folders whose mtime or ctime changed since they were indexed.
        #[arg(long)]
        incremental: bool,
    },
    /// Reconcile on startup, then maintain the index from Linux inotify events.
    Watch {
        #[command(flatten)]
        scan: ScanArgs,
        /// Seconds between scheduled scans (incremental unless a deep scan is due).
        #[arg(long, default_value_t = 86400)]
        reconcile_secs: u64,
        /// Seconds between deep scans that stat every entry; 0 makes every scan deep.
        #[arg(long, default_value_t = 604800)]
        deep_secs: u64,
        #[arg(long, default_value_t = 250)]
        debounce_ms: u64,
        /// Dashboard listener. Authentication is required on every interface.
        #[arg(long, default_value = "0.0.0.0:7331")]
        listen: std::net::SocketAddr,
        #[arg(long)]
        no_web: bool,
        #[arg(long)]
        password_file: Option<PathBuf>,
        /// Behind a reverse proxy on this host: take the login rate-limit address
        /// from X-Forwarded-For or X-Real-IP when the peer is loopback.
        #[arg(long, env = "DISKO_TRUST_PROXY", value_parser = clap::builder::FalseyValueParser::new())]
        trust_proxy: bool,
    },
    /// Serve the dashboard for an existing index, without starting another writer.
    Serve {
        #[arg(long, default_value = "127.0.0.1:7331")]
        listen: std::net::SocketAddr,
        #[arg(long)]
        password_file: Option<PathBuf>,
        /// Behind a reverse proxy on this host: take the login rate-limit address
        /// from X-Forwarded-For or X-Real-IP when the peer is loopback.
        #[arg(long, env = "DISKO_TRUST_PROXY", value_parser = clap::builder::FalseyValueParser::new())]
        trust_proxy: bool,
    },
    /// Read a new dashboard password from stdin and store an Argon2id hash.
    Password {
        #[arg(long)]
        password_file: Option<PathBuf>,
    },
    /// Biggest immediate children of a directory, using stored subtree totals.
    Du {
        #[arg(default_value = "/")]
        path: PathBuf,
        #[arg(short = 'n', long, default_value_t = 25)]
        limit: usize,
        #[arg(long)]
        apparent: bool,
    },
    /// Largest files or directory trees, globally or below a path.
    Largest {
        #[arg(default_value = "/")]
        path: PathBuf,
        #[arg(long, value_enum, default_value = "files")]
        kind: Kind,
        #[arg(short = 'n', long, default_value_t = 25)]
        limit: usize,
        #[arg(long)]
        apparent: bool,
    },
    /// Cached metadata, identity, timestamps and subtree totals for a path.
    Info { path: PathBuf },
    /// Search filename prefixes (indexed), or substrings with --contains.
    Find {
        text: String,
        #[arg(long)]
        contains: bool,
        #[arg(short = 'n', long, default_value_t = 50)]
        limit: usize,
    },
    /// Coverage, watcher health, freshness, and cached filesystem totals.
    Status,
    /// Largest build-output, package and cache folders that tools can recreate.
    Cleanup {
        #[arg(short = 'n', long, default_value_t = 25)]
        limit: usize,
        /// Smallest folder to list, in MiB.
        #[arg(long, default_value_t = 64)]
        min_mib: i64,
    },
    /// Large folders that grew most, from the daemon's daily size record.
    Grew {
        #[arg(long, default_value_t = 7)]
        days: i64,
    },
    /// Run a read-only SQL query; useful for custom filesystem analysis.
    Sql { query: String },
    /// One health verdict for scripts and agents; exit 0 ok, 2 warn, 1 fail.
    Health {
        /// Also check that this path is in the index.
        path: Option<PathBuf>,
    },
    /// Ask the running daemon for a deep scan (needs root).
    Rescan,
    /// Index tables and what their columns mean, for `disko sql`.
    Schema,
    /// Print the agent skill, or install it as DIR/disko/SKILL.md.
    Skill {
        #[arg(long, value_name = "DIR")]
        install: Option<PathBuf>,
    },
}
#[derive(clap::Args)]
pub struct ScanArgs {
    #[arg(default_value = "/")]
    pub roots: Vec<PathBuf>,
    /// Additional absolute subtrees to omit. Virtual filesystems and snapshots are always omitted.
    #[arg(long)]
    pub exclude: Vec<PathBuf>,
    /// Parallel directory workers; 0 picks one per core up to 4 (the writer is the limit).
    #[arg(long, default_value_t = 0)]
    pub jobs: u32,
}
#[derive(Clone, Copy, ValueEnum)]
pub enum Kind {
    Files,
    Dirs,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusting_a_proxy_is_opt_in() {
        let trusts = |args: &[&str]| match Cli::try_parse_from(args).unwrap().command {
            Command::Serve { trust_proxy, .. } | Command::Watch { trust_proxy, .. } => trust_proxy,
            _ => unreachable!(),
        };
        assert!(!trusts(&["disko", "serve"]));
        assert!(trusts(&["disko", "serve", "--trust-proxy"]));
        assert!(trusts(&["disko", "watch", "/", "--trust-proxy"]));
    }
}
