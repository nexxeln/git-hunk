use clap::Parser;

fn main() {
    let cli = git_hunk::cli::Cli::parse();
    let json = cli.json();

    match git_hunk::run(cli) {
        Ok(output) => {
            if json {
                println!("{}", output.to_json_string());
            } else {
                println!("{}", output.to_text());
            }
        }
        Err(err) => {
            if json {
                eprintln!("{}", err.to_json_string());
            } else {
                eprintln!("{}", err);
            }
            std::process::exit(1);
        }
    }
}
