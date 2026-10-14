import 'dotenv/config';
import { App } from '@slack/bolt';
import { WebClient } from '@slack/web-api';
// TODO: Remove this - it's unused and cluttering imports
import { info } from 'console';
import fs from 'fs';
const web = new WebClient(process.env.SLACK_BOT_TOKEN);
const defaultchannel = process.env.CHANNEL_TO_USE; // get from .envwait why am i hav                ng a feafualt channel lol
// TODO: Verify these channel IDs - especially CNMU9L92Q which looks different
const Channel_Confession_Logs = "C08R8LE6DDG" // real id! hopefully doesnt change or get privated
const Channel_Confession = "CNMU9L92Q" // why does this look different?
const Channel_Meta = "C0188CY57PZ"
const Prox2_Bot_ID = "B09VBHWJF4Z" // app id: A024CV4UZJ4, user id: U023L3A4UKX

const app = new App({
  token: process.env.SLACK_BOT_TOKEN,
  appToken: process.env.SLACK_APP_TOKEN,
  socketMode: true, // becuase yes
});

async function sendMessage(MessageToSend: any, channel: string = defaultchannel!) {
  const result = await web.chat.postMessage({
    channel: channel,
    text: MessageToSend
  });

  console.log(`Message sent: ${result.ts}`);
}

// TODO: Remove this test call - it sends "Hi" on every startup
sendMessage(":neocat:").catch(console.error);
// aaaaaaaaa

// TODO: Fix this function - it references post.ID and user.ID which don't exist
// Should accept parameters: post (the confession data), user (who submitted it)
async function checkStatus(information: string, post: any, user: any) { // ok so this needs to be called every new message in that ig because bots dont have pingwords
  const lower = information.toLowerCase(); // idt it needs to be used but hey i can remove it later

  switch (true) {
    case lower.includes("approved for meta"):
      // TODO: Generate proper link to the confession in #meta channel
      await sendMessage(`:perrytheplatpus: Your post ${post.ID} got approved for #meta! Here's [a link|hackclub.enterprise.slack.com/ewfrgibgo//frgthn] to it.`, user.ID)
      // ok so here we need to mark the post in confessions.json as resolved
      break;
    case lower.includes("approved"):
      // TODO: Generate proper link to the confession in #confessions channel
      await sendMessage(`:yay: Your post ${post.ID} got approved for #confessions! Here's [a link|efowediverbfuvnfeorghiubdfojrthuit] to it.`)
      // ok so here we need to mark the post in confessions.json as resolved
      break;
    case lower.includes("rejected"):
      // TODO: Pull denial reason from thread using conversations.replies({ channel: Channel_Confession_Logs, ts: postTs })
      await sendMessage(`:neocat_sad: Your post ${post.ID} was denied, link to the deny message.`) // TODO: add a thing to check the thread to see if a @crt member added a reason
      break;
    case lower.includes("unapproved"):
      await sendMessage(`:uuh: Your post ${post.ID} was unapproved and the thread has been nuked.`)
      break;
  }
}

// we call this when a confession is confirmed either via a command or some sort of user token to see when prox2 sends a message (would allow for eneph messages to be seen to)
// TODO: This function stores confession data - make sure to call it when we detect "staged as confession"
async function confessionMade(infomration: string, userId: string) {
  const lower = infomration.toLowerCase();
  // TODO: Extract the actual confession ID from the message (might need better regex)
  const confessionId = infomration.replace(/\D/g, "");
  const data = JSON.parse(fs.readFileSync("confessions.json", "utf-8"));
  console.log('we weeo')
  // TODO: Store the original message text too, not just ID and userId
  const resolved = false // always false till its true then we  save it for maybe two days (add as config at top) then delete incase it gets unapproved and/or threadlocked/threadripped from couminuty logs
  data.push({ id: confessionId, userId, resolved, date: new Date().toISOString() });
  fs.writeFileSync("confessions.json", JSON.stringify(data, null, 2));
}

async function isdisourconfession(information: string) {
  // TODO: check if the confession id is the same as one of the things we stored in our db then dm the user a link to it and it's status.
  // This should be called when we see a new confession to verify it's one we're tracking
}

// Okay so like here we recieve events when a message from Prox2 (different bot) is sent in dms ig???
// google made nned to rerewrite
// TODO: needs to know when something is thread
app.event('message', async ({ event, context, logger }) => {
  try { //make this switch case later
    // TODO: Fix this to only listen to Prox2 bot messages
    // Should check: if ('bot_id' in event && event.bot_id === Prox2_Bot_ID)
    // Then check message text for keywords like "staged as confession", "approved", "rejected", etc.
    // When "staged as confession" is found, call confessionMade(text, userId)
    // When status keywords found, call checkStatus(text, post, user) with the matching confession from confessions.json
    if ('user' in event && event.user && event.channel == defaultchannel) { //replace with confession lgos later
      console.log(`${event.user} ${'text' in event ? event.text : '[No text content]'}`);
      console.log(event.text);
      checkStatus(event.text ?? '');
    }
    // if ('user' in event && event.user && event.channel == defaultchannel) { //replace with confession channel later

    // else if ('thread_ts' in event && event.thread_ts) {
    //   console.log(`threab ${event.thread_ts} from ${event.user}: ${'text' in event ? event.text : '[No text content]'}`);
    // }
    else if ('user' in event && event.user && event.channel != defaultchannel) {
      console.log(`:) ${event.user} ${'text' in event ? event.text : '[No text content]'}`);
    }


    //  ignore messages from the bot, something that would never happen but shrug
    // should probably make it so we only listen to messages by prox2 here
    // if ('user' in event && event.user !== context.botUserId) {
    //   console.log(` New message from user ${event.user} in channel ${event.channel}:`);
    //   console.log(`Content: "${'text' in event ? event.text : '[No text content]'}"`);
    // }
  } catch (error) {
    logger.error('Error handling message event:', error);
  }
});

// while True:
//   if message contains "Staged as confession" && from prox2:
//     save to thingies.txt // find a better name
// TODO: This is the core logic we need to implement in the message listener above
// The flow is:
// 1. Receive message event from Slack
// 2. Check if it's from Prox2 bot (bot_id === Prox2_Bot_ID)
// 3. Check message text for keywords
// 4. If "staged as confession" → call confessionMade()
// 5. If "approved"/"rejected"/"unapproved"/"meta" → call checkStatus()
// 6. DM the user who submitted the confession with the update


// func Notify_User(ConfessionStatus)
//   dm(user,ConfessionStatus)
// TODO: This is essentially what sendMessage does when called with user.ID as channel

// run slack(SLACK_BOT_TOKEN)
app.start();
console.log("Bot is running...");

app.command('/mrox', async ({ command, ack, respond }) => {
  await ack();

  const confessionId = command.text?.trim();

  if (!confessionId || !/^\d+$/.test(confessionId)) {
    await respond("Welcome to Mrox! To start tracking a confession please run: /mrox {id}");
    return;
  }

  // TODO: look up the confession by ID in confessions.json and respond with its status
  await respond(`Looking up confession ${confessionId}...`);
});
