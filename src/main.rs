mod cli;
mod host_wire;

fn main() {
    let result = if std::env::args().skip(1).eq(["host-v1"]) {
        host_wire::main().map_err(Into::into)
    } else {
        cli_main()
    };
    if let Err(error) = result {
        eprintln!("rein: {error}");
        std::process::exit(1);
    }
}

#[tokio::main]
async fn cli_main() -> Result<(), Box<dyn std::error::Error>> {
    cli::main().await
}
