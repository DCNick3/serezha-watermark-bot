use std::path::Path;

use anyhow::{anyhow, Context as _, Result};
use grammers_client::{Client, Config, InitParams, SignInError};
use grammers_session::Session;
use indoc::indoc;
use tracing::{debug, info, warn};

pub async fn connect_and_login(config: &crate::config::Telegram) -> Result<Client> {
    let mut catch_up = false;

    let session = match &config.session_storage {
        Some(session_storage) => {
            let session_storage = Path::new(session_storage);
            if session_storage.exists() {
                info!("Loading saved session from {}", session_storage.display());
                // only request catch up when loading our own session, not a prepared or a new one
                catch_up = true;
                Some(Session::load_file(session_storage).context("Loading session")?)
            } else {
                info!("No session file found, creating a new session");
                None
            }
        }
        None => {
            warn!("No session storage configured, creating a new session. This will create dangling sessions on restarts!");
            None
        }
    };

    let session = match session {
        Some(session) => session,
        None => match &config.account {
            crate::config::TelegramAccount::PreparedSession { session } => {
                info!("Loading session from config");
                Session::load(session).context("Loading session")?
            }
            _ => Session::new(),
        },
    };

    let client = Client::connect(Config {
        session,
        api_id: config.api_id,
        api_hash: config.api_hash.clone(),
        params: InitParams {
            catch_up,
            ..Default::default()
        },
    })
    .await
    .context("Connecting to telegram")?;

    if !client
        .is_authorized()
        .await
        .context("failed to check whether we are signed in")?
    {
        info!("Not signed in, signing in...");

        match &config.account {
            crate::config::TelegramAccount::PreparedSession { .. } => {
                return Err(anyhow!(
                    "{}",
                    indoc!(
                        r#"Prepared session is not signed in, please sign in manually
                        and provide the session file"#
                    )
                ));
            }
            crate::config::TelegramAccount::Bot { token } => {
                info!("Signing in as bot");
                client
                    .bot_sign_in(token)
                    .await
                    .context("Signing in as bot")?;
            }
            crate::config::TelegramAccount::User { phone } => {
                info!("Signing in as user");
                let login_token = client
                    .request_login_code(phone)
                    .await
                    .context("Requesting login code")?;

                info!("Asked telegram for login code, waiting for it to be entered");

                let mut logic_code = String::new();
                std::io::stdin()
                    .read_line(&mut logic_code)
                    .context("Reading login code")?;
                let logic_code = logic_code.strip_suffix('\n').unwrap();

                match client.sign_in(&login_token, logic_code).await {
                    Ok(_) => {}
                    Err(SignInError::PasswordRequired(password_token)) => {
                        info!(
                            "2FA Password required, asking for it. Password hint: {}",
                            password_token.hint().unwrap()
                        );
                        let mut password = String::new();
                        std::io::stdin()
                            .read_line(&mut password)
                            .context("Reading password")?;
                        let password = password.strip_suffix('\n').unwrap();

                        client
                            .check_password(password_token, password)
                            .await
                            .context("Checking password")?;
                    }
                    Err(e) => {
                        return Err(e).context("Signing in as user");
                    }
                }
            }
        }

        if config.session_storage.is_some() {
            info!("Signed in, saving session");
            save_session(&client, config)?;
        } else {
            warn!("Signed in, but no session storage configured. This will leave dangling sessions on restarts!");
        }
    }

    Ok(client)
}

pub fn save_session(client: &Client, config: &crate::config::Telegram) -> Result<()> {
    if let Some(session_storage) = &config.session_storage {
        debug!("Saving session to {}", session_storage);
        std::fs::write(session_storage, client.session().save()).context("Saving session")?;
    }

    Ok(())
}

pub async fn save_session_periodic(
    client: &Client,
    config: &crate::config::Telegram,
) -> Result<()> {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60 * 5));

    loop {
        interval.tick().await;
        save_session(client, config)?;
    }
}
