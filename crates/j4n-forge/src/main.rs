//! `j4n-forge make --seed N --out DIR` · `j4n-forge play SPEC [ARGS…]` · `j4n-forge upgrade DIR` · `j4n-forge show --seed N`.

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("play") if args.len() >= 2 => j4n_forge::play::play(Path::new(&args[1]), args[2..].to_vec()),
        Some("make") => match (flag(&args, "--seed").and_then(|s| s.parse::<u64>().ok()), flag(&args, "--out")) {
            (Some(seed), Some(out)) => {
                let made = if args.iter().any(|a| a == "--agent") { j4n_forge::spec::make_agent(seed, Path::new(&out)) } else { j4n_forge::spec::make(seed, Path::new(&out)) };
                say(made.map(|s| println!("{} {}", s.name, out)))
            }
            _ => usage(),
        },
        Some("show") => match flag(&args, "--seed").and_then(|s| s.parse::<u64>().ok()) {
            Some(seed) => {
                let s = j4n_forge::spec::draw(seed);
                println!("{} {}", s.name, j4n_forge::spec::axes_line(&s));
                0
            }
            None => usage(),
        },
        Some("upgrade") if args.len() == 2 => say(std::fs::write(Path::new(&args[1]).join("upgraded"), "1")),
        _ => usage(),
    };
    std::process::exit(code);
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn say<E: std::fmt::Display>(r: Result<(), E>) -> i32 {
    match r {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("j4n-forge: {e}");
            1
        }
    }
}

fn usage() -> i32 {
    eprintln!("usage: j4n-forge make --seed N --out DIR | play SPEC [ARGS…] | upgrade DIR | show --seed N");
    2
}
