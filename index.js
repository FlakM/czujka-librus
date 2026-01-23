import Librus from 'librus-api';
import dotenv from 'dotenv';
import { initDatabase, getExistingAnnouncementIds, getExistingMessageIds, getExistingGradeIds, getExistingEventIds, saveAnnouncement, saveMessage, saveGrade, saveEvent, closeDatabase } from './src/database.js';
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
    const INBOX_FOLDER = 6; // Folder 6 = RECEIVED (inbox)
    const SENT_FOLDER = 5;  // Folder 5 = SENT

    // Get sent message IDs to exclude our own messages
    const sentMessages = await client.inbox.listInbox(SENT_FOLDER);
    const sentIds = new Set(sentMessages.map(m => m.id.toString()));

    const messages = await client.inbox.listInbox(INBOX_FOLDER);
    const existingIds = getExistingMessageIds();

    const newMessages = [];

    for (const message of messages) {
      // Skip messages we sent ourselves
      if (sentIds.has(message.id.toString())) {
        continue;
      }
      if (!existingIds.has(message.id.toString())) {
        try {
          const fullMessage = await client.inbox.getMessage(INBOX_FOLDER, message.id);
          const messageData = {
            id: message.id,
            title: message.title,
            body: fullMessage.content || '',
            date: message.date,
            user: fullMessage.user || message.user
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

async function fetchAndProcessGrades() {
  try {
    const allGrades = await client.info.getGrades();

    if (!allGrades || allGrades.length === 0) {
      logger.info('No grades returned from API');
      return null;
    }

    const existingIds = getExistingGradeIds();
    const newGrades = [];

    for (const subject of allGrades) {
      if (!subject.semester || !Array.isArray(subject.semester)) continue;

      for (const semester of subject.semester) {
        if (!semester.grades || !Array.isArray(semester.grades)) continue;

        for (const grade of semester.grades) {
          const gradeId = grade.id.toString();
          if (!existingIds.has(gradeId)) {
            const gradeData = {
              id: grade.id,
              subject: subject.name,
              value: grade.value,
              info: grade.info
            };
            newGrades.push(gradeData);
            saveGrade(gradeData);
          }
        }
      }
    }

    if (newGrades.length === 0) {
      logger.info('No new grades');
      return null;
    }

    logger.info(`Found ${newGrades.length} new grade(s)`);
    return newGrades;
  } catch (error) {
    logger.error('Error fetching grades', { error: error.message, stack: error.stack });
    return null;
  }
}

async function fetchAndProcessEvents() {
  try {
    const currentMonth = new Date().getMonth() + 1;
    const currentYear = new Date().getFullYear();
    const calendarData = await client.calendar.getCalendar(currentMonth, currentYear);

    if (!calendarData || calendarData.length === 0) {
      logger.info('No calendar events returned from API');
      return null;
    }

    const existingIds = getExistingEventIds();
    const newEvents = [];

    for (const dayEvents of calendarData) {
      if (!dayEvents || !Array.isArray(dayEvents)) continue;

      for (const event of dayEvents) {
        if (!event || event.id === -1) continue;

        const eventId = event.id.toString();
        if (!existingIds.has(eventId)) {
          try {
            const eventDetails = await client.calendar.getEvent(event.id);
            const eventData = {
              id: event.id,
              title: event.title,
              day: event.day,
              description: eventDetails?.description || ''
            };
            newEvents.push(eventData);
            saveEvent(eventData);
          } catch (err) {
            const eventData = {
              id: event.id,
              title: event.title,
              day: event.day,
              description: ''
            };
            newEvents.push(eventData);
            saveEvent(eventData);
            logger.warn(`Failed to fetch event details ${event.id}`, { error: err.message });
          }
        }
      }
    }

    if (newEvents.length === 0) {
      logger.info('No new events');
      return null;
    }

    logger.info(`Found ${newEvents.length} new event(s)`);
    return newEvents;
  } catch (error) {
    logger.error('Error fetching events', { error: error.message, stack: error.stack });
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

    const [newAnnouncements, newMessages, newGrades, newEvents] = await Promise.all([
      fetchAndProcessAnnouncements(),
      fetchAndProcessMessages(),
      fetchAndProcessGrades(),
      fetchAndProcessEvents()
    ]);

    let announcementsAnalysis = null;
    let messagesAnalysis = null;
    let gradesAnalysis = null;
    let eventsAnalysis = null;

    if (newAnnouncements && newAnnouncements.length > 0) {
      logger.info('Analyzing announcements with OpenAI');
      announcementsAnalysis = await summarizeAndClassify(newAnnouncements, 'announcements');
    }

    if (newMessages && newMessages.length > 0) {
      logger.info('Analyzing messages with OpenAI');
      messagesAnalysis = await summarizeAndClassify(newMessages, 'messages');
    }

    if (newGrades && newGrades.length > 0) {
      logger.info('Analyzing grades with OpenAI');
      gradesAnalysis = await summarizeAndClassify(newGrades, 'grades');
    }

    if (newEvents && newEvents.length > 0) {
      logger.info('Analyzing events with OpenAI');
      eventsAnalysis = await summarizeAndClassify(newEvents, 'events');
    }

    if (announcementsAnalysis || messagesAnalysis || gradesAnalysis || eventsAnalysis) {
      await sendNotification(announcementsAnalysis, messagesAnalysis, gradesAnalysis, eventsAnalysis, newAnnouncements, newMessages, newGrades, newEvents);
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
