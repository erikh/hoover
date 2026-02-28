use clap::Parser;

#[derive(Parser)]
#[command(name = "hoover", about = "spy on yourself for good")]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
}
