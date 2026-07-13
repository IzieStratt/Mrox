import 'dotenv/config';
import { App } from '@slack/bolt';
import { WebClient } from '@slack/web-api';
import { info } from 'console';
const web = new WebClient(process.env.SLACK_BOT_TOKEN);
const channeltouse = process.env.CHANNEL_TO_USE; // get from .env

const app = new App({
  token: process.env.SLACK_BOT_TOKEN,
  appToken: process.env.SLACK_APP_TOKEN,
  socketMode: true, // becuase yes
});

async function sendMessage(MessageToSend: any) { // I should find better variable names
  const result = await web.chat.postMessage({
    channel: channeltouse,
    text: MessageToSend
  });

  console.log(`Message sent: ${result.ts}`);
}

sendMessage("Hi").catch(console.error);
// aaaaaaaaa
async function checkStatus(information: String) {
  case information.contains("meta") {

  }
  switch (information.contains("approved"))
  switch (infomration.contains("denied"))
}

// Okay so like here we recieve events when a message from Prox2 is sent in dms ig???
app.event('message', async ({ event, context, logger }) => {
  try {
    //  ignore
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
