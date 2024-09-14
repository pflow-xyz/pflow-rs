use std::error::Error;
use clap::Parser;
use tokio::task;
use pflow_cli::server::app;

#[derive(Debug, Parser)]
struct Config {
    #[clap(short = 'p', long, default_value = "3000")]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::parse();
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], config.port));
    println!("Listening on {}", addr);

    let browser_task = task::spawn(async {
        if webbrowser::open("http://localhost:3000").is_ok() {
            println!("Browser opened successfully");
        } else {
            println!("Failed to open browser");
        }
    });

    axum_server::bind(addr)
        .serve(app().into_make_service())
        .await?;

    browser_task.await?;
    Ok(())
}