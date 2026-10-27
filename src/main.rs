mod cli;

#[tokio::main]
async fn main() {
    if let Err(error) = cli::main().await {
        eprintln!("rein: {error}");
        std::process::exit(1);
    }
}
