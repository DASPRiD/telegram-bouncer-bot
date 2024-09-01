# Telegram Bouncer Bot

[![ci](https://github.com/DASPRiD/telegram-bouncer-bot/actions/workflows/cicd.yml/badge.svg)](https://github.com/DASPRiD/telegram-bouncer-bot/actions/workflows/cicd.yml)

A simple bot to act as a bouncer for your Telegram group. It works the following way:

- You publish a link to the bot instead of your chat link (which should be made private)
- A user chats to the bot and specifies a reason for joining your chat
- The bot will post a message to a moderator group where the members can review the request
- Upon approval the user will receive a one-time join link

# Installation

The recommended installation is through [Docker](https://www.docker.com/) or similar container runtimes. We supply
pre-built containers for this purpose. A simple setup with a `docker-compose.yml` file could look like this:

```yaml
services:
  bot:
    image: ghcr.io/dasprid/telegram-bouncer-bot:latest
    restart: unless-stopped
    volumes:
      - 'bot-data:/data'
    environment:
      - TELOXIDE_TOKEN=<telegram-bot-token>
      - STORAGE_PATH=/data
      - PRIMARY_CHAT_ID=<primary-chat-id>
      - MODERATOR_CHAT_ID=<primary-chat-id>
volumes:
  bot-data:
```

Before you can run the bot, you need to do following steps:

1. Register a bot with `@BotFather` on Telegram, this will get you a bot token for the configuration.
2. Invite `@myidbot` into your primary and moderator group in order to get their group IDs.
3. Replace the placeholder values with the real values.

The bot can also run without persistent storage. This will make the bot forget any conversations it had upon restart.
If this isn't a problem for you, simply remove the volume and the `STORAGE_PATH` env variable.

## Linked channels

When you link a public channel to your group, people can still join your group through that channel and circumvent the
bouncer entirely. It is thus recommended that you remove the link and use this bot to forward and pin messages in your
primary chat.

To do so, allow the bot to pin messages in your primary chat and add it to the channel. Then add the following new
environment variable: `CHANNEL_ID`. The bot will now forward any posts to your main chat and pin them. Edits are not
automatically forwarded, as the first reaction to a post after a few minutes triggers an false-positive edit event.

## Bot permissions

After adding the bot to both your primary and your moderator chat, you need to give the bot the following administrator
permissions:

- Primary chat: Add users or invite users via invite link
- Moderator chat: Delete messages

## Version locking

In the example above we locked the bot to the latest  version. This isn't the best choice though, as it might later
update to a new major version with breaking changes. Instead, you should lock it to a major or minor version. You
can find a list of available versions in the
[package registry](https://github.com/DASPRiD/telegram-bouncer-bot/pkgs/container/telegram-bouncer-bot).

## Internationalization

Within the moderator chat, the bot will always write messages in English. In communication with the user it will try to
detect the user's language and reply in that if available, otherwise it will fall back to English.

If you want to have another language supported you can open a pull request with your language added to the `i18n`
folder.
