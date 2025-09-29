import { Injectable, Logger, OnModuleInit } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import * as admin from 'firebase-admin';
import { FcmNotificationData, FcmResponse } from './dto/fcm-notification.dto';

@Injectable()
export class FcmService implements OnModuleInit {
  private readonly logger = new Logger(FcmService.name);

  constructor(private configService: ConfigService) {}

  onModuleInit() {
    this.initializeFirebase();
  }

  private initializeFirebase() {
    const projectId = this.configService.get<string>('firebase.projectId');
    const privateKey = this.configService.get<string>('firebase.privateKey');
    const clientEmail = this.configService.get<string>('firebase.clientEmail');

    if (!projectId || !privateKey || !clientEmail) {
      this.logger.warn('Firebase configuration is incomplete. FCM service will not be available.');
      return;
    }

    try {
      admin.initializeApp({
        credential: admin.credential.cert({
          projectId,
          privateKey,
          clientEmail,
        }),
      });
      this.logger.log('Firebase Admin SDK initialized successfully');
    } catch (error) {
      this.logger.error('Failed to initialize Firebase Admin SDK', error.stack);
    }
  }

  async sendNotification(fcmToken: string, notification: FcmNotificationData): Promise<FcmResponse> {
    if (!admin.apps.length) {
      this.logger.error('Firebase Admin SDK not initialized');
      return { success: false, error: 'Firebase not initialized' };
    }

    try {
      const message = {
        notification: {
          title: notification.title,
          body: notification.body,
        },
        data: notification.data || {},
        token: fcmToken,
        android: {
          notification: {
            channelId: 'gitlab_notifications',
            priority: 'high' as const,
          },
        },
        apns: {
          payload: {
            aps: {
              alert: {
                title: notification.title,
                body: notification.body,
              },
              badge: 1,
              sound: 'default',
            },
          },
        },
      };

      const response = await admin.messaging().send(message);
      this.logger.log(`FCM notification sent successfully: ${response}`);
      
      return { success: true, messageId: response };
    } catch (error) {
      this.logger.error(`Failed to send FCM notification: ${error.message}`, error.stack);
      
      if (error.code === 'messaging/registration-token-not-registered') {
        return { success: false, error: 'Invalid FCM token' };
      }
      
      return { success: false, error: error.message };
    }
  }

  async sendNotificationToMultiple(fcmTokens: string[], notification: FcmNotificationData): Promise<FcmResponse[]> {
    const promises = fcmTokens.map(token => this.sendNotification(token, notification));
    return Promise.all(promises);
  }

  async validateToken(fcmToken: string): Promise<boolean> {
    if (!admin.apps.length) {
      return false;
    }

    try {
      await admin.messaging().send({
        token: fcmToken,
        data: { test: 'true' },
      }, true); // dry run
      return true;
    } catch (error) {
      this.logger.warn(`Invalid FCM token: ${error.message}`);
      return false;
    }
  }
}
