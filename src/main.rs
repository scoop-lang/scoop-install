use std::path::Path;

use anyhow::Result;
use clap::Parser;
use scoop_install::cli::Cli;
use scoop_install::store::{Store, home, host};

fn execute() -> Result<i32> {
    let mut args = std::env::args_os();
    let invoked = args.next().unwrap_or_default();
    let name = Path::new(&invoked)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("scoop-install");
    if matches!(name, "scoop" | "sco" | "scoopc")
        || (name.starts_with("scoop-") && !name.starts_with("scoop-install"))
    {
        let store = Store::new(
            home()?,
            std::env::var("SCOOP_INSTALL_HOST")
                .map(Ok)
                .unwrap_or_else(|_| host())?,
            std::env::var("SCOOP_DIST_SERVER")
                .unwrap_or_else(|_| "https://release.scoop-lang.org".into()),
        )?;
        let mut args: Vec<_> = args.collect();
        let version = args
            .first()
            .and_then(|arg| arg.to_str())
            .and_then(|arg| arg.strip_prefix('+'))
            .map(str::to_owned);
        if version.is_some() {
            args.remove(0);
        }
        let selected = store.select(version.as_deref(), &std::env::current_dir()?)?;
        return store.run(&selected.version, name, &args);
    }
    Cli::parse().execute()
}

fn main() {
    match execute() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("scoop-install: {error:#}");
            std::process::exit(1);
        }
    }
}
