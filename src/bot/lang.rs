//! Contains all the text messages to be sent to the user

use displaydoc::Display;

#[derive(Debug, Display)]
pub enum Lang {
    /// Send me an image, please
    NotImage,

    /// Wowking...
    StatusWorking,

    /// Here's your processed images
    ResultSuccess,
    /**
    Some error occurred:

    {0}*/
    ResultGenericError(String),
}

impl From<Lang> for grammers_client::InputMessage {
    fn from(value: Lang) -> Self {
        let str = format!("{value}");
        grammers_client::InputMessage::text(str)
    }
}
