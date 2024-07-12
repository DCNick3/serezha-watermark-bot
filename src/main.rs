use anyhow::{Context as _, Result};
use tracing::{error, info};

mod bot;
mod config;
mod grammers_boilerplate;
mod init_tracing;
mod mask_generator;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    init_tracing::init_tracing()?;

    let environment = std::env::var("ENVIRONMENT").context(
        "Please set ENVIRONMENT env var (probably you want to use either 'prod' or 'dev')",
    )?;

    let config = config::Config::load(&environment).context("Loading config has failed")?;

    info!("Resolved config: {:#?}", config);

    let client = grammers_boilerplate::connect_and_login(&config.telegram).await?;

    tokio::select!(
        _ = tokio::signal::ctrl_c() => {
            info!("Got SIGINT; quitting early gracefully");
        }
        r = bot::run_bot(&client, config.masks.clone()) => {
            match r {
                Ok(_) => info!("Got disconnected from Telegram gracefully"),
                Err(e) => error!("Error during update handling: {}", e),
            }
        }
        r = grammers_boilerplate::save_session_periodic(&client, &config.telegram) => {
            match r {
                Ok(_) => unreachable!(),
                Err(e) => error!("Error during session saving: {}", e),
            }
        }
    );

    grammers_boilerplate::save_session(&client, &config.telegram)?;

    Ok(())
}
