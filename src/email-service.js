import nodemailer from 'nodemailer';
import { logger } from './logger.js';

let transporter;

function shouldSendEmail() {
  return process.env.SEND_EMAIL === 'true';
}

function getTransporter() {
  if (!transporter && shouldSendEmail()) {
    transporter = nodemailer.createTransport({
      host: process.env.EMAIL_HOST,
      port: parseInt(process.env.EMAIL_PORT),
      secure: false,
      auth: {
        user: process.env.EMAIL_USER,
        pass: process.env.EMAIL_PASSWORD
      }
    });
  }
  return transporter;
}

function generateUrgencyBadge(urgency) {
  const colors = {
    'PILNE': '#dc3545',
    'NORMALNE': '#ffc107',
    'NIEPILNE': '#28a745'
  };

  return `<span style="background-color: ${colors[urgency] || colors.NORMALNE}; color: white; padding: 4px 8px; border-radius: 4px; font-size: 12px; font-weight: bold;">${urgency}</span>`;
}

function formatMarkdown(text) {
  if (!text) return text;
  return text.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
}

function generateHtmlEmail(announcementsData, messagesData, announcements, messages) {
  let html = `
<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <style>
    body { font-family: Arial, sans-serif; line-height: 1.6; color: #333; max-width: 800px; margin: 0 auto; padding: 20px; }
    h1 { color: #2c3e50; border-bottom: 3px solid #3498db; padding-bottom: 10px; }
    h2 { color: #34495e; margin-top: 30px; }
    h3 { color: #495057; font-size: 16px; margin-top: 20px; }
    .summary { background-color: #f8f9fa; padding: 15px; border-left: 4px solid #3498db; margin: 20px 0; }
    .key-points { background-color: #fff; padding: 15px; border: 1px solid #dee2e6; border-radius: 5px; }
    ul { margin: 10px 0; }
    li { margin: 8px 0; }
    .footer { margin-top: 40px; padding-top: 20px; border-top: 1px solid #dee2e6; color: #6c757d; font-size: 12px; }
    .section { margin-bottom: 40px; }
    .item { margin: 15px 0; padding: 15px; background: #f8f9fa; border-radius: 5px; border-left: 3px solid #6c757d; }
    .item-header { font-weight: bold; color: #2c3e50; margin-bottom: 5px; }
    .item-meta { font-size: 13px; color: #6c757d; margin-bottom: 10px; }
    details { margin-top: 10px; }
    summary { cursor: pointer; color: #3498db; font-weight: 500; }
    summary:hover { color: #2980b9; }
    .item-content { margin-top: 10px; padding: 10px; background: white; border-radius: 3px; white-space: pre-wrap; }
  </style>
</head>
<body>
  <h1>📚 Nowe powiadomienia z Librusa</h1>
`;

  if (announcementsData) {
    html += `
  <div class="section">
    <h2>📢 Ogłoszenia ${generateUrgencyBadge(announcementsData.urgency)}</h2>
    <div class="summary">
      <strong>Podsumowanie:</strong><br>
      ${formatMarkdown(announcementsData.summary)}
    </div>
    <div class="key-points">
      <strong>Kluczowe punkty:</strong>
      <ul>
        ${announcementsData.keyPoints.map(point => `<li>${formatMarkdown(point)}</li>`).join('')}
      </ul>
    </div>
    <h3>Szczegóły ogłoszeń:</h3>
`;

    if (announcements && announcements.length > 0) {
      announcements.forEach((item, idx) => {
        html += `
    <div class="item">
      <div class="item-header">${idx + 1}. ${item.title || 'Brak tytułu'}</div>
      <div class="item-meta">
        Autor: ${item.user || item.author || 'Nieznany'} | Data: ${item.date || 'Brak daty'}
      </div>
      <details>
        <summary>Pokaż treść</summary>
        <div class="item-content">${item.content || 'Brak treści'}</div>
      </details>
    </div>
`;
      });
    }

    html += `
  </div>
`;
  }

  if (messagesData) {
    html += `
  <div class="section">
    <h2>✉️ Wiadomości ${generateUrgencyBadge(messagesData.urgency)}</h2>
    <div class="summary">
      <strong>Podsumowanie:</strong><br>
      ${formatMarkdown(messagesData.summary)}
    </div>
    <div class="key-points">
      <strong>Kluczowe punkty:</strong>
      <ul>
        ${messagesData.keyPoints.map(point => `<li>${formatMarkdown(point)}</li>`).join('')}
      </ul>
    </div>
    <h3>Szczegóły wiadomości:</h3>
`;

    if (messages && messages.length > 0) {
      messages.forEach((item, idx) => {
        const messageUrl = item.id ? `https://synergia.librus.pl/wiadomosci/1/5/${item.id}/f0` : null;
        html += `
    <div class="item">
      <div class="item-header">
        ${idx + 1}. ${item.title || 'Brak tytułu'}
        ${messageUrl ? ` <a href="${messageUrl}" style="color: #3498db; font-size: 13px;">[Otwórz w Librus]</a>` : ''}
      </div>
      <div class="item-meta">
        Od: ${item.user || 'Nieznany'} | Data: ${item.date || 'Brak daty'}
      </div>
      <details>
        <summary>Pokaż treść</summary>
        <div class="item-content">${item.body || 'Brak treści'}</div>
      </details>
    </div>
`;
      });
    }

    html += `
  </div>
`;
  }

  html += `
  <div class="footer">
    <p>To powiadomienie zostało wygenerowane automatycznie przez system monitorowania Librusa.</p>
    <p>Data wygenerowania: ${new Date().toLocaleString('pl-PL')}</p>
  </div>
</body>
</html>
`;

  return html;
}

