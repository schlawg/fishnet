use structopt::StructOpt;
use std::fs;
use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use std::num::{ParseIntError, NonZeroU32};
use std::time::Duration;
use url::Url;
use configparser::ini::Ini;

const DEFAULT_ENDPOINT: &str = "https://lichess.org/fishnet";

/// Distributed Stockfish analysis for lichess.org.
#[derive(Debug, StructOpt)]
pub struct Opt {
    /// Increase verbosity.
    #[structopt(flatten)]
    verbose: Verbose,

    /// Automatically install available updates on startup and at random
    /// intervals.
    #[structopt(long, global = true)]
    auto_update: bool,

    /// Configuration file.
    #[structopt(long, parse(from_os_str), default_value = "fishnet.ini", global = true)]
    conf: PathBuf,

    /// Do not use a configuration file.
    #[structopt(long, conflicts_with = "conf", global = true)]
    no_conf: bool,

    /// Fishnet API key.
    #[structopt(long, alias = "apikey", short = "k", global = true)]
    key: Option<String>,

    /// Lichess HTTP endpoint.
    #[structopt(long, global = true)]
    endpoint: Option<Url>,

    /// Number of logical CPU cores to use for engine processes
    /// (or auto for n - 1, or all for n).
    #[structopt(long, alias = "threads", global = true)]
    cores: Option<Cores>,

    /// Prefer to run high-priority jobs only if older than this duration
    /// (for example 120s).
    #[structopt(long, global = true)]
    user_backlog: Option<Backlog>,

    /// Prefer to run low-priority jobs only if older than this duration
    /// (for example 2h).
    #[structopt(long, global = true)]
    system_backlog: Option<Backlog>,

    #[structopt(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Default, StructOpt)]
struct Verbose {
    #[structopt(name = "verbose", short = "v", parse(from_occurrences), global = true)]
    level: u32,
}

#[derive(Debug)]
enum Cores {
    Auto,
    All,
    Number(NonZeroU32),
}

impl Default for Cores {
    fn default() -> Cores {
        Cores::Auto
    }
}

impl FromStr for Cores {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(if s == "auto" {
            Cores::Auto
        } else if s == "all" {
            Cores::All
        } else {
            Cores::Number(s.parse()?)
        })
    }
}

#[derive(Debug)]
enum Backlog {
    Short,
    Long,
    Duration(Duration),
}

impl Default for Backlog {
    fn default() -> Backlog {
        Backlog::Duration(Duration::default())
    }
}

impl FromStr for Backlog {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(if s == "short" {
            Backlog::Short
        } else if s == "long" {
            Backlog::Long
        } else {
            let (s, factor) = if let Some(s) = s.strip_suffix("d") {
                (s, 60 * 60 * 24)
            } else if let Some(s) = s.strip_suffix("h") {
                (s, 60 * 60)
            } else if let Some(s) = s.strip_suffix("m") {
                (s, 60)
            } else {
                (s.strip_suffix("s").unwrap_or(s), 1)
            };
            Backlog::Duration(Duration::from_secs(u64::from(s.trim().parse::<u32>()?) * factor))
        })
    }
}

#[derive(StructOpt, Debug, PartialEq, Eq)]
enum Command {
    /// Donate CPU time by running analysis (default).
    Run,
    /// Run interactive configuration.
    Configure,
    /// Generate a systemd service file.
    Systemd,
    /// Generate a systemd user service file.
    SystemdUser,
    /// Show debug information about OS and CPU.
    Cpuid,
}

#[derive(Debug, Default)]
struct Config {
    endpoint: Option<Url>,
    key: Option<String>,
    cores: Option<Cores>,
    user_backlog: Option<Backlog>,
    system_backlog: Option<Backlog>,
}

