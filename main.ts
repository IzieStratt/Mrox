import 'dotenv/config';
import { WebClient } from '@slack/web-api';
const web = new WebClient(process.env.SLACK_BOT_TOKEN);
const channeltouse = process.env.CHANNEL_TO_USE; // get from .env

async function sendMessage(MessageToSend) { // I should find better variable names
  const result = await web.chat.postMessage({
    channel: channeltouse,
    text: MessageToSend
  });

  console.log(`Message sent: ${result.ts}`);
}

sendMessage("Hi").catch(console.error);

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
