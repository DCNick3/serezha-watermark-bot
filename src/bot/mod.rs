mod lang;

use crate::bot::lang::Lang;
use crate::config;
use crate::config::NamedPreset;
use anyhow::{Context as _, Result};
use grammers_client::types::{Chat, Downloadable, Media, Message};
use grammers_client::{Client, InputMessage, Update};
use std::io::Cursor;
use tracing::{debug, error, info, instrument};

pub async fn run_bot(client: &Client, mask_config: config::Mask) -> Result<()> {
    while let Some(update) = client.next_update().await.context("Getting next update")? {
        let Update::NewMessage(message) = update else {
            continue;
        };
        if message.outgoing() {
            continue;
        }

        let client = client.clone();
        let mask_config = mask_config.clone();
        tokio::spawn(async move {
            // error are logged by tracing instrument macro
            let _ = handle_message(message, client, mask_config).await;
        });
    }

    info!("Stopped getting updates!");
    Ok(())
}

pub enum MessageResult {
    Reply(InputMessage),
    Ignore,
}

#[instrument(skip_all, fields(chat_id = message.chat().id(), username = message.chat().username()), err(Debug))]
async fn handle_message(message: Message, client: Client, mask_config: config::Mask) -> Result<()> {
    let result = handle_message_impl(&message, client, mask_config).await;

    // reply to the user if there's an error or the handler requested a reply.
    // any error here will only be reported to the tracing, not to the user (because sending a message after a failed message will probably fail too..)
    match result {
        Ok(MessageResult::Reply(reply)) => {
            message
                .reply(reply)
                .await
                .context("Replying to the message")?;
        }
        Ok(MessageResult::Ignore) => {}
        Err(e) => {
            error!("Error while handing a message: {:?}", e);
            let mut report = format!("{:?}", e);

            // TODO: truncate in a nicer way, with a message that the report was truncated
            report.truncate(4096 - 24); // with some chars to spare

            // TODO: make the error a code block
            // the markdown parser seems a bit buggy, so can't really use it here.
            // TODO: and now that a Lang is here, it's even less clear as to how
            message
                .reply(Lang::ResultGenericError(report))
                .await
                .context("Sending the error message to the user")?;
        }
    };

    Ok(())
}

#[instrument(skip_all, fields(chat_id = message.chat().id(), username = message.chat().username()))]
async fn handle_message_impl(
    message: &Message,
    client: Client,
    mask_config: config::Mask,
) -> Result<MessageResult> {
    let chat = message.chat();
    debug!("Got message from {:?}", chat.id());
    if !matches!(chat, Chat::User(_)) {
        info!("Ignoring message not from private chat ({:?})", chat);
    }

    let Some(Media::Photo(photo)) = message.media() else {
        return Ok(MessageResult::Reply(Lang::NotImage.into()));
    };

    let status_message = message
        .reply(Lang::StatusWorking)
        .await
        .context("Sending status message")?;

    // we are not limiting the photo size
    // Telegram already has reasonable limits, right?

    let mut photo_data = Vec::new();
    let mut download_iter = client.iter_download(&Downloadable::Media(Media::Photo(photo)));
    while let Some(chunk) = download_iter
        .next()
        .await
        .context("Downloading photo chunk")?
    {
        photo_data.extend_from_slice(&chunk);
    }

    let results = tokio::task::spawn_blocking(|| {
        let image = image::load(Cursor::new(photo_data), image::ImageFormat::Jpeg)?.to_rgb8();

        let mut results = Vec::new();
        for NamedPreset { name, preset } in mask_config.presets {
            let mut image = image.clone();
            crate::mask_generator::apply_mask(preset, &mut image);

            let mut result = Vec::new();
            image.write_to(&mut Cursor::new(&mut result), image::ImageFormat::Jpeg)?;
            results.push((name, result));
        }

        Ok::<_, anyhow::Error>(results)
    })
    .await??;

    status_message
        .delete()
        .await
        .context("Deleting status message")?;

    for (name, result) in results {
        let size = result.len();
        let result_file = client
            .upload_stream(
                &mut Cursor::new(result),
                size,
                "masked_image.jpg".to_string(),
            )
            .await
            .context("Uploading masked image")?;

        message
            .reply(InputMessage::text(name).photo(result_file))
            .await
            .context("Sending the result")?;
    }

    Ok(MessageResult::Ignore)
}
