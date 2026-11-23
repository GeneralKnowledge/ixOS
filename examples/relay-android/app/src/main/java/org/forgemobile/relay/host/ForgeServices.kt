// The Android counterpart of ForgeKit's ForgeServices.install() (forge-services' Swift adapters):
// the platform services Rust asks for through `forge_services::native_services!`.
//
// - SecureStore: the Android Keystore. A key that never leaves the Keystore (AES-GCM) seals each
//   value, kept in the app's private preferences, which backups never copy (the manifest turns
//   them off). Relay's Rust core moves this device's key in from `device.key` the first time and
//   keeps the chat server account here, as the iPhone keeps both in the Keychain.
// - LocalNotifier: notifications on a "Messages" channel, with the actions Rust names (Reply as
//   a text field, Mark as Read); a tap or an action comes back to Rust as a NotificationResponse.
// - PermissionProvider: notifications only (POST_NOTIFICATIONS on Android 13 and later), asked
//   through the activity on screen.
//
// It lives in this app until a second Android app needs it, like the rest of host/
// (docs/architecture.md#framework-principles).
package org.forgemobile.relay.host

import android.Manifest
import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.RemoteInput
import androidx.core.content.ContextCompat
import java.security.KeyStore
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicReference
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import kotlin.coroutines.resume
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeoutOrNull
import uniffi.relay_core.AppDirectories
import uniffi.relay_core.LocalNotification
import uniffi.relay_core.LocalNotifier
import uniffi.relay_core.NativePermission
import uniffi.relay_core.NativeServiceException
import uniffi.relay_core.NotificationActionKind
import uniffi.relay_core.NotificationCategory
import uniffi.relay_core.NotificationResponse
import uniffi.relay_core.NotificationTrigger
import uniffi.relay_core.PermissionProvider
import uniffi.relay_core.PermissionStatus
import uniffi.relay_core.SecureStore
import uniffi.relay_core.deliverNotificationResponse
import uniffi.relay_core.installNativeServices
import uniffi.relay_core.nativeServicesInstalled

object ForgeServices {
    /** The action id of a tap on a notification (forge-services' `NOTIFICATION_OPENED`). */
    const val OPENED = "forge.opened"

    lateinit var secureStore: KeystoreSecureStore
        private set
    lateinit var notifier: AndroidNotifier
        private set
    lateinit var permissions: AndroidPermissions
        private set

    /** Installs the services once per process, before any Rust core opens. */
    @Synchronized
    fun install(context: Context) {
        if (nativeServicesInstalled()) return
        val app = context.applicationContext
        secureStore = KeystoreSecureStore(app)
        notifier = AndroidNotifier(app)
        permissions = AndroidPermissions(app)
        installNativeServices(
            AppDirectories(
                documents = app.filesDir.absolutePath,
                applicationSupport = app.filesDir.absolutePath,
                caches = app.cacheDir.absolutePath,
                temporary = app.cacheDir.absolutePath,
            ),
            secureStore,
            notifier,
            permissions,
        )
    }

    /** Whether [install] ran in this process. */
    val isInstalled: Boolean get() = nativeServicesInstalled()
}

class KeystoreSecureStore(context: Context) : SecureStore {
    private val prefs = context.getSharedPreferences("forge-secure-store", Context.MODE_PRIVATE)

    /**
     * Set for a launch on throwaway data (UI tests): secrets then live only in this map, so a
     * test never reads or replaces the person's device key or account.
     */
    @Volatile
    var throwaway: MutableMap<String, ByteArray>? = null

    override fun get(key: String): ByteArray? {
        throwaway?.let { return it[key]?.copyOf() }
        val stored = prefs.getString(PREFIX + key, null) ?: return null
        return try {
            val bytes = Base64.decode(stored, Base64.NO_WRAP)
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes, 0, IV_SIZE))
            cipher.doFinal(bytes, IV_SIZE, bytes.size - IV_SIZE)
        } catch (e: Exception) {
            // Never "missing": a key that can't be read must not be replaced with a new one.
            throw NativeServiceException.Platform(-1, "the secure store couldn't read $key: ${e.javaClass.simpleName}")
        }
    }

    override fun set(key: String, value: ByteArray) {
        throwaway?.let {
            it[key] = value.copyOf()
            return
        }
        val sealed = try {
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, key())
            cipher.iv + cipher.doFinal(value)
        } catch (e: Exception) {
            throw NativeServiceException.Platform(-1, "the secure store couldn't seal $key: ${e.javaClass.simpleName}")
        }
        if (!prefs.edit().putString(PREFIX + key, Base64.encodeToString(sealed, Base64.NO_WRAP)).commit()) {
            throw NativeServiceException.Platform(-1, "the secure store couldn't save $key")
        }
    }

    override fun delete(key: String) {
        throwaway?.let {
            it.remove(key)
            return
        }
        if (!prefs.edit().remove(PREFIX + key).commit()) {
            throw NativeServiceException.Platform(-1, "the secure store couldn't remove $key")
        }
    }

    @Synchronized
    private fun key(): SecretKey {
        val store = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    private companion object {
        const val KEYSTORE = "AndroidKeyStore"
        const val ALIAS = "forge.secure-store"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_SIZE = 12
        const val PREFIX = "v1:"
    }
}

