use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use crate::store::{Store, home, host};

#[derive(Parser)]
#[command(version, about = "Install and select Scoop toolchains and targets")]
pub struct Cli {
    #[arg(long, env = "SCOOP_HOME", global = true)]
    home: Option<PathBuf>,
    #[arg(long, env = "SCOOP_INSTALL_HOST", global = true)]
    host: Option<String>,
    #[arg(
        long,
        env = "SCOOP_DIST_SERVER",
        default_value = "https://release.scoop-lang.org",
        global = true
    )]
    dist_server: String,
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    #[command(subcommand)]
    Toolchain(Toolchain),
    #[command(subcommand)]
    Target(Target),
    #[command(subcommand)]
    Component(Component),
    Default {
        version: String,
    },
    Run {
        version: String,
        command: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    Show,
    Update {
        #[arg(default_value = "latest")]
        version: String,
        #[arg(long)]
        manifest: Option<String>,
        #[arg(long)]
        skip_docs: bool,
    },
    Docs {
        #[command(flatten)]
        selection: ToolchainOption,
        #[arg(long, value_delimiter = ',')]
        target: Vec<String>,
    },
}

#[derive(Subcommand)]
enum Toolchain {
    Install {
        #[arg(default_value = "latest")]
        version: String,
        #[arg(long)]
        manifest: Option<String>,
        #[arg(long, value_delimiter = ',')]
        target: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        component: Vec<String>,
        #[arg(long)]
        skip_docs: bool,
    },
    List,
    Remove {
        version: String,
    },
}

#[derive(Args)]
struct ToolchainOption {
    #[arg(long)]
    toolchain: Option<String>,
}

#[derive(Args)]
struct TargetChange {
    #[arg(required = true)]
    targets: Vec<String>,
    #[command(flatten)]
    selection: ToolchainOption,
    #[arg(long)]
    skip_docs: bool,
}

#[derive(Subcommand)]
enum Target {
    Add(TargetChange),
    Remove(TargetChange),
    List {
        #[command(flatten)]
        selection: ToolchainOption,
        #[arg(long)]
        installed: bool,
    },
}

#[derive(Args)]
struct ComponentChange {
    name: String,
    #[arg(long)]
    target: Option<String>,
    #[command(flatten)]
    selection: ToolchainOption,
    #[arg(long)]
    skip_docs: bool,
}

#[derive(Subcommand)]
enum Component {
    Add(ComponentChange),
    Remove(ComponentChange),
    List {
        #[command(flatten)]
        selection: ToolchainOption,
        #[arg(long)]
        installed: bool,
    },
}

impl ToolchainOption {
    fn version(&self, store: &Store) -> Result<String> {
        Ok(store
            .select(self.toolchain.as_deref(), &std::env::current_dir()?)?
            .version)
    }
}

impl Cli {
    pub fn execute(self) -> Result<i32> {
        let store = Store::new(
            self.home.map(Ok).unwrap_or_else(home)?,
            self.host.map(Ok).unwrap_or_else(host)?,
            self.dist_server,
        )?;
        match self.command {
            Action::Toolchain(Toolchain::Install {
                version,
                manifest,
                target,
                component,
                skip_docs,
            }) => {
                store.install(
                    &version,
                    manifest.as_deref(),
                    &target,
                    &component,
                    skip_docs,
                )?;
            }
            Action::Toolchain(Toolchain::List) => {
                let default = store.settings()?.default;
                for state in store.list()? {
                    println!(
                        "{} {}{}",
                        state.manifest.version,
                        state.host,
                        if default.as_ref() == Some(&state.manifest.version) {
                            " (default)"
                        } else {
                            ""
                        }
                    );
                }
            }
            Action::Toolchain(Toolchain::Remove { version }) => store.remove(&version)?,
            Action::Default { version } => {
                let _guard = store.toolchain_lock(&version)?;
                store.set_default(&version, false)?;
            }
            Action::Run {
                version,
                command,
                args,
            } => return store.run(&version, &command, &args),
            Action::Show => {
                let selected = store.select(None, &std::env::current_dir()?)?;
                let state = store.state(&selected.version)?;
                println!(
                    "Toolchain: {}\nHost: {}\nSelected by: {}\nRoot: {}\nTargets: {}\nComponents: {}",
                    selected.version,
                    store.host,
                    selected.reason,
                    store.root(&selected.version)?.display(),
                    state.targets().join(", "),
                    state
                        .components
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            Action::Target(Target::Add(args)) => store.targets_change(
                &args.selection.version(&store)?,
                &args.targets,
                true,
                args.skip_docs,
            )?,
            Action::Target(Target::Remove(args)) => store.targets_change(
                &args.selection.version(&store)?,
                &args.targets,
                false,
                args.skip_docs,
            )?,
            Action::Target(Target::List {
                selection,
                installed,
            }) => {
                let state = store.state(&selection.version(&store)?)?;
                for target in &state.manifest.host(&store.host)?.targets {
                    let present = state
                        .components
                        .contains(&state.manifest.targets[target].base);
                    if !installed || present {
                        println!("{target}{}", if present { " (installed)" } else { "" });
                    }
                }
            }
            Action::Component(Component::Add(args)) => store.component_change(
                &args.selection.version(&store)?,
                &args.name,
                args.target.as_deref(),
                true,
                args.skip_docs,
            )?,
            Action::Component(Component::Remove(args)) => store.component_change(
                &args.selection.version(&store)?,
                &args.name,
                args.target.as_deref(),
                false,
                args.skip_docs,
            )?,
            Action::Component(Component::List {
                selection,
                installed,
            }) => {
                let state = store.state(&selection.version(&store)?)?;
                let mut names = vec![
                    ("sources".into(), state.manifest.sources.clone()),
                    ("vendor".into(), state.manifest.vendor.clone()),
                ];
                for target in &state.manifest.host(&store.host)?.targets {
                    if let Some(gui) = &state.manifest.targets[target].gui {
                        names.push((format!("gui --target {target}"), gui.clone()));
                    }
                }
                for (name, component) in names {
                    let present = state.components.contains(&component);
                    if !installed || present {
                        println!("{name}{}", if present { " (installed)" } else { "" });
                    }
                }
            }
            Action::Update {
                version,
                manifest,
                skip_docs,
            } => store.update(&version, manifest.as_deref(), skip_docs)?,
            Action::Docs { selection, target } => {
                let version = selection.version(&store)?;
                let _guard = store.toolchain_lock(&version)?;
                store.docs(&store.state(&version)?, &target)?;
            }
        }
        Ok(0)
    }
}
