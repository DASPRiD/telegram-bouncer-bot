use std::env;
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
use log::{error, info};
use once_cell::sync::Lazy;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use structured_logger::async_json::new_writer;
use structured_logger::Builder;
use teloxide::dispatching::dialogue::serializer::Json;
use teloxide::dispatching::dialogue::{ErasedStorage, SqliteStorage, Storage};
use teloxide::types::{MessageId, ParseMode, User};
use teloxide::utils::markdown::escape;
use teloxide::{
    dispatching::{dialogue, dialogue::InMemStorage, UpdateHandler},
    prelude::*,
    types::{InlineKeyboardButton, InlineKeyboardMarkup},
    utils::command::BotCommands,
    ApiError, RequestError,
};

use crate::review::{Review, ReviewAction};

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

    #[envconfig(from = "CHANNEL_ID")]
    pub channel_id: Option<i64>,

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
    Blocked,
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
    if env::var("ENABLE_STRUCTURED_LOG").is_ok() {
        Builder::with_level(&env::var("RUST_LOG").unwrap_or("error".to_string()))
            .with_target_writer("*", new_writer(tokio::io::stdout()))
            .init();
    } else {
        env_logger::init();
    }

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

    info!("Bot started");

    Dispatcher::builder(bot, schema())
        .dependencies(dptree::deps![storage, Arc::new(config)])
        .default_handler(|_| async move {
            // We ignore any update we don't know
        })
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}

fn schema() -> UpdateHandler<Box<dyn Error + Send + Sync + 'static>> {
    use dptree::case;

    let command_handler = teloxide::filter_command::<Command, _>()
        .branch(case![Command::Start].endpoint(start))
        .branch(case![Command::Help].endpoint(help))
        .branch(case![Command::Cancel].endpoint(cancel))
        .branch(case![Command::Privacy].endpoint(privacy));

    let message_handler = Update::filter_message()
        .branch(case![State::Blocked].endpoint(blocked))
        .branch(command_handler)
        .branch(case![State::ReceiveReason].endpoint(receive_reason))
        .branch(case![State::AwaitApproval { message_id }].endpoint(await_approval));

    let callback_query_handler = Update::filter_callback_query().endpoint(review);
    let channel_post_handler = Update::filter_channel_post().endpoint(forward_channel_post);

    dialogue::enter::<Update, ErasedStorage<State>, State, _>()
        .branch(message_handler)
        .branch(callback_query_handler)
        .branch(channel_post_handler)
}

