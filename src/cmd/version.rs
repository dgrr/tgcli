use crate::Cli;

pub fn run(_cli: &Cli) {
    println!("tgcli {}", env!("CARGO_PKG_VERSION"));
}
