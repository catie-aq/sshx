use std::path::PathBuf;
use std::process::ExitCode;

use ansi_term::Color::{Cyan, Fixed, Green, White, Yellow};
use anyhow::Result;
use clap::{Parser, Subcommand};
use sshx::{
    analyze::{AnalysisDb, ANALYSIS_FILENAME},
    controller::Controller,
    runner::Runner,
    terminal::get_default_shell,
    workspace::{send_widget_names, spawn_claude_pid_tracker, spawn_claude_tracker, spawn_source_analyzer},
};
use tokio::signal;
use tracing::{error, warn};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

/// A secure web-based, collaborative terminal.
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(subcommand)]
    command: Option<Commands>,

    // ---- Session options (used when no subcommand is given) ---------------

    /// Address of the remote sshx server.
    #[clap(long, default_value = "https://sshx.io", env = "SSHX_SERVER")]
    server: String,

    /// Local shell command to run in the terminal.
    #[clap(long)]
    shell: Option<String>,

    /// Quiet mode — only prints the URL to stdout.
    #[clap(short, long)]
    quiet: bool,

    /// Session name displayed in the title (defaults to user@hostname).
    #[clap(long)]
    name: Option<String>,

    /// Enable read-only access mode — generates separate URLs for viewers
    /// and editors.
    #[clap(long)]
    enable_readers: bool,

    /// Workspace root to stream to the browser (default: current directory).
    /// Reads `.sshx-analysis.json` if present; falls back to an inline scan.
    #[clap(long)]
    workspace: Option<PathBuf>,

    /// Disable workspace file streaming entirely.
    #[clap(long)]
    no_workspace: bool,

    /// Disable Claude Code JSONL event tracking.
    #[clap(long)]
    no_claude_tracking: bool,

    /// Launch sshx-browser alongside this session, opening the given URL.
    /// The session name, token and server are forwarded automatically.
    /// Example: --with-browser https://example.com
    #[clap(long, value_name = "URL")]
    with_browser: Option<String>,

    /// Browser binary forwarded to sshx-browser's --browser flag.
    /// Only used when --with-browser is set.
    #[clap(long, value_name = "BIN", env = "SSHX_BROWSER")]
    browser_bin: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Analyze the workspace and write source metadata to
    /// `.sshx-analysis.json`.
    ///
    /// This command scans every source file (Rust, TypeScript, Svelte, …),
    /// extracts import/export graphs, line counts, and file timestamps, then
    /// writes a structured JSON database to the workspace root.
    ///
    /// The file is read by `sshx` at session start and streamed to the browser
    /// so the FileTree and FileCard widgets are populated.  Re-running this
    /// command while a session is live will push an update to all connected
    /// browsers automatically.
    ///
    /// AI descriptions are left empty by default; they can be filled in
    /// on-demand from the browser UI.
    Analyze {
        /// Root directory to analyse (default: current directory).
        #[clap(long, value_name = "PATH")]
        workspace: Option<PathBuf>,

        /// Output file path.
        /// Defaults to `<workspace>/.sshx-analysis.json`.
        #[clap(long, value_name = "FILE")]
        output: Option<PathBuf>,
    },
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    let args = Args::parse();

    let default_level = if args.quiet { "error" } else { "info" };

    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or(default_level.into()))
        .with_writer(std::io::stderr)
        .init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    match runtime.block_on(run(args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            error!("{err:?}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<()> {
    match args.command {
        Some(Commands::Analyze { workspace, output }) => {
            run_analyze(workspace, output).await
        }
        None => run_session(args).await,
    }
}

// ---------------------------------------------------------------------------
// `sshx analyze`
// ---------------------------------------------------------------------------

async fn run_analyze(workspace: Option<PathBuf>, output: Option<PathBuf>) -> Result<()> {
    let root = match workspace {
        Some(p) => p,
        None => std::env::current_dir()?,
    };
    let root = root.canonicalize().unwrap_or(root);
    let out_path = output.unwrap_or_else(|| root.join(ANALYSIS_FILENAME));

    println!(
        "\n  {} Scanning {}…\n",
        Green.bold().paint("sshx analyze"),
        Fixed(8).paint(root.display().to_string()),
    );

    // Build fresh analysis.
    let fresh = AnalysisDb::build(&root)?;

    // Merge with any existing file to preserve previously-written descriptions.
    let db = match AnalysisDb::load(&out_path)? {
        Some(existing) => fresh.merge_with_existing(&existing),
        None => fresh,
    };

    // Write.
    db.save(&out_path)?;

    // Summary table.
    let total = db.file_count();
    let described = db.described_count();

    // Group by kind.
    let mut by_kind: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for entry in db.files.values() {
        *by_kind.entry(entry.kind.as_str()).or_default() += 1;
    }
    let mut by_kind: Vec<(&str, usize)> = by_kind.into_iter().collect();
    by_kind.sort_by_key(|(k, _)| *k);

    println!(
        "  {}  {}",
        Green.paint("✓"),
        White.bold().paint(format!("{total} files indexed")),
    );
    for (kind, count) in &by_kind {
        println!("      {:<12} {}", Fixed(8).paint(*kind), count);
    }
    println!(
        "\n  {}  descriptions: {}/{total}",
        Yellow.paint("ℹ"),
        described,
    );
    println!(
        "\n  {}  {}\n",
        Green.paint("→"),
        Cyan.underline().paint(out_path.display().to_string()),
    );

    if described < total {
        println!(
            "  {} To add AI descriptions, open the Workspace panel in the browser\n  {} and click \"Describe\" on individual files or the whole project.\n",
            Fixed(8).paint("tip"),
            Fixed(8).paint("   "),
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `sshx` (default session)
// ---------------------------------------------------------------------------

fn print_greeting(shell: &str, controller: &Controller) {
    let version_str = match option_env!("CARGO_PKG_VERSION") {
        Some(version) => format!("v{version}"),
        None => String::from("[dev]"),
    };
    if let Some(write_url) = controller.write_url() {
        println!(
            r#"
  {sshx} {version}

  {arr}  Read-only link: {link_v}
  {arr}  Writable link:  {link_e}
  {arr}  Shell:          {shell_v}
"#,
            sshx = Green.bold().paint("sshx"),
            version = Green.paint(&version_str),
            arr = Green.paint("➜"),
            link_v = Cyan.underline().paint(controller.url()),
            link_e = Cyan.underline().paint(write_url),
            shell_v = Fixed(8).paint(shell),
        );
    } else {
        println!(
            r#"
  {sshx} {version}

  {arr}  Link:  {link_v}
  {arr}  Shell: {shell_v}
"#,
            sshx = Green.bold().paint("sshx"),
            version = Green.paint(&version_str),
            arr = Green.paint("➜"),
            link_v = Cyan.underline().paint(controller.url()),
            shell_v = Fixed(8).paint(shell),
        );
    }
}

async fn run_session(args: Args) -> Result<()> {
    let shell = match args.shell {
        Some(shell) => shell,
        None => get_default_shell().await,
    };

    let name = args.name.unwrap_or_else(|| {
        let mut name = whoami::username();
        if let Ok(host) = whoami::fallible::hostname() {
            let host = host.split('.').next().unwrap_or(&host);
            name += "@";
            name += host;
        }
        name
    });

    let runner = Runner::Shell(shell.clone());
    let mut controller =
        Controller::new(&args.server, &name, runner, args.enable_readers).await?;

    // Spawn workspace intelligence tasks.
    if !args.no_workspace {
        let workspace_root = args
            .workspace
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        spawn_source_analyzer(workspace_root, controller.output_sender());
    }
    if !args.no_claude_tracking {
        spawn_claude_tracker(controller.output_sender());
        spawn_claude_pid_tracker(controller.output_sender());
    }
    // Send persisted widget names to the server so they survive restarts.
    send_widget_names(&controller.output_sender()).await;

    if args.quiet {
        if let Some(write_url) = controller.write_url() {
            println!("{}", write_url);
        } else {
            println!("{}", controller.url());
        }
    } else {
        print_greeting(&shell, &controller);
    }

    // Spawn sshx-browser if requested.
    let mut browser_child = if let Some(browser_url) = &args.with_browser {
        match spawn_sshx_browser(&args.server, controller.name(), controller.token(), browser_url, args.browser_bin.as_deref()) {
            Ok(child) => {
                if !args.quiet {
                    println!("  {}  sshx-browser launched (pid {})\n", Green.paint("➜"), child.id());
                }
                Some(child)
            }
            Err(err) => {
                warn!("failed to spawn sshx-browser: {err:#}");
                None
            }
        }
    } else {
        None
    };

    let exit_signal = signal::ctrl_c();
    tokio::pin!(exit_signal);
    tokio::select! {
        _ = controller.run() => unreachable!(),
        Ok(()) = &mut exit_signal => (),
    };
    controller.close().await?;

    if let Some(ref mut child) = browser_child {
        child.kill().ok();
    }

    Ok(())
}

/// Find and launch `sshx-browser` with the session credentials pre-filled.
///
/// Looks for `sshx-browser` next to the running `sshx` binary first, then
/// falls back to the PATH.
fn spawn_sshx_browser(
    server: &str,
    session: &str,
    token: &str,
    url: &str,
    browser_bin: Option<&str>,
) -> Result<std::process::Child> {
    // Prefer a sibling binary (useful when both are installed in the same dir).
    let browser_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("sshx-browser")))
        .filter(|p| p.exists())
        .map(|p| p.into_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("sshx-browser"));

    let mut cmd = std::process::Command::new(&browser_exe);
    cmd.arg("--server").arg(server)
        .arg("--session").arg(session)
        .arg("--token").arg(token)
        .arg("--url").arg(url);

    if let Some(bin) = browser_bin {
        cmd.arg("--browser").arg(bin);
    }

    cmd.spawn().map_err(|e| anyhow::anyhow!("could not start sshx-browser ({browser_exe:?}): {e}"))
}
