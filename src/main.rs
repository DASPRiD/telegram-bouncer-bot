use std::error::Error;
use std::ops::Add;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{TimeDelta, Utc};
use envconfig::Envconfig;
use i18n_embed::fluent::{fluent_language_loader, FluentLanguageLoader, NegotiationStrategy};
use i18n_embed::unic_langid::LanguageIdentifier;
use i18n_embed::LanguageLoader;
use i18n_embed_fl::fl;
use once_cell::sync::Lazy;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use teloxide::dispatching::dialogue::serializer::Json;
use teloxide::dispatching::dialogue::{ErasedStorage, SqliteStorage, Storage};
use teloxide::types::{MessageId, ParseMode, User};
use teloxide::utils::markdown::escape;
use teloxide::{
    dispatching::{dialogue, dialogue::InMemStorage, UpdateHandler},
    prelude::*,
    types::{InlineKeyboardButton, InlineKeyboardMarkup},
    utils::command::BotCommands,
};

use crate::review::Review;

mod review;

type JoinDialogue = Dialogue<State, ErasedStorage<State>>;
type JoinStorage = Arc<ErasedStorage<State>>;
type HandlerResult = Result<(), Box<dyn Error + Send + Sync>>;

#[derive(Envconfig)]
pub struct Config {
    #[envconfig(from = "PRIMARY_CHAT_ID")]
    pub primary_chat_id: i64,

    #[envconfig(from = "MODERATOR_CHAT_ID")]
    pub moderator_chat_id: i64,

    #[envconfig(from = "STORAGE_PATH")]
    pub storage_path: Option<PathBuf>,
}

#[derive(BotCommands, Clone)]
#[command(
    rename_rule = "lowercase",
    description = "These commands are supported:"
)]
enum Command {
    #[command(description = "display help")]
    Help,
    #[command(description = "display privacy policy")]
    Privacy,
    #[command(description = "request a join link")]
    Start,
    #[command(description = "cancel join request")]
    Cancel,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub enum State {
    #[default]
    Start,
    ReceiveReason,
    AwaitApproval {
        message_id: MessageId,
    },
}

#[derive(RustEmbed)]
#[folder = "i18n/"]
struct Localizations;

static LANGUAGE_LOADER: Lazy<FluentLanguageLoader> = Lazy::new(|| {
    let loader: FluentLanguageLoader = fluent_language_loader!();

    loader
        .load_available_languages(&Localizations)
        .expect("Error while loading languages");

    loader
});

#[tokio::main]
async fn main() {
    pretty_env_logger::init();
    let bot = Bot::from_env();

    let loader: FluentLanguageLoader = fluent_language_loader!();
    loader
        .load_languages(&Localizations, &[loader.fallback_language()])
        .unwrap();

    let config = Config::init_from_env().unwrap();

    let storage: JoinStorage = if let Some(storage_path) = config.storage_path.clone() {
        SqliteStorage::open(
            storage_path.join("dialogues.sqlite").to_str().unwrap(),
            Json,
        )
        .await
        .unwrap()
        .erase()
    } else {
        InMemStorage::new().erase()
    };

    Dispatcher::builder(bot, schema())
        .dependencies(dptree::deps![storage, Arc::new(config)])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}

fn schema() -> UpdateHandler<Box<dyn Error + Send + Sync + 'static>> {
    use dptree::case;

    let command_handler = teloxide::filter_command::<Command, _>()
        .branch(
            case![State::Start]
                .branch(case![Command::Help].endpoint(help))
                .branch(case![Command::Start].endpoint(start)),
        )
        .branch(case![Command::Cancel].endpoint(cancel))
        .branch(case![Command::Privacy].endpoint(privacy));

    let message_handler = Update::filter_message()
        .branch(command_handler)
        .branch(case![State::ReceiveReason].endpoint(receive_reason))
        .branch(case![State::AwaitApproval { message_id }].endpoint(await_approval));

    let callback_query_handler = Update::filter_callback_query().endpoint(review);

    dialogue::enter::<Update, ErasedStorage<State>, State, _>()
        .branch(message_handler)
        .branch(callback_query_handler)
}

fn locale_from_message(msg: &Message) -> LanguageIdentifier {
    match msg.from() {
        Some(user) => user.language_code.clone().unwrap_or("en".to_string()),
        None => "en".to_string(),
    }
    .parse()
    .unwrap_or_else(|_| "en".to_string().parse().unwrap())
}

fn loader_from_message(msg: &Message) -> FluentLanguageLoader {
    LANGUAGE_LOADER
        .select_languages_negotiate(&[locale_from_message(msg)], NegotiationStrategy::Filtering)
}

async fn start(bot: Bot, dialogue: JoinDialogue, msg: Message) -> HandlerResult {
    if !msg.chat.is_private() {
        return Ok(());
    }

    let loader = loader_from_message(&msg);
    bot.send_message(msg.chat.id, fl!(loader, "reason-prompt"))
        .await?;
    dialogue.update(State::ReceiveReason).await?;
    Ok(())
}

async fn help(bot: Bot, msg: Message) -> HandlerResult {
    if !msg.chat.is_private() {
        return Ok(());
    }

    bot.send_message(msg.chat.id, Command::descriptions().to_string())
        .await?;
    Ok(())
}