fn intro() {
    println!(r#".   _________         .    ."#);
    println!(r#".  (..       \_    ,  |\  /|"#);
    println!(r#".   \       O  \  /|  \ \/ /"#);
    println!(r#".    \______    \/ |   \  /      _____ _     _     _   _      _"#);
    println!(r#".       vvvv\    \ |   /  |     |  ___(_)___| |__ | \ | | ___| |_"#);
    println!(r#".       \^^^^  ==   \_/   |     | |_  | / __| '_ \|  \| |/ _ \ __|"#);
    println!(r#".        `\_   ===    \.  |     |  _| | \__ \ | | | |\  |  __/ |_"#);
    println!(r#".        / /\_   \ /      |     |_|   |_|___/_| |_|_| \_|\___|\__| {}"#, env!("CARGO_PKG_VERSION"));
    println!(r#".        |/   \_  \|      /"#);
    println!(r#".               \________/      Distributed Stockfish analysis for lichess.org"#);
}

pub fn parse_and_configure() -> Opt {
    let opt = Opt::from_args();

    // Show intro.
    match opt.command {
        Some(Command::Systemd) | Some(Command::SystemdUser) => (),
        _ => intro(),
    }

    // Handle config file.
    if !opt.no_conf || opt.command == Some(Command::Configure) {
        let mut ini = Ini::new();
        ini.set_default_section("Fishnet");

        // Load ini.
        let file_found = match fs::read_to_string(&opt.conf) {
            Ok(contents) => {
                ini.read(contents).expect("parse config file");
                true
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => false,
            Err(err) => panic!("failed to open config file: {}", err),
        };

        // Configuration dialog.
        if !file_found || opt.command == Some(Command::Configure) {
            eprintln!();
            eprintln!("### Configuration");

            eprintln!();
            let endpoint = loop {
                let mut endpoint = String::new();
                eprint!("Endpoint (default: {}): ", ini.get("Fishnet", "Endpoint").unwrap_or(DEFAULT_ENDPOINT.to_owned()));
                io::stderr().flush().expect("flush stderr");
                io::stdin().read_line(&mut endpoint).expect("read endpoint from stdin");

                let endpoint = Some(endpoint.trim().to_owned())
                    .filter(|e| !e.is_empty())
                    .or_else(|| ini.get("Fishnet", "Endpoint").map(|e| e.trim().to_owned()))
                    .unwrap_or(DEFAULT_ENDPOINT.to_owned());

                match Url::from_str(&endpoint) {
                    Ok(url) => {
                        ini.setstr("Fishnet", "Endpoint", Some(&endpoint));
                        break opt.endpoint.clone().unwrap_or(url);
                    }
                    Err(err) => eprintln!("Invalid: {}", err),
                }
            };

            eprintln!();
            loop {
                let mut key = String::new();
                let required = if let Some(current) = ini.get("Fishnet", "Key") {
                    eprint!("Personal fishnet key (append ! to force, default: keep {}): ", "*".repeat(current.trim().len()));
                    false
                } else if endpoint.host_str() == Some("lichess.org") {
                    eprint!("Personal fishnet key (append ! to force, https://lichess.org/get-fishnet): ");
                    true
                } else {
                    eprint!("Personal fishnet key (append ! to force, probably not required): ");
                    false
                };

                io::stderr().flush().expect("flush stderr");
                io::stdin().read_line(&mut key).expect("read key from stdin");

                let key = key.trim();
                let key = if key.is_empty() {
                    if required {
                        eprintln!("Key required.");
                        continue;
                    } else {
                        break;
                    }
                } else if let Some(key) = key.strip_suffix("!") {
                    key
                } else {
                    // TODO: Validate.
                    key
                };

                ini.setstr("Fishnet", "Key", Some(key));
                break;
            }

            eprintln!();
            let mut cores = String::new();
            eprint!("Number of logical cores to use for engine threads (default {}, max {}): ", 3, 4);
            io::stderr().flush().expect("flush stderr");
            io::stdin().read_line(&mut cores).expect("read cores from stdin");

            eprintln!();
            let mut backlog = String::new();
            eprintln!("You can choose to join only if a backlog is building up. Examples:");
            eprintln!("* Rented server exclusively for fishnet: choose no");
            eprintln!("* Running on a laptop: choose yes");
            eprint!("Would you prefer to keep your client idle? (default: no) ");
            io::stderr().flush().expect("flush stderr");
            io::stdin().read_line(&mut backlog).expect("read backlog from stdin");

            eprintln!();
            let mut write = String::new();
            eprint!("Done. Write configuration to {:?} now? (default: yes) ", opt.conf);
            io::stderr().flush().expect("flush stderr");
            io::stdin().read_line(&mut write).expect("read confirmation from stdin");
        }
    }

    opt
}
