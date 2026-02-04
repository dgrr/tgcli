use crate::Cli;

pub fn run(_cli: &Cli) {
    println!("tgrs {}", env!("CARGO_PKG_VERSION"));
}
