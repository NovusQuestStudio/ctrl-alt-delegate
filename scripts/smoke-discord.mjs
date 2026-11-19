// Live check against the running Discord desktop app: sets, verifies, and clears one activity.
import {setTimeout as delay} from 'node:timers/promises';
import {DiscordClient} from '../dist/discord.js';
import {APP_NAME, DISCORD_CLIENT_ID} from '../dist/constants.js';
const id = process.argv[2] ?? process.env.DISCORD_CLIENT_ID ?? DISCORD_CLIENT_ID;
const largeImage = process.argv[3];
if (!/^\d{17,20}$/.test(id)) throw new Error('Pass a Discord Application ID');
const client = new DiscordClient(id, undefined, undefined, console.error);
async function acknowledged() {
  const start = Date.now();
  while (!client.acknowledged) {
    if (Date.now() - start > 15_000) throw new Error(`No Discord acknowledgement (${client.status}). Is the Discord desktop app running?`);
    await delay(100);
  }
}
try {
  client.setActivity({type: 0, name: APP_NAME, details: 'Smoke test · Working', state: 'Delegating to an AI agent',
    timestamps: {start: Math.floor(Date.now() / 1000)},
    ...(largeImage ? {assets: {large_image: largeImage, large_text: APP_NAME}} : {})});
  client.start();
  await acknowledged();
  console.log(`Discord acknowledged SET_ACTIVITY as ${client.user?.username ?? 'unknown user'}. Check your profile now; clearing in 20 s.`);
  await delay(20_000);
  client.setActivity(null);
  await acknowledged(); console.log('Discord acknowledged clearing activity.');
} finally { client.stop(); }