async fn forward_channel_post(bot: Bot, msg: Message, config: Arc<Config>) -> HandlerResult {
    let channel_id = match config.channel_id {
        Some(channel_id) => ChatId(channel_id),
        None => return Ok(()),
    };

    if msg.chat.id != channel_id {
        return Ok(());
    }

    let primary_chat_id = ChatId(config.primary_chat_id);
    let result = bot
        .forward_message(primary_chat_id, channel_id, msg.id)
        .await?;
    bot.pin_chat_message(primary_chat_id, result.id).await?;

    Ok(())
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

async fn blocked(bot: Bot, _dialogue: JoinDialogue, msg: Message) -> HandlerResult {
    if !msg.chat.is_private() {
        return Ok(());
    }

    let loader = loader_from_message(&msg);
    bot.send_message(msg.chat.id, fl!(loader, "blocked"))
        .await?;
    Ok(())
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

fn get_markdown_display_name(user: &User) -> String {
    let mut full_name = user.first_name.clone();

    if let Some(last_name) = user.last_name.clone() {
        full_name.push(' ');
        full_name.push_str(&last_name);
    }

    let mut display_name = format!("[{}](tg://user?id={})", escape(&full_name), user.id);

    if let Some(username) = user.username.clone() {
        display_name.push_str(&escape(&format!(" (@{})", &username)));
    }

    display_name
}

fn get_plaintext_display_name(user: &User) -> String {
    let mut display_name = user.first_name.clone();

    if let Some(last_name) = user.last_name.clone() {
        display_name.push(' ');
        display_name.push_str(&last_name);
    }

    if let Some(username) = user.username.clone() {
        display_name.push_str(&format!(" (@{})", &username));
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

    let keyboard: Vec<Vec<InlineKeyboardButton>> = vec![
        vec![
            InlineKeyboardButton::callback(
                "Approve",
                Review::new(ReviewAction::Approve, msg.chat.id, user.id, locale.clone()),
            ),
            InlineKeyboardButton::callback(
                "Deny",
                Review::new(ReviewAction::Deny, msg.chat.id, user.id, locale.clone()),
            ),
        ],
        vec![
            InlineKeyboardButton::callback(
                "Block",
                Review::new(ReviewAction::Block, msg.chat.id, user.id, locale.clone()),
            ),
            InlineKeyboardButton::callback(
                "Request contact",
                Review::new(ReviewAction::RequestContact, msg.chat.id, user.id, locale),
            ),
        ],
    ];
    let keyboard_markup = InlineKeyboardMarkup::new(keyboard);

    let moderator_message = bot
        .send_message(
            ChatId(config.moderator_chat_id),
            format!(
                "{} would like to join for the following reason:\n\n{}",
                get_markdown_display_name(user),
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

    info!(user:debug; "Join reason received");

    Ok(())
}

async fn update_review_message(
    bot: Bot,
    message: Message,
    chat_id: ChatId,
    action: ReviewAction,
    reviewer: &User,
    keyboard_markup: Option<InlineKeyboardMarkup>,
    is_bot_blocked: bool,
) -> HandlerResult {
    let mut text = match message.text() {
        Some(text) => text.to_string(),
        None => return Ok(()),
    };

    let entities = match message.entities() {
        Some(entities) => entities.to_vec(),
        None => return Ok(()),
    };

    text.push_str(&format!(
        "\n\n{} by {}",
        match action {
            ReviewAction::Approve => "Approved",
            ReviewAction::Deny => "Denied",
            ReviewAction::Block => "Blocked",
            ReviewAction::Unblock => "Unblocked",
            ReviewAction::RequestContact => "Contact requested",
        },
        get_plaintext_display_name(reviewer),
    ));

    if is_bot_blocked {
        text.push_str("\n\nUser has blocked this bot");
    }

    let mut edit_message = bot
        .edit_message_text(chat_id, message.id, &text)
        .entities(entities);

    if !is_bot_blocked {
        if let Some(keyboard_markup) = keyboard_markup {
            edit_message = edit_message.reply_markup(keyboard_markup);
        }
    }

    edit_message.await?;

    Ok(())
}

fn is_bot_blocked_result(result: Result<Message, RequestError>) -> Result<bool, RequestError> {
    match result {
        Ok(_) => Ok(false),
        Err(RequestError::Api(ApiError::BotBlocked)) => Ok(true),
        Err(error) => Err(error),
    }
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
        Err(error) => {
            error!(error:err; "Failed to parse review");
            return Ok(());
        }
    };

    let message = match query.message {
        Some(message) => message,
        None => {
            error!("Review has no message attached");
            return Ok(());
        }
    };

    info!(review:debug; "Received review");
    bot.answer_callback_query(query.id).await?;

    let loader = LANGUAGE_LOADER
        .select_languages_negotiate(&[review.locale.clone()], NegotiationStrategy::Filtering);

    let mut keyboard_markup = None;
    let is_bot_blocked;

    match review.action {
        ReviewAction::Approve => {
            let invite_link = bot
                .create_chat_invite_link(ChatId(config.primary_chat_id))
                .expire_date(Utc::now().add(TimeDelta::hours(24)))
                .member_limit(1)
                .await?;

            is_bot_blocked = is_bot_blocked_result(
                bot.send_message(
                    review.chat_id,
                    fl!(loader, "request-approved", link = invite_link.invite_link),
                )
                .await,
            )?;

            let _ = storage.remove_dialogue(review.chat_id).await;
        }
        ReviewAction::Deny => {
            is_bot_blocked = is_bot_blocked_result(
                bot.send_message(review.chat_id, fl!(loader, "request-denied"))
                    .await,
            )?;

            let _ = storage.remove_dialogue(review.chat_id).await;
        }
        ReviewAction::Block => {
            let _ = storage
                .update_dialogue(review.chat_id, State::Blocked)
                .await;

            is_bot_blocked = is_bot_blocked_result(
                bot.send_message(review.chat_id, fl!(loader, "blocked"))
                    .await,
            )?;

            let keyboard: Vec<Vec<InlineKeyboardButton>> =
                vec![vec![InlineKeyboardButton::callback(
                    "Unblock",
                    Review::new(
                        ReviewAction::Unblock,
                        review.chat_id,
                        review.user_id,
                        review.locale,
                    ),
                )]];
            keyboard_markup = Some(InlineKeyboardMarkup::new(keyboard));
        }
        ReviewAction::Unblock => {
            let _ = storage.remove_dialogue(review.chat_id).await;

            is_bot_blocked = is_bot_blocked_result(
                bot.send_message(review.chat_id, fl!(loader, "unblocked"))
                    .await,
            )?;
        }
        ReviewAction::RequestContact => {
            println!(
                "{}",
                fl!(
                    loader,
                    "contact-requested",
                    moderator = get_markdown_display_name(&query.from)
                )
            );

            is_bot_blocked = is_bot_blocked_result(
                bot.send_message(
                    review.chat_id,
                    fl!(
                        loader,
                        "contact-requested",
                        moderator = get_markdown_display_name(&query.from)
                    ),
                )
                .parse_mode(ParseMode::MarkdownV2)
                .await,
            )?;

            let _ = storage.remove_dialogue(review.chat_id).await;

            let keyboard: Vec<Vec<InlineKeyboardButton>> =
                vec![vec![InlineKeyboardButton::callback(
                    "Block",
                    Review::new(
                        ReviewAction::Block,
                        review.chat_id,
                        review.user_id,
                        review.locale,
                    ),
                )]];
            keyboard_markup = Some(InlineKeyboardMarkup::new(keyboard));
        }
    }

    update_review_message(
        bot,
        message,
        ChatId(config.moderator_chat_id),
        review.action,
        &query.from,
        keyboard_markup,
        is_bot_blocked,
    )
    .await?;

    Ok(())
}
