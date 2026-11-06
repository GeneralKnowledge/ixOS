package org.forgemobile.chat

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat

/**
 * Posts [ChatNotification]s as Android notifications on a "Messages" channel. High importance,
 * so they show as heads-up banners while the app is in the foreground too, like on iOS.
 *
 * Local only: the app must be running, because the Rust client holds the room connections.
 * There is no push server (see docs/chat-demo.md). Without the `POST_NOTIFICATIONS` permission
 * (Android 13+, asked for by [MainActivity]) nothing is posted.
 */
class AndroidChatNotifier(context: Context) : ChatNotifier {
    private val context = context.applicationContext
    private val manager = NotificationManagerCompat.from(this.context)

    init {
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL_ID, "Messages", NotificationManager.IMPORTANCE_HIGH).apply {
                description = "New messages in rooms you are not in"
            },
        )
    }

    override fun post(notification: ChatNotification) {
        if (!canPost(context)) return
        val open =
            PendingIntent.getActivity(
                context,
                0,
                Intent(context, MainActivity::class.java)
                    .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP),
                PendingIntent.FLAG_IMMUTABLE,
            )
        val built =
            NotificationCompat.Builder(context, CHANNEL_ID)
                .setSmallIcon(android.R.drawable.stat_notify_chat)
                .setContentTitle(notification.roomTitle)
                .setContentText("${notification.author}: ${notification.body}")
                .setStyle(NotificationCompat.BigTextStyle().bigText(notification.body))
                .setSubText(notification.author)
                .setCategory(NotificationCompat.CATEGORY_MESSAGE)
                .setPriority(NotificationCompat.PRIORITY_HIGH)
                // Groups a room's notifications together.
                .setGroup(notification.conversationId)
                .setContentIntent(open)
                .setAutoCancel(true)
                .build()
        try {
            manager.notify(notification.messageId, 0, built)
        } catch (_: SecurityException) {
            // The permission was revoked between the check and the post.
        }
    }

    companion object {
        private const val CHANNEL_ID = "messages"

        /** Android 13 added a runtime permission for notifications. */
        fun needsPermission(context: Context): Boolean =
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU && !canPost(context)

        private fun canPost(context: Context): Boolean =
            Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
                ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) ==
                PackageManager.PERMISSION_GRANTED
    }
}
