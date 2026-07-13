import 'dotenv/config';
import { App } from '@slack/bolt';
import { WebClient } from '@slack/web-api';
import { info } from 'console';
const web = new WebClient(process.env.SLACK_BOT_TOKEN);
const defaultchannel = process.env.CHANNEL_TO_USE; // get from .env

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
async function checkStatus(information: string) {
  const lower = information.toLowerCase();
  switch (true) {
    case lower.includes("meta"):
      await sendMessage(`:perrytheplatpus: Your post ${post.ID} got approved for #meta! Here's [a link|hackclub.enterprise.slack.com/ewfrgibgo//frgthn] to it.`, user.ID)
      break;
    case lower.includes("approved"):
      await sendMessage(`:yay: Your post ${post.ID} got approved for #confessions! Here's [a link|efowediverbfuvnfeorghiubdfojrthuit] to it.`)
      break;
    case lower.includes("denied"):
      await sendMessage(`:neocat_sad: Your post ${post.ID} was denied, link to the deny message`) // TODO: add a thing to check the thread to see if a @crt member added a reason
      break;
  }
}

// Okay so like here we recieve events when a message from Prox2 is sent in dms ig???
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

// if messsage in confession logs contains thing in thingies.txt:
//   case if "Meta"

//   else if "approved"

//   else if "denied"

// func Notify_User(ConfessionStatus)
//   dm(user,ConfessionStatus)

// run slack(SLACK_BOT_TOKEN)