class AndroidNotifier(private val context: Context) : LocalNotifier {
    private val categories = AtomicReference<Map<String, NotificationCategory>>(emptyMap())
    private val scheduled = context.getSharedPreferences("forge-notifications", Context.MODE_PRIVATE)

    init {
        val channel = NotificationChannel(CHANNEL, "Messages", NotificationManager.IMPORTANCE_DEFAULT).apply {
            description = "New messages in your conversations"
        }
        context.getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    override suspend fun schedule(notification: LocalNotification) {
        when (val trigger = notification.trigger) {
            is NotificationTrigger.Now -> post(context, notification.toIntent(context))
            is NotificationTrigger.After -> later(notification, SystemClock.elapsedRealtime() + trigger.seconds.toLong() * 1000, elapsed = true)
            is NotificationTrigger.At -> later(notification, trigger.unixMs, elapsed = false)
        }
    }

    private fun later(notification: LocalNotification, at: Long, elapsed: Boolean) {
        val alarms = context.getSystemService(AlarmManager::class.java)
        // Inexact: no exact-alarm permission, and a reminder a few minutes late is fine.
        alarms.set(if (elapsed) AlarmManager.ELAPSED_REALTIME_WAKEUP else AlarmManager.RTC_WAKEUP, at, alarm(notification.id, notification.toIntent(context)))
        scheduled.edit().putBoolean(notification.id, true).apply()
    }

    override fun cancel(id: String) {
        context.getSystemService(AlarmManager::class.java).cancel(alarm(id, Intent(context, NotificationReceiver::class.java)))
        scheduled.edit().remove(id).apply()
        NotificationManagerCompat.from(context).cancel(id, 0)
    }

    override suspend fun pending(): List<String> = scheduled.all.keys.filter { it != OPENS_APP }

    override fun setCategories(categories: List<NotificationCategory>) {
        this.categories.set(categories.associateBy { it.id })
        // Kept across launches: a tap can start the process before Rust registers them again.
        val opening = categories.flatMap { c ->
            c.actions.filter { it.kind is NotificationActionKind.OpensApp }.map { "${c.id}/${it.id}" }
        }.toSet()
        scheduled.edit().putStringSet(OPENS_APP, opening).apply()
    }

    /**
     * Whether [response] is one that opens the app: a tap, or an action registered as opening
     * it. Only those may come through the activity, which any app can start; Reply and Mark as
     * Read come only through [NotificationReceiver], which no other app can reach.
     */
    fun opensApp(response: NotificationResponse): Boolean {
        if (response.actionId == ForgeServices.OPENED) return true
        val category = response.category ?: return false
        val registered = categories.get()[category]?.actions?.firstOrNull { it.id == response.actionId }
        if (registered != null) return registered.kind is NotificationActionKind.OpensApp
        return scheduled.getStringSet(OPENS_APP, emptySet())!!.contains("$category/${response.actionId}")
    }

    private fun alarm(id: String, intent: Intent): PendingIntent =
        PendingIntent.getBroadcast(
            context,
            id.hashCode(),
            intent.setClass(context, NotificationReceiver::class.java).setAction(ACTION_POST).setData(Uri.parse("forge-notification:${Uri.encode(id)}")),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

    /** Posts the notification [intent] carries. Also called when an alarm fires. */
    internal fun post(context: Context, intent: Intent) {
        val id = intent.getStringExtra(EXTRA_ID) ?: return
        scheduled.edit().remove(id).apply()
        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        val title = intent.getStringExtra(EXTRA_TITLE).orEmpty()
        val body = intent.getStringExtra(EXTRA_BODY).orEmpty()
        val category = intent.getStringExtra(EXTRA_CATEGORY)
        val thread = intent.getStringExtra(EXTRA_THREAD)
        val builder = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(android.R.drawable.stat_notify_chat)
            .setContentTitle(title)
            .setContentText(body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(body))
            .setAutoCancel(true)
            .setContentIntent(opened(context, intent))
        intent.getStringExtra(EXTRA_SUBTITLE)?.let { builder.setSubText(it) }
        thread?.let { builder.setGroup(it) }
        category?.let { categories.get()[it] }?.actions?.forEachIndexed { index, action ->
            val response = responseIntent(context, intent, action.id)
            when (val kind = action.kind) {
                is NotificationActionKind.TextInput -> {
                    val input = RemoteInput.Builder(EXTRA_REPLY).setLabel(kind.placeholder).build()
                    val pending = PendingIntent.getBroadcast(
                        context, (id + action.id).hashCode() + index, response,
                        // A reply needs a mutable intent: the system adds the text to it.
                        PendingIntent.FLAG_MUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                    )
                    builder.addAction(
                        NotificationCompat.Action.Builder(0, action.title, pending).addRemoteInput(input).setAllowGeneratedReplies(false).build(),
                    )
                }
                is NotificationActionKind.Button -> builder.addAction(
                    0, action.title,
                    PendingIntent.getBroadcast(context, (id + action.id).hashCode() + index, response, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT),
                )
                is NotificationActionKind.OpensApp -> builder.addAction(0, action.title, opened(context, intent, action.id))
            }
        }
        NotificationManagerCompat.from(context).notify(id, 0, builder.build())
    }

    private fun responseIntent(context: Context, posted: Intent, action: String): Intent =
        Intent(posted).setClass(context, NotificationReceiver::class.java).setAction(ACTION_RESPOND)
            .setData(Uri.parse("forge-notification:${Uri.encode(posted.getStringExtra(EXTRA_ID))}/${Uri.encode(action)}"))
            .putExtra(EXTRA_ACTION, action)

    private fun opened(context: Context, posted: Intent, action: String = ForgeServices.OPENED): PendingIntent {
        val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)!!
            .setAction(Intent.ACTION_VIEW)
            .setData(posted.getStringExtra(EXTRA_LINK)?.let(Uri::parse))
            .putExtras(posted)
            .putExtra(EXTRA_ACTION, action)
            .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        return PendingIntent.getActivity(
            context, (posted.getStringExtra(EXTRA_ID) + action).hashCode(), launch,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }

    companion object {
        const val CHANNEL = "messages"
        private const val OPENS_APP = "forge.opens-app-actions"

        /**
         * Posts the notification [id] again as it is, saying the answer didn't go: a reply's text
         * field spins until its notification is updated, and this makes it answerable again.
         * Rebuilt from the one showing, since the actions it was posted with may not be registered
         * in this process. Nothing if it is no longer showing.
         */
        fun notAnswered(context: Context, id: String) {
            val manager = context.getSystemService(NotificationManager::class.java)
            val showing = manager.activeNotifications.firstOrNull { it.tag == id && it.id == 0 } ?: return
            val again = Notification.Builder.recoverBuilder(context, showing.notification)
                .setSubText("Not sent. Open Relay, or try again.")
                .setOnlyAlertOnce(true)
                .build()
            // Showing means it was allowed; a permission taken back since leaves it as it is.
            try {
                manager.notify(id, 0, again)
            } catch (_: SecurityException) {
            }
        }

        const val ACTION_POST = "org.forgemobile.forge.POST_NOTIFICATION"
        const val ACTION_RESPOND = "org.forgemobile.forge.NOTIFICATION_RESPONSE"
        const val EXTRA_ID = "forge.notification.id"
        const val EXTRA_TITLE = "forge.notification.title"
        const val EXTRA_BODY = "forge.notification.body"
        const val EXTRA_SUBTITLE = "forge.notification.subtitle"
        const val EXTRA_THREAD = "forge.notification.thread"
        const val EXTRA_LINK = "forge.notification.link"
        const val EXTRA_CATEGORY = "forge.notification.category"
        const val EXTRA_ACTION = "forge.notification.action"
        const val EXTRA_REPLY = "forge.notification.reply"

        private fun LocalNotification.toIntent(context: Context): Intent =
            Intent(context, NotificationReceiver::class.java)
                .putExtra(EXTRA_ID, id)
                .putExtra(EXTRA_TITLE, title)
                .putExtra(EXTRA_BODY, body)
                .putExtra(EXTRA_SUBTITLE, subtitle)
                .putExtra(EXTRA_THREAD, threadId)
                .putExtra(EXTRA_LINK, deepLink)
                .putExtra(EXTRA_CATEGORY, category)

        /**
         * What the person did with a notification, from the intent that opened the app or
         * reached [NotificationReceiver]; null for any other intent.
         */
        fun response(intent: Intent): NotificationResponse? {
            val id = intent.getStringExtra(EXTRA_ID) ?: return null
            val action = intent.getStringExtra(EXTRA_ACTION) ?: return null
            val text = RemoteInput.getResultsFromIntent(intent)?.getCharSequence(EXTRA_REPLY)?.toString()
            return NotificationResponse(
                notificationId = id,
                actionId = action,
                text = text,
                category = intent.getStringExtra(EXTRA_CATEGORY),
                threadId = intent.getStringExtra(EXTRA_THREAD),
                deepLink = intent.getStringExtra(EXTRA_LINK),
            )
        }
    }
}

/**
 * Posts a notification whose time has come, and hands Rust the person's answer to one (Reply,
 * Mark as Read). The app opens Relay in the background first, if it wasn't running, through
 * [onResponse], so Rust has a handler for the answer (it keeps one until a handler is set).
 */
class NotificationReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            AndroidNotifier.ACTION_POST -> if (ForgeServices.isInstalled) ForgeServices.notifier.post(context, intent)
            AndroidNotifier.ACTION_RESPOND -> {
                val response = AndroidNotifier.response(intent) ?: return
                // Kept alive until Rust has the answer: a reply that started Relay waits for its
                // core to open, since Android may end a receiver's process once this returns.
                // Rust's handler then hands the send to finishing work, which holds background
                // time of its own.
                val pending = goAsync()
                responses.launch {
                    try {
                        val open = onResponse
                        val ready = open == null || withTimeoutOrNull(RECEIVER_TIME_MS) { open() } == true
                        // Handed over only when something takes it: kept back otherwise, so the
                        // notification stays to be answered again and the answer can't go twice
                        // (once now, once later when a handler appears).
                        if (ready) {
                            deliverNotificationResponse(response)
                            // Answered: the notification goes (a reply's text field would spin
                            // otherwise).
                            NotificationManagerCompat.from(context).cancel(response.notificationId, 0)
                        } else {
                            AndroidNotifier.notAnswered(context, response.notificationId)
                        }
                    } finally {
                        pending.finish()
                    }
                }
            }
        }
    }

    companion object {
        /**
         * Set by the app: opens its Rust cores if they aren't, returning true once a handler is
         * set to take the response (false if none could be, and the notification stays).
         */
        @Volatile
        var onResponse: (suspend () -> Boolean)? = null

        private val responses = CoroutineScope(SupervisorJob() + Dispatchers.Default)

        /** Under the ten seconds a receiver gets. */
        private const val RECEIVER_TIME_MS = 8_000L
    }
}

