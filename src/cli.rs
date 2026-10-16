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
    Scan(ScanArgs),
    /// Reconcile on startup, then maintain the index from Linux inotify events.
    Watch {
        #[command(flatten)]
        scan: ScanArgs,
        #[arg(long, default_value_t = 21600)]
        reconcile_secs: u64,
        #[arg(long, default_value_t = 250)]
        debounce_ms: u64,
        /// Dashboard listener. Authentication is required on every interface.
        #[arg(long, default_value = "0.0.0.0:7331")]
        listen: std::net::SocketAddr,
        #[arg(long)]
        no_web: bool,
        #[arg(long)]
        password_file: Option<PathBuf>,
    },
    /// Serve the dashboard for an existing index, without starting another writer.
    Serve {
        #[arg(long, default_value = "127.0.0.1:7331")]
        listen: std::net::SocketAddr,
        #[arg(long)]
        password_file: Option<PathBuf>,
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
    /// Run a read-only SQL query; useful for custom filesystem analysis.
    Sql { query: String },
}
#[derive(clap::Args)]
pub struct ScanArgs {
    #[arg(default_value = "/")]
    pub roots: Vec<PathBuf>,
    /// Additional absolute subtrees to omit. Virtual filesystems and snapshots are always omitted.
    #[arg(long)]
    pub exclude: Vec<PathBuf>,
}
#[derive(Clone, Copy, ValueEnum)]
pub enum Kind {
    Files,
    Dirs,
}
