import Librus from 'librus-api';
import dotenv from 'dotenv';
import { initDatabase, getExistingAnnouncementIds, getExistingMessageIds, getExistingGradeIds, getExistingEventIds, getExistingHomeworkIds, saveAnnouncement, saveMessage, saveGrade, saveEvent, saveHomework, closeDatabase } from './src/database.js';
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

async function fetchAndProcessHomework() {
  try {
    const subjects = await client.homework.listSubjects();
    if (!subjects || subjects.length === 0) {
      logger.info('No subjects found for homework');
      return null;
    }

    const existingIds = getExistingHomeworkIds();
    const newHomework = [];

    const now = new Date();
    const from = new Date(now.getFullYear(), now.getMonth(), 1).toISOString().split('T')[0];
    const to = new Date(now.getFullYear(), now.getMonth() + 2, 0).toISOString().split('T')[0];

    for (const subject of subjects) {
      if (!subject.id || subject.id === 0) continue;

      try {
        const homeworkList = await client.homework.listHomework(subject.id, from, to);
        if (!homeworkList || homeworkList.length === 0) continue;

        for (const hw of homeworkList) {
          if (!existingIds.has(hw.id.toString())) {
            try {
              const details = await client.homework.getHomework(hw.id);
              const homeworkData = {
                id: hw.id,
                subject: hw.subject || subject.name,
                title: hw.title,
                type: hw.type,
                from: hw.from,
                to: hw.to,
                content: details?.content || '',
                user: hw.user || details?.user || ''
              };
              newHomework.push(homeworkData);
              saveHomework(homeworkData);
            } catch (err) {
              const homeworkData = {
                id: hw.id,
                subject: hw.subject || subject.name,
                title: hw.title,
                type: hw.type,
                from: hw.from,
                to: hw.to,
                content: '',
                user: hw.user || ''
              };
              newHomework.push(homeworkData);
              saveHomework(homeworkData);
              logger.warn(`Failed to fetch homework details ${hw.id}`, { error: err.message });
            }
          }
        }
      } catch (err) {
        logger.warn(`Failed to fetch homework for subject ${subject.name}`, { error: err.message });
      }
    }

    if (newHomework.length === 0) {
      logger.info('No new homework');
      return null;
    }

    logger.info(`Found ${newHomework.length} new homework assignment(s)`);
    return newHomework;
  } catch (error) {
    logger.error('Error fetching homework', { error: error.message, stack: error.stack });
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

    const [newAnnouncements, newMessages, newGrades, newEvents, newHomework] = await Promise.all([
      fetchAndProcessAnnouncements(),
      fetchAndProcessMessages(),
      fetchAndProcessGrades(),
      fetchAndProcessEvents(),
      fetchAndProcessHomework()
    ]);

    let announcementsAnalysis = null;
    let messagesAnalysis = null;
    let gradesAnalysis = null;
    let eventsAnalysis = null;
    let homeworkAnalysis = null;

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

    if (newHomework && newHomework.length > 0) {
      logger.info('Analyzing homework with OpenAI');
      homeworkAnalysis = await summarizeAndClassify(newHomework, 'homework');
    }

    if (announcementsAnalysis || messagesAnalysis || gradesAnalysis || eventsAnalysis || homeworkAnalysis) {
      await sendNotification(announcementsAnalysis, messagesAnalysis, gradesAnalysis, eventsAnalysis, homeworkAnalysis, newAnnouncements, newMessages, newGrades, newEvents, newHomework);
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
