use std::io::{self, Read};
use std::process::Command;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let exe = std::env::current_exe().unwrap();
    let version =
        std::fs::read_to_string(exe.parent().unwrap().parent().unwrap().join("VERSION")).unwrap();
    match args.get(1).map(String::as_str) {
        Some("--version") => print!("{version}"),
        Some("exit") => std::process::exit(37),
        Some("stdin") => {
            let mut text = String::new();
            io::stdin().read_to_string(&mut text).unwrap();
            print!("{text}");
        }
        Some("child") => {
            let name = if cfg!(windows) {
                "scoopc.exe"
            } else {
                "scoopc"
            };
            let child = std::path::PathBuf::from(std::env::var_os("SCOOP_HOME").unwrap())
                .join("bin")
                .join(name);
            std::process::exit(
                Command::new(child)
                    .arg("--version")
                    .status()
                    .unwrap()
                    .code()
                    .unwrap(),
            );
        }
        _ => println!(
            "version={}\ntoolchain={}\ncwd={}\nargs={:?}",
            version.trim(),
            std::env::var("SCOOP_TOOLCHAIN").unwrap_or_default(),
            std::env::current_dir().unwrap().display(),
            &args[1..]
        ),
    }
}
