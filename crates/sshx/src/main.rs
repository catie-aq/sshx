use std::path::PathBuf;
use std::process::ExitCode;

use ansi_term::Color::{Cyan, Fixed, Green, White, Yellow};
use anyhow::Result;
use clap::{Parser, Subcommand};
use sshx::{
    agent::{spawn_proc_poller, TerminalRegistry, ENV_HOOK_SOCKET, ENV_SESSION},
    analyze::{AnalysisDb, ANALYSIS_FILENAME},
    controller::Controller,
    platform::{PlatformClient, PlatformConfig, Registration},
    runner::{Runner, ShellContext},
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
#[clap(author, version = sshx::update::VERSION, about, long_about = None)]
struct Args {
    #[clap(subcommand)]
    command: Option<Commands>,

    /// Address of the remote sshx server. Defaults to the server announced by
    /// the platform at enrollment, then to the built-in server.
    #[clap(long, global = true, env = "SSHX_SERVER")]
    server: Option<String>,

    // ---- Session options (used when no subcommand is given) ---------------

    /// Do not check the server for sshx updates in the background.
    #[clap(long, env = "SSHX_NO_AUTO_UPDATE", value_parser = clap::builder::BoolishValueParser::new())]
    no_auto_update: bool,

    /// Local shell command to run in the terminal.
    #[clap(long)]
    shell: Option<String>,

    /// Quiet mode — only prints the URL to stdout.
    #[clap(short, long)]
    quiet: bool,

    /// Session name displayed in the title (defaults to user@hostname).
    #[clap(long)]
    name: Option<String>,

    /// Custom session slug — sets the URL path to /s/<slug>.
    /// Must be 2–40 alphanumeric characters, hyphens, or underscores.
    /// Example: --slug plati  →  https://sshx.io/s/plati
    #[clap(long)]
    slug: Option<String>,

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

    /// Path to the `openvscode-server` binary. When set, enables the IDE
    /// tunnel feature: browsers can access VS Code at `/ide/s/{session}/`.
    /// Overrides --ide auto-download.
    #[clap(long, value_name = "PATH", env = "SSHX_OPENVSCODE_BIN")]
    openvscode_bin: Option<PathBuf>,

    /// Enable the embedded VS Code IDE. Downloads OpenVSCode Server to
    /// ~/.sshx/openvscode-server/ on first use. Use --openvscode-bin to
    /// point to a custom binary instead.
    #[clap(long)]
    ide: bool,

    /// Do not offer VS Code to SSHX Platform. By default, a session registered
    /// with the platform installs OpenVSCode Server when VS Code is first
    /// opened from it.
    #[clap(long, conflicts_with = "ide")]
    no_ide: bool,

    /// URL of the SSHX Platform to register this session with. Defaults to
    /// the configuration saved by `sshx platform enroll`.
    #[clap(long, value_name = "URL", env = "SSHX_PLATFORM_URL")]
    platform: Option<String>,

    /// Machine token created in the platform UI (`smt_….<key>`).
    #[clap(long, value_name = "TOKEN", env = "SSHX_PLATFORM_TOKEN", hide_env_values = true)]
    platform_token: Option<String>,

    /// Do not register this session with the platform.
    #[clap(long)]
    no_platform: bool,
}

#[derive(Subcommand, Debug)]
enum HooksAction {
    /// Add the sshx hooks to ~/.claude/settings.json (idempotent).
    Install,
    /// Remove the sshx hooks from ~/.claude/settings.json.
    Uninstall,
    /// Show whether the hooks are installed.
    Status,
}

#[derive(Subcommand, Debug)]
enum PlatformAction {
    /// Save the platform URL and machine token for future sessions, and
    /// install the Claude Code hooks.
    Enroll {
        /// Base URL of the platform.
        url: String,
        /// Machine token shown by the platform (`smt_….<key>`).
        token: String,
        /// Do not install the Claude Code hooks.
        #[clap(long)]
        no_claude_hooks: bool,
    },
    /// Check the whole chain: enrollment, token, sshx server, Claude Code
    /// hooks and, inside an sshx terminal, its hook socket. Exits with 1 if a
    /// check fails.
    Status,
    /// Delete the saved platform configuration.
    Forget,
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

    /// Forward an agent hook event (read on stdin) to the sshx process that
    /// owns this terminal. Used by `sshx claude-hooks install`; silent no-op
    /// outside an sshx terminal.
    #[clap(hide = true)]
    Hook {
        /// Agent kind sending the event.
        #[clap(default_value = "claude")]
        agent: String,
    },

    /// Manage the Claude Code hooks that report agent state per terminal
    /// (working, finished, question, permission).
    ClaudeHooks {
        #[clap(subcommand)]
        action: HooksAction,
    },

    /// Connect this machine to SSHX Platform.
    Platform {
        #[clap(subcommand)]
        action: PlatformAction,
    },

    /// Update sshx to the latest release published by the server.
    Update {
        /// Only check whether an update is available.
        #[clap(long)]
        check: bool,

        /// Reinstall the server's release even if it is not newer.
        #[clap(long)]
        force: bool,
    },
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    let args = Args::parse();

    // Hooks run on every agent event: forward and exit before any setup.
    if let Some(Commands::Hook { agent }) = &args.command {
        sshx::agent::forward_hook(agent);
        return ExitCode::SUCCESS;
    }

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

/// Built-in sshx server, used when neither `--server` nor an enrollment
/// gives one.
const DEFAULT_SERVER: &str = env!("SSHX_DEFAULT_SERVER_URL");

/// sshx server for commands without a session: `--server`, else the one saved
/// at enrollment, else the built-in default.
fn configured_server(explicit: Option<String>) -> String {
    explicit
        .or_else(|| PlatformConfig::load().ok().flatten().and_then(|c| c.server))
        .unwrap_or_else(|| DEFAULT_SERVER.to_string())
}

async fn run(mut args: Args) -> Result<()> {
    sshx::update::cleanup_old_exe();
    match args.command.take() {
        Some(Commands::Analyze { workspace, output }) => {
            run_analyze(workspace, output).await
        }
        Some(Commands::Update { check, force }) => {
            run_update(&configured_server(args.server), check, force).await
        }
        Some(Commands::Hook { .. }) => Ok(()),
        Some(Commands::ClaudeHooks { action }) => run_claude_hooks(action),
        Some(Commands::Platform { action }) => run_platform(action, args.server).await,
        None => run_session(args).await,
    }
}

// ---------------------------------------------------------------------------
// `sshx update`
// ---------------------------------------------------------------------------

async fn run_update(server: &str, check: bool, force: bool) -> Result<()> {
    use sshx::update::{install, is_newer, latest_release, PLATFORM, VERSION};

    println!(
        "\n  {} {}  {}\n",
        Green.bold().paint("sshx"),
        Green.paint(format!("v{VERSION}")),
        Fixed(8).paint(PLATFORM),
    );

    let Some(release) = latest_release(server).await? else {
        anyhow::bail!("{server} does not publish an sshx release for {PLATFORM}");
    };

    if !force && !is_newer(&release.version, VERSION) {
        println!(
            "  {}  Already up to date (latest: v{})\n",
            Green.paint("✓"),
            release.version,
        );
        return Ok(());
    }

    if check {
        println!(
            "  {}  Update available: v{}  (run `sshx update`)\n",
            Yellow.paint("ℹ"),
            release.version,
        );
        return Ok(());
    }

    println!(
        "  {}  Downloading v{} from {}…",
        Green.paint("➜"),
        release.version,
        Fixed(8).paint(server),
    );
    let exe = install(server, &release).await?;
    println!(
        "  {}  Updated {} to v{}\n",
        Green.paint("✓"),
        Cyan.paint(exe.display().to_string()),
        release.version,
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// `sshx claude-hooks` and `sshx platform`
// ---------------------------------------------------------------------------

fn run_claude_hooks(action: HooksAction) -> Result<()> {
    use sshx::claude_hooks::{install, status, uninstall, HOOK_EVENTS};
    match action {
        HooksAction::Install => {
            let path = install()?;
            println!(
                "\n  {}  Claude Code hooks installed in {}",
                Green.paint("✓"),
                Cyan.paint(path.display().to_string()),
            );
            println!(
                "  {}  Claude sessions started in sshx terminals now report their state.\n  {}  Already running sessions pick it up after a restart.\n",
                Fixed(8).paint("ℹ"),
                Fixed(8).paint(" "),
            );
        }
        HooksAction::Uninstall => {
            let (path, removed) = uninstall()?;
            println!(
                "\n  {}  Removed {removed} hook(s) from {}\n",
                Green.paint("✓"),
                Cyan.paint(path.display().to_string()),
            );
        }
        HooksAction::Status => {
            let (path, events) = status()?;
            let ok = events.len() == HOOK_EVENTS.len();
            println!(
                "\n  {}  {}/{} hook events installed in {}\n",
                if ok { Green.paint("✓") } else { Yellow.paint("ℹ") },
                events.len(),
                HOOK_EVENTS.len(),
                Cyan.paint(path.display().to_string()),
            );
            if !ok {
                println!("  {} run `sshx claude-hooks install`\n", Fixed(8).paint("tip"));
            }
        }
    }
    Ok(())
}

async fn run_platform(action: PlatformAction, explicit_server: Option<String>) -> Result<()> {
    match action {
        PlatformAction::Enroll { url, token, no_claude_hooks } => {
            let mut config = PlatformConfig { url, token, server: None };
            let client = PlatformClient::new(&config)?;
            let machine = client.whoami().await?;
            config.server = explicit_server.or(machine.sshx_server.clone());
            let path = config.save()?;
            println!(
                "\n  {}  Enrolled as {} ({}, owner {}) on {}",
                Green.paint("✓"),
                White.bold().paint(&machine.name),
                machine.kind,
                machine.owner,
                Cyan.underline().paint(client.base_url()),
            );
            if let Some(server) = &config.server {
                println!("  {}  sshx server: {}", Green.paint("✓"), Cyan.paint(server));
            }
            println!(
                "  {}  Configuration saved to {}",
                Green.paint("✓"),
                Cyan.paint(path.display().to_string()),
            );
            if no_claude_hooks {
                println!(
                    "  {}  Claude Code hooks not installed (run `sshx claude-hooks install` later)",
                    Fixed(8).paint("ℹ"),
                );
            } else {
                match sshx::claude_hooks::install() {
                    Ok(path) => println!(
                        "  {}  Claude Code hooks installed in {}",
                        Green.paint("✓"),
                        Cyan.paint(path.display().to_string()),
                    ),
                    Err(err) => println!(
                        "  {}  Claude Code hooks not installed: {err:#}",
                        Yellow.paint("✗"),
                    ),
                }
            }
            println!(
                "\n  {}  Every `sshx` session now appears on the platform. Check with `sshx platform status`.\n",
                Green.paint("➜"),
            );
        }
        PlatformAction::Status => {
            if !platform_status(explicit_server).await {
                std::process::exit(1);
            }
        }
        PlatformAction::Forget => {
            if PlatformConfig::forget()? {
                println!("\n  {}  Platform configuration deleted\n", Green.paint("✓"));
            } else {
                println!("\n  {}  No platform configuration\n", Yellow.paint("ℹ"));
            }
        }
    }
    Ok(())
}

/// Printer for `sshx platform status`.
#[derive(Default)]
struct Checklist {
    failed: usize,
}

impl Checklist {
    fn line(&self, mark: ansi_term::ANSIGenericString<'static, str>, label: &str, detail: &str) {
        println!("  {mark}  {:<14} {detail}", White.bold().paint(label).to_string());
    }
    fn ok(&mut self, label: &str, detail: impl AsRef<str>) {
        self.line(Green.paint("✓"), label, detail.as_ref());
    }
    fn info(&mut self, label: &str, detail: impl AsRef<str>) {
        self.line(Fixed(8).paint("ℹ"), label, detail.as_ref());
    }
    fn fail(&mut self, label: &str, detail: impl AsRef<str>, tip: &str) {
        self.failed += 1;
        self.line(Yellow.paint("✗"), label, detail.as_ref());
        if !tip.is_empty() {
            println!("     {:<14} {}", "", Fixed(8).paint(tip.to_string()));
        }
    }
}

/// Check every link of the platform chain. Returns false if one is broken.
async fn platform_status(explicit_server: Option<String>) -> bool {
    use sshx::agent::ENV_TERMINAL_ID;
    use sshx::claude_hooks::{command_executable, hook_command, HOOK_EVENTS};

    let mut c = Checklist::default();
    println!("\n  {}\n", Green.bold().paint("sshx platform status"));

    // 1. Enrollment.
    let config = match PlatformConfig::resolve(
        std::env::var("SSHX_PLATFORM_URL").ok(),
        std::env::var("SSHX_PLATFORM_TOKEN").ok(),
    ) {
        Ok(Some(config)) => {
            let source = PlatformConfig::path()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            c.ok("Enrolled", format!("{}  ({source})", config.url));
            Some(config)
        }
        Ok(None) => {
            c.fail(
                "Enrolled",
                "no platform configured",
                "in the platform: Ajouter une machine, then run the command it shows",
            );
            None
        }
        Err(err) => {
            c.fail("Enrolled", format!("{err:#}"), "run `sshx platform enroll <url> <token>` again");
            None
        }
    };

    // 2. Platform and token.
    let machine = match config.as_ref().map(PlatformClient::new) {
        Some(Ok(client)) => match client.whoami().await {
            Ok(machine) => {
                c.ok(
                    "Platform",
                    format!("machine {} ({}, owner {})", machine.name, machine.kind, machine.owner),
                );
                Some(machine)
            }
            Err(err) => {
                c.fail("Platform", format!("{err:#}"), "check the URL, or create a new token in the platform");
                None
            }
        },
        Some(Err(err)) => {
            c.fail("Platform", format!("{err:#}"), "copy the whole token, including the part after the dot");
            None
        }
        None => None,
    };

    // 3. sshx server.
    let server = explicit_server
        .or_else(|| config.as_ref().and_then(|c| c.server.clone()))
        .or_else(|| machine.as_ref().and_then(|m| m.sshx_server.clone()))
        .unwrap_or_else(|| DEFAULT_SERVER.to_string());
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build();
    match http {
        Ok(http) => match http.get(&server).send().await {
            Ok(_) => c.ok("sshx server", &server),
            Err(err) => c.fail(
                "sshx server",
                format!("{server}: unreachable ({err})"),
                "is sshx-server running? pass --server to override",
            ),
        },
        Err(err) => c.fail("sshx server", format!("{err:#}"), ""),
    }
    if let (Some(saved), Some(announced)) = (
        config.as_ref().and_then(|c| c.server.as_deref()),
        machine.as_ref().and_then(|m| m.sshx_server.as_deref()),
    ) {
        if saved.trim_end_matches('/') != announced.trim_end_matches('/') {
            c.info("", format!("the platform now announces {announced}; enroll again to use it"));
        }
    }

    // 4. Sessions.
    if let Some(n) = machine.as_ref().and_then(|m| m.sessions_online) {
        if n == 0 {
            c.info("Sessions", "none online: run `sshx` to open one");
        } else {
            c.ok("Sessions", format!("{n} online"));
        }
    }

    // 5. Claude Code hooks.
    match sshx::claude_hooks::report() {
        Ok(report) if report.events.is_empty() => c.fail(
            "Claude hooks",
            format!("not installed in {}", report.path.display()),
            "run `sshx claude-hooks install` to report Claude Code states",
        ),
        Ok(report) => {
            let missing: Vec<_> = report
                .commands
                .iter()
                .map(|cmd| command_executable(cmd))
                .filter(|exe| !exe.exists())
                .collect();
            if !missing.is_empty() {
                c.fail(
                    "Claude hooks",
                    format!("run {}, which does not exist", missing[0].display()),
                    "run `sshx claude-hooks install` again",
                );
            } else if report.events.len() < HOOK_EVENTS.len() {
                c.fail(
                    "Claude hooks",
                    format!("{}/{} events installed", report.events.len(), HOOK_EVENTS.len()),
                    "run `sshx claude-hooks install` again",
                );
            } else {
                let current = hook_command().ok();
                let exe = report
                    .commands
                    .first()
                    .map(|cmd| command_executable(cmd).display().to_string())
                    .unwrap_or_default();
                c.ok("Claude hooks", format!("{}/{} events → {exe}", report.events.len(), HOOK_EVENTS.len()));
                if current.is_some_and(|cur| !report.commands.contains(&cur)) {
                    c.info("", "the hooks run another sshx binary than this one");
                }
            }
        }
        Err(err) => c.fail("Claude hooks", format!("{err:#}"), ""),
    }

    // 6. This terminal.
    match (
        std::env::var_os(ENV_HOOK_SOCKET),
        std::env::var(ENV_SESSION).ok(),
        std::env::var(ENV_TERMINAL_ID).ok(),
    ) {
        (Some(socket), Some(session), Some(terminal)) => {
            #[cfg(unix)]
            let reachable = std::os::unix::net::UnixStream::connect(&socket).is_ok();
            #[cfg(not(unix))]
            let reachable = false;
            if reachable {
                c.ok("This terminal", format!("tracked (session {session}, terminal {terminal})"));
            } else {
                c.fail(
                    "This terminal",
                    format!("hook socket of session {session} unreachable"),
                    "the sshx session was restarted: open a new terminal",
                );
            }
        }
        _ => c.info("This terminal", "not inside an sshx terminal"),
    }

    if c.failed == 0 {
        println!("\n  {}  Everything works.\n", Green.paint("➜"));
    } else {
        println!(
            "\n  {}  {} check{} failed.\n",
            Yellow.paint("✗"),
            c.failed,
            if c.failed > 1 { "s" } else { "" },
        );
    }
    c.failed == 0
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
    let version_str = format!("v{}", sshx::update::VERSION);
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
    let workspace_root = args
        .workspace
        .clone()
        .or_else(|| std::env::current_dir().ok());

    // Resolve the OpenVSCode Server binary path.
    let openvscode_bin = if let Some(bin) = args.openvscode_bin.clone() {
        // Explicit override — use as-is.
        Some(bin)
    } else if args.ide {
        // Auto-download / verify.
        let path = sshx::openvscode::ensure_binary().await?;
        Some(path)
    } else {
        None
    };

    // Platform registration, if configured. It also tells which sshx server
    // to use when `--server` is not given.
    let platform_config = if args.no_platform {
        None
    } else {
        PlatformConfig::resolve(args.platform.clone(), args.platform_token.clone()).unwrap_or_else(|err| {
            warn!("invalid platform configuration: {err:#}");
            None
        })
    };
    let platform_client = platform_config.as_ref().and_then(|config| {
        PlatformClient::new(config)
            .map_err(|err| warn!("invalid platform configuration: {err:#}"))
            .ok()
    });
    let server = match (args.server.clone(), &platform_config, &platform_client) {
        (Some(server), _, _) => server,
        (None, Some(PlatformConfig { server: Some(server), .. }), _) => server.clone(),
        (None, _, Some(client)) => client
            .whoami()
            .await
            .ok()
            .and_then(|m| m.sshx_server)
            .unwrap_or_else(|| DEFAULT_SERVER.to_string()),
        (None, _, None) => DEFAULT_SERVER.to_string(),
    };

    let mut controller =
        Controller::new(
            &server,
            &name,
            runner,
            args.enable_readers,
            Some(std::path::PathBuf::from(sshx::controller::SESSION_DIR)),
            openvscode_bin,
            workspace_root.clone(),
            args.slug,
        )
        .await?;

    // Per-terminal tracking: environment, hook socket, cwd/git/process poller.
    let registry = TerminalRegistry::new();
    let mut shell_env = vec![(ENV_SESSION.to_string(), controller.name().to_string())];
    #[cfg(unix)]
    let _hook_socket = match sshx::agent::spawn_hook_server(registry.clone(), controller.name().to_string()) {
        Ok(guard) => {
            shell_env.push((ENV_HOOK_SOCKET.to_string(), guard.path().to_string_lossy().into_owned()));
            Some(guard)
        }
        Err(err) => {
            warn!("agent hook socket unavailable: {err:#}");
            None
        }
    };
    spawn_proc_poller(registry.clone());
    controller.set_shell_context(ShellContext {
        env: shell_env,
        registry: Some(registry.clone()),
    });

    // Register with the platform, which offers VS Code in each terminal's folder.
    if platform_client.is_some() && !args.no_ide {
        controller.enable_ide_install();
    }
    let platform = platform_client.map(|client| {
        let reg = Registration {
            name: controller.name().to_string(),
            url: controller.public_url().to_string(),
            sshx_server: controller.origin().to_string(),
            display_name: name.clone(),
            encryption_key: controller.encryption_key().to_string(),
            write_password: controller.write_password().map(String::from),
        };
        let url = client.base_url().to_string();
        (sshx::platform::spawn_reporter(client, reg, registry.clone()), url)
    });

    // Spawn workspace intelligence tasks.
    if !args.no_workspace {
        let workspace_root = workspace_root.unwrap_or_else(|| PathBuf::from("."));
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
        if let Some((_, url)) = &platform {
            println!("  {}  Platform: {}\n", Green.paint("➜"), Cyan.underline().paint(url));
            if !sshx::claude_hooks::is_installed() {
                println!(
                    "  {} run `sshx claude-hooks install` to report Claude Code status\n",
                    Fixed(8).paint("tip"),
                );
            }
        }
    }

    if !args.no_auto_update {
        let server = server.clone();
        let quiet = args.quiet;
        tokio::spawn(async move {
            match sshx::update::auto_update(&server).await {
                Ok(Some(version)) if !quiet => println!(
                    "  {}  sshx updated to v{version} (applies on next launch)\n",
                    Green.paint("➜"),
                ),
                Ok(_) => {}
                Err(err) => warn!("automatic update failed: {err:#}"),
            }
        });
    }

    // Spawn sshx-browser if requested.
    let mut browser_child = if let Some(browser_url) = &args.with_browser {
        match spawn_sshx_browser(&server, controller.name(), controller.token(), browser_url, args.browser_bin.as_deref()) {
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
    if let Some((handle, _)) = platform {
        handle.close().await;
    }
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
