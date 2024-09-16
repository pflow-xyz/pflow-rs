use std::error::Error;
use clap::{Parser};
use tokio::task;
use pflow_cli::server::app;
use pflow_cli::command::{run_command, Action};

#[derive(Debug, Parser)]
struct Config {
    #[clap(short = 'p', long, default_value = "3000")]
    port: u16,

    #[clap(short = 'd', long, default_value = "false")]
    daemon: bool,

    #[clap(short = 'v', long, default_value = "false")]
    verbose: bool,

    #[clap(value_parser)]
    input: Option<Action>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::parse();
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], config.port));

    if config.daemon {
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
    } else {
        if config.input.is_none() {
            println!("No input provided");
        } else {
            run_command(config.input.expect("No input provided"));
        }
        return Ok(());
    }

    Ok(())
}