function getSubjectPrefix(urgency) {
  switch (urgency) {
    case 'PILNE': return '[PILNE]';
    case 'NIEPILNE': return '[NIEPILNE]';
    default: return '[NORMALNE]';
  }
}

export async function sendNotification(announcementsData, messagesData, announcements, messages) {
  const hasAnnouncements = announcementsData && announcementsData.keyPoints.length > 0;
  const hasMessages = messagesData && messagesData.keyPoints.length > 0;

  if (!hasAnnouncements && !hasMessages) {
    logger.info('No new items to notify about');
    return;
  }

  const maxUrgency = !hasAnnouncements ? messagesData.urgency :
                     !hasMessages ? announcementsData.urgency :
                     (announcementsData.urgency === 'PILNE' || messagesData.urgency === 'PILNE') ? 'PILNE' :
                     (announcementsData.urgency === 'NORMALNE' || messagesData.urgency === 'NORMALNE') ? 'NORMALNE' :
                     'NIEPILNE';

  const subject = `${getSubjectPrefix(maxUrgency)} Nowe powiadomienia z Librusa`;
  const html = generateHtmlEmail(announcementsData, messagesData, announcements, messages);

  if (!shouldSendEmail()) {
    logger.info('EMAIL SENDING DISABLED - Notification content:');
    console.log('\n' + '='.repeat(80));
    console.log(`Subject: ${subject}`);
    console.log('='.repeat(80));
    console.log('\nHTML Preview (simplified):');
    console.log(html.replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ').trim().substring(0, 500) + '...');
    console.log('\n' + '='.repeat(80) + '\n');
    return;
  }

  try {
    const emailTo = process.env.EMAIL_TO.split(',').map(email => email.trim()).join(',');
    const recipientCount = emailTo.split(',').length;

    logger.info('Sending email notification', {
      urgency: maxUrgency,
      recipients: emailTo,
      recipientCount
    });

    await getTransporter().sendMail({
      from: process.env.EMAIL_FROM || process.env.EMAIL_USER,
      to: emailTo,
      subject,
      html
    });

    logger.info('Email notification sent successfully', {
      urgency: maxUrgency,
      recipients: emailTo,
      recipientCount
    });
  } catch (error) {
    logger.error('Failed to send email notification', { error: error.message });
    throw error;
  }
}
