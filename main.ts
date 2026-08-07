import 'dotenv/config';
import { App } from '@slack/bolt';
import { WebClient } from '@slack/web-api';
import { info } from 'console';
import fs from 'fs';
const web = new WebClient(process.env.SLACK_BOT_TOKEN);
const defaultchannel = process.env.CHANNEL_TO_USE; // get from .envwait why am i having a feafualt channel lol
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

sendMessage("Hi").catch(console.error);
// aaaaaaaaa
async function checkStatus(information: string) { // ok so this needs to be called every new message in that ig because bots dont have pingwords
  const lower = information.toLowerCase(); // idt it needs to be used but hey i can remove it later

  switch (true) {
    case lower.includes("approved for meta"):
      await sendMessage(`:perrytheplatpus: Your post ${post.ID} got approved for #meta! Here's [a link|hackclub.enterprise.slack.com/ewfrgibgo//frgthn] to it.`, user.ID)
      // ok so here we need to mark the post in confessions.json as resolved
      break;
    case lower.includes("approved"):
      await sendMessage(`:yay: Your post ${post.ID} got approved for #confessions! Here's [a link|efowediverbfuvnfeorghiubdfojrthuit] to it.`)
      // ok so here we need to mark the post in confessions.json as resolved
      break;
    case lower.includes("rejected"):
      await sendMessage(`:neocat_sad: Your post ${post.ID} was denied, link to the deny message.`) // TODO: add a thing to check the thread to see if a @crt member added a reason
      break;
    case lower.includes("unapproved"):
      await sendMessage(`:uuh: Your post ${post.ID} was unapproved and the thread has been nuked.`)
      break;
  }
}
// call this when a confession is sent to prox2,
async function confessionMade(infomration: string, userId: string) {
  const lower = infomration.toLowerCase();
  const confessionId = infomration.replace(/\D/g, "");
  const data = JSON.parse(fs.readFileSync("confessions.json", "utf-8"));
  const resolved = false // always false till its true then we  save it for maybe two days (add as config at top) then delete incase it gets unapproved and/or threadlocked/threadripped from couminuty logs
  data.push({ id: confessionId, userId, resolved, date: new Date().toISOString() });
  fs.writeFileSync("confessions.json", JSON.stringify(data, null, 2));
}

// Okay so like here we recieve events when a message from Prox2 (different bot) is sent in dms ig???
// google made nned to rerewrite
app.event('message', async ({ event, context, logger }) => {
  try {
    //  ignore messages from the bot, something that would never happen but shrug
    // should probably make it so we only listen to messages by prox2 here
    if ('user' in event && event.user !== context.botUserId) {
      console.log(` New message from user ${event.user} in channel ${event.channel}:`);
      console.log(`Content: "${'text' in event ? event.text : '[No text content]'}"`);
    }
  } catch (error) {
    logger.error('Error handling message event:', error);
  }
});

// while True:
//   if message contains "Staged as confession" && from prox2:
//     save to thingies.txt // find a better name


// func Notify_User(ConfessionStatus)
//   dm(user,ConfessionStatus)

// run slack(SLACK_BOT_TOKEN)