async fn cancel(
    bot: Bot,
    dialogue: JoinDialogue,
    msg: Message,
    config: Arc<Config>,
) -> HandlerResult {
    if !msg.chat.is_private() {
        return Ok(());
    }

    if let Some(State::AwaitApproval { message_id }) = dialogue.get().await? {
        bot.delete_message(ChatId(config.moderator_chat_id), message_id)
            .await?;
    }

    let loader = loader_from_message(&msg);
    bot.send_message(msg.chat.id, fl!(loader, "cancelling-join-request"))
        .await?;
    dialogue.exit().await?;
    Ok(())
}

async fn privacy(bot: Bot, msg: Message) -> HandlerResult {
    let loader = loader_from_message(&msg);
    bot.send_message(msg.chat.id, fl!(loader, "privacy-policy"))
        .await?;
    Ok(())
}

async fn await_approval(bot: Bot, msg: Message) -> HandlerResult {
    let loader = loader_from_message(&msg);
    bot.send_message(msg.chat.id, fl!(loader, "under-review"))
        .await?;
    Ok(())
}

fn get_display_name(user: &User) -> String {
    let mut display_name = user.first_name.clone();

    if let Some(last_name) = user.last_name.clone() {
        display_name.push(' ');
        display_name.push_str(&last_name);
    }

    if let Some(username) = user.username.clone() {
        display_name.push_str(&format!(" (@{})", username));
    }

    display_name
}

async fn receive_reason(
    bot: Bot,
    dialogue: JoinDialogue,
    msg: Message,
    config: Arc<Config>,
) -> HandlerResult {
    let locale = locale_from_message(&msg);
    let loader =
        LANGUAGE_LOADER.select_languages_negotiate(&[&locale], NegotiationStrategy::Filtering);

    let reason = match msg.text() {
        Some(text) => text.to_owned(),
        None => {
            bot.send_message(msg.chat.id, fl!(loader, "reason-missing"))
                .await?;
            return Ok(());
        }
    };

    let user = match msg.from() {
        Some(user) => user,
        None => return Ok(()),
    };

    let keyboard: Vec<Vec<InlineKeyboardButton>> = vec![vec![
        InlineKeyboardButton::callback(
            "Approve",
            Review::new(true, msg.chat.id, user.id, locale.clone()),
        ),
        InlineKeyboardButton::callback("Deny", Review::new(false, msg.chat.id, user.id, locale)),
    ]];
    let keyboard_markup = InlineKeyboardMarkup::new(keyboard);

    let display_name = get_display_name(user);
    let moderator_message = bot
        .send_message(
            ChatId(config.moderator_chat_id),
            format!(
                "[{}](tg://user?id={}) would like to join for the following reason:\n\n{}",
                escape(&display_name),
                user.id,
                escape(reason.trim()),
            ),
        )
        .parse_mode(ParseMode::MarkdownV2)
        .reply_markup(keyboard_markup)
        .await?;

    bot.send_message(msg.chat.id, fl!(loader, "reason-received"))
        .await?;
    dialogue
        .update(State::AwaitApproval {
            message_id: moderator_message.id,
        })
        .await?;
    Ok(())
}

async fn update_review_message(
    bot: Bot,
    message: Message,
    chat_id: ChatId,
    approved: bool,
    reviewer: &User,
) -> HandlerResult {
    let previous_text = match message.text() {
        Some(text) => text,
        None => return Ok(()),
    };

    let display_name = get_display_name(reviewer);
    let mut new_text = previous_text.to_string();
    new_text.push_str(&format!(
        "\n\n{} by [{}](tg://user?id={})",
        if approved { "Approved" } else { "Denied" },
        display_name,
        reviewer.id
    ));

    bot.edit_message_text(chat_id, message.id, escape(&new_text))
        .parse_mode(ParseMode::MarkdownV2)
        .reply_markup(InlineKeyboardMarkup::default())
        .await?;

    Ok(())
}

async fn review(
    bot: Bot,
    query: CallbackQuery,
    storage: JoinStorage,
    config: Arc<Config>,
) -> HandlerResult {
    let data = match query.data {
        Some(data) => data,
        None => return Ok(()),
    };

    let review: Review = match data.try_into() {
        Ok(review) => review,
        Err(_) => return Ok(()),
    };

    let message = match query.message {
        Some(message) => message,
        None => return Ok(()),
    };

    bot.answer_callback_query(query.id).await?;
    let _ = storage.remove_dialogue(review.chat_id).await;

    let loader = LANGUAGE_LOADER
        .select_languages_negotiate(&[review.locale], NegotiationStrategy::Filtering);

    if review.approved {
        let invite_link = bot
            .create_chat_invite_link(ChatId(config.primary_chat_id))
            .expire_date(Utc::now().add(TimeDelta::hours(24)))
            .member_limit(1)
            .await?;

        bot.send_message(
            review.chat_id,
            fl!(loader, "request-approved", link = invite_link.invite_link),
        )
        .await?;
    } else {
        bot.send_message(review.chat_id, fl!(loader, "request-denied"))
            .await?;
    }

    update_review_message(
        bot,
        message,
        ChatId(config.moderator_chat_id),
        review.approved,
        &query.from,
    )
    .await?;

    Ok(())
}
