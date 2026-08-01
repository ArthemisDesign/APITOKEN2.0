#!/usr/bin/env node
/**
 * Создаёт 6 топиков forum-группы для devbot и печатает готовые env-строки.
 *
 *   DEVBOT_TELEGRAM_TOKEN=... DEVBOT_CHAT_ID=-100... node scripts/provision-topics.mjs
 *
 * Токен не логируется; ошибки Telegram печатаются с description из Bot API.
 */

const TOPICS = [
  { key: "DEVBOT_TOPIC_CRITICAL", name: "🚨 Critical", icon_color: 0xfb6f5f },
  { key: "DEVBOT_TOPIC_DEPLOYS", name: "🚀 Deploys", icon_color: 0x6fb9f0 },
  { key: "DEVBOT_TOPIC_WARNINGS", name: "⚠️ Warnings", icon_color: 0xffd67e },
  { key: "DEVBOT_TOPIC_COMMERCE", name: "💰 Commerce", icon_color: 0x8eee98 },
  { key: "DEVBOT_TOPIC_CI", name: "🧪 CI & Validation", icon_color: 0xcb86db },
  { key: "DEVBOT_TOPIC_DIGEST", name: "📊 Digest", icon_color: 0x92a8d1 },
];

const token = process.env.DEVBOT_TELEGRAM_TOKEN;
const chatId = process.env.DEVBOT_CHAT_ID;

if (!token || !chatId) {
  console.error("Usage: DEVBOT_TELEGRAM_TOKEN=... DEVBOT_CHAT_ID=-100... node scripts/provision-topics.mjs");
  process.exit(1);
}

async function call(method, body) {
  const response = await fetch(`https://api.telegram.org/bot${token}/${method}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await response.json().catch(() => null);
  if (!response.ok || !data?.ok) {
    throw new Error(`${method}: ${data?.description ?? `HTTP ${response.status}`}`);
  }
  return data.result;
}

const lines = [];
let failed = false;

for (const topic of TOPICS) {
  try {
    const result = await call("createForumTopic", {
      chat_id: Number(chatId),
      name: topic.name,
      icon_color: topic.icon_color,
    });
    const threadId = result.message_thread_id;
    console.log(`created «${topic.name}» → thread id ${threadId}`);
    lines.push(`${topic.key}=${threadId}`);
  } catch (error) {
    failed = true;
    console.error(`FAILED «${topic.name}»: ${error.message}`);
  }
}

if (lines.length > 0) {
  console.log("\n--- paste into /etc/apitoken/devbot.env ---");
  for (const line of lines) console.log(line);
}

process.exit(failed ? 1 : 0);
