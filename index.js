import Librus from 'librus-api';
import dotenv from 'dotenv';
import { initDatabase, getExistingAnnouncementIds, getExistingMessageIds, saveAnnouncement, saveMessage, closeDatabase } from './src/database.js';
import { logger } from './src/logger.js';
import { summarizeAndClassify } from './src/openai-service.js';
import { sendNotification } from './src/email-service.js';

dotenv.config();

const username = process.env.LIBRUS_USERNAME;
const password = process.env.LIBRUS_PASSWORD;

if (!username || !password) {
  logger.error('LIBRUS_USERNAME and LIBRUS_PASSWORD must be set in .env file');
  process.exit(1);
}

if (!process.env.OPENAI_API_KEY) {
  logger.error('OPENAI_API_KEY must be set in .env file');
  process.exit(1);
}

const client = new Librus();

async function fetchAndProcessAnnouncements() {
  try {
    const announcements = await client.inbox.listAnnouncements();

    if (!announcements || announcements.length === 0) {
      logger.info('No announcements returned from API');
      return null;
    }

    const existingIds = getExistingAnnouncementIds();

    const newAnnouncements = announcements.filter(a => {
      if (!a || !a.title || !a.date) {
        logger.warn('Announcement missing required fields, skipping', { announcement: a });
        return false;
      }
      const compositeId = `${a.title}_${a.date}`;
      return !existingIds.has(compositeId);
    });

    if (newAnnouncements.length === 0) {
      logger.info('No new announcements');
      return null;
    }

    logger.info(`Found ${newAnnouncements.length} new announcement(s)`);

    for (const announcement of newAnnouncements) {
      const compositeId = `${announcement.title}_${announcement.date}`;
      saveAnnouncement({ ...announcement, id: compositeId });
    }

    return newAnnouncements;
  } catch (error) {
    logger.error('Error fetching announcements', { error: error.message, stack: error.stack });
    return null;
  }
}

async function fetchAndProcessMessages() {
  try {
    const messages = await client.inbox.listInbox(5);
    const existingIds = getExistingMessageIds();

    const newMessages = [];

    for (const message of messages) {
      if (!existingIds.has(message.id.toString())) {
        try {
          const fullMessage = await client.inbox.getMessage(5, message.id);
          const messageData = {
            id: message.id,
            title: message.title,
            body: fullMessage.body || fullMessage.content || '',
            date: message.date,
            user: message.user
          };
          newMessages.push(messageData);
          saveMessage(messageData);
        } catch (err) {
          logger.warn(`Failed to fetch full message ${message.id}`, { error: err.message });
        }
      }
    }

    if (newMessages.length === 0) {
      logger.info('No new messages');
      return null;
    }

    logger.info(`Found ${newMessages.length} new message(s)`);
    return newMessages;
  } catch (error) {
    logger.error('Error fetching messages', { error: error.message });
    return null;
  }
}

async function main() {
  try {
    logger.info('Starting Librus notification service');

    if (process.env.SEND_EMAIL === 'true' && process.env.EMAIL_TO) {
      const emailRecipients = process.env.EMAIL_TO.split(',').map(email => email.trim());
      logger.info('Email notifications enabled', {
        recipients: emailRecipients,
        recipientCount: emailRecipients.length
      });
    } else {
      logger.info('Email notifications disabled');
    }

    initDatabase();

    logger.info('Authenticating with Librus');
    await client.authorize(username, password);
    logger.info('Authentication successful');

    const [newAnnouncements, newMessages] = await Promise.all([
      fetchAndProcessAnnouncements(),
      fetchAndProcessMessages()
    ]);

    let announcementsAnalysis = null;
    let messagesAnalysis = null;

    if (newAnnouncements && newAnnouncements.length > 0) {
      logger.info('Analyzing announcements with OpenAI');
      announcementsAnalysis = await summarizeAndClassify(newAnnouncements, 'announcements');
    }

    if (newMessages && newMessages.length > 0) {
      logger.info('Analyzing messages with OpenAI');
      messagesAnalysis = await summarizeAndClassify(newMessages, 'messages');
    }

    if (announcementsAnalysis || messagesAnalysis) {
      await sendNotification(announcementsAnalysis, messagesAnalysis, newAnnouncements, newMessages);
    } else {
      logger.info('No new items to process');
    }

    closeDatabase();
    logger.info('Service completed successfully');
  } catch (error) {
    logger.error('Service error', { error: error.message, stack: error.stack });
    closeDatabase();
    process.exit(1);
  }
}

main();
