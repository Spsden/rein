#[path = "../evaluation/mod.rs"]
mod evaluation;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let report = evaluation::run(&arguments).await?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    if report.passed != report.total {
        std::process::exit(1);
    }
    Ok(())
}
