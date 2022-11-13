use clap::Parser;

#[derive(Parser)]
#[command(author, version, about, long_about = None, arg_required_else_help(true))]
struct Cli {
    /// Extract the contents of the hog file
    #[arg(short = 'x', long)]
    extract: bool,

    /// The files to operate on (1 or more)
    #[arg(required = true)]
    file: Vec<String>,
}

fn main() {
    let _cli = Cli::parse();
}