class AndroidPermissions(private val context: Context) : PermissionProvider {
    private val main = Handler(Looper.getMainLooper())
    private val asked = context.getSharedPreferences("forge-permissions", Context.MODE_PRIVATE)

    /** How the activity on screen asks for a runtime permission; null with none on screen. */
    private val asker = MutableStateFlow<((String, (Boolean) -> Unit) -> Unit)?>(null)
    private val waiting = ConcurrentHashMap<String, Boolean>()

    /** The activity on screen asks with [ask]; call again with null when it goes. */
    fun attach(ask: ((String, (Boolean) -> Unit) -> Unit)?) {
        asker.value = ask
    }

    override suspend fun status(permission: NativePermission): PermissionStatus = when (permission) {
        NativePermission.NOTIFICATIONS -> when {
            NotificationManagerCompat.from(context).areNotificationsEnabled() -> PermissionStatus.GRANTED
            Build.VERSION.SDK_INT >= 33 && !asked.getBoolean(Manifest.permission.POST_NOTIFICATIONS, false) ->
                PermissionStatus.NOT_DETERMINED
            else -> PermissionStatus.DENIED
        }
        else -> PermissionStatus.RESTRICTED
    }

    override suspend fun request(permission: NativePermission): PermissionStatus {
        if (permission != NativePermission.NOTIFICATIONS || Build.VERSION.SDK_INT < 33) return status(permission)
        if (status(permission) != PermissionStatus.NOT_DETERMINED) return status(permission)
        val name = Manifest.permission.POST_NOTIFICATIONS
        // One question at a time; a second caller waits for the answer to the first.
        if (waiting.putIfAbsent(name, true) != null) return status(permission)
        try {
            val ask = asker.filterNotNull().first()
            suspendCancellableCoroutine { done ->
                main.post { ask(name) { if (done.isActive) done.resume(it) } }
            }
            asked.edit().putBoolean(name, true).apply()
        } finally {
            waiting.remove(name)
        }
        return status(permission)
    }
}
