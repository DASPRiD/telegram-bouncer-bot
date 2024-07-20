# Telegram Bouncer Bot

[![ci](https://github.com/DASPRiD/telegram-bouncer-bot/actions/workflows/cicd.yml/badge.svg)](https://github.com/DASPRiD/telegram-bouncer-bot/actions/workflows/cicd.yml)

A simple bot to act as a bouncer for your Telegram group. It works the following way:

- You publish a link to the bot instead of your chat link (which should be made private)
- A user chats to the bot and specifies a reason for joining your chat
- The bot will post a message to a moderator group where the members can review the request
- Upon approval the user will receive a one-time join link
