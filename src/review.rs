use base64::prelude::BASE64_STANDARD_NO_PAD;
use base64::{DecodeError, Engine};
use i18n_embed::unic_langid::LanguageIdentifier;
use std::io::Write;
use std::str::Utf8Error;
use teloxide::prelude::ChatId;
use teloxide::types::UserId;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum ReviewAction {
    Approve = 1,
    Deny = 0,
    Block = 2,
    Unblock = 3,
    RequestContact = 4,
}

impl From<ReviewAction> for u8 {
    fn from(value: ReviewAction) -> Self {
        value as u8
    }
}

#[derive(Debug)]
pub struct InvalidReviewActionError {}

impl TryFrom<u8> for ReviewAction {
    type Error = InvalidReviewActionError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(ReviewAction::Approve),
            0 => Ok(ReviewAction::Deny),
            2 => Ok(ReviewAction::Block),
            3 => Ok(ReviewAction::Unblock),
            4 => Ok(ReviewAction::RequestContact),
            _ => Err(InvalidReviewActionError {}),
        }
    }
}

#[derive(Debug)]
pub struct Review {
    pub action: ReviewAction,
    pub chat_id: ChatId,
    pub user_id: UserId,
    pub locale: LanguageIdentifier,
}

impl Review {
    pub fn new(
        action: ReviewAction,
        chat_id: ChatId,
        user_id: UserId,
        locale: LanguageIdentifier,
    ) -> Self {
        Self {
            action,
            chat_id,
            user_id,
            locale,
        }
    }
}

impl From<Review> for String {
    fn from(review: Review) -> String {
        let locale = review.locale.to_string();

        let mut buffer = Vec::with_capacity(32);
        buffer.write_all(&[review.action.into()]).unwrap();
        buffer.write_all(&review.chat_id.0.to_le_bytes()).unwrap();
        buffer.write_all(&review.user_id.0.to_le_bytes()).unwrap();
        buffer.write_all(&[locale.len() as u8]).unwrap();
        buffer.write_all(&locale.into_bytes()).unwrap();

        BASE64_STANDARD_NO_PAD.encode(buffer)
    }
}

#[derive(Error, Debug)]
pub enum TryFromError {
    #[error("Invalid Base64 encoded string")]
    InvalidBase64,
    #[error("Buffer is too short")]
    TooShort,
    #[error("Invalid review action")]
    InvalidReviewAction,
    #[error("Invalid locale")]
    InvalidLocale,
}

impl From<InvalidReviewActionError> for TryFromError {
    fn from(_error: InvalidReviewActionError) -> Self {
        TryFromError::InvalidReviewAction
    }
}

impl From<DecodeError> for TryFromError {
    fn from(_error: DecodeError) -> Self {
        TryFromError::InvalidBase64
    }
}

impl From<Utf8Error> for TryFromError {
    fn from(_error: Utf8Error) -> Self {
        TryFromError::InvalidLocale
    }
}

impl TryFrom<String> for Review {
    type Error = TryFromError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let buffer = BASE64_STANDARD_NO_PAD.decode(value)?;

        if buffer.len() < 18 {
            return Err(TryFromError::TooShort);
        }

        let locale_length = buffer[17] as usize;

        if buffer.len() - 17 < locale_length {
            return Err(TryFromError::TooShort);
        }

        Ok(Self {
            action: buffer[0].try_into()?,
            chat_id: ChatId(i64::from_le_bytes(buffer[1..9].try_into().unwrap())),
            user_id: UserId(u64::from_le_bytes(buffer[9..17].try_into().unwrap())),
            locale: std::str::from_utf8(&buffer[18..18 + locale_length])?
                .parse()
                .unwrap(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_to_base64() {
        let review = Review::new(
            ReviewAction::Approve,
            ChatId(123456),
            UserId(987654),
            "de-DE".parse().unwrap(),
        );
        let result: String = review.into();

        assert_eq!(result, "AUDiAQAAAAAABhIPAAAAAAAFZGUtREU");
    }

    #[test]
    fn decodes_from_base64() {
        let data = "AUDiAQAAAAAABhIPAAAAAAAFZGUtREU".to_string();
        let review: Review = data.try_into().unwrap();

        assert_eq!(review.action, ReviewAction::Approve);
        assert_eq!(review.chat_id.0, 123456);
        assert_eq!(
            review.locale,
            "de-DE".parse::<LanguageIdentifier>().unwrap()
        );
    }
}
