package org.forgemobile.chat

import android.Manifest
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import org.forgemobile.chat.ui.ChatRoot

class MainActivity : ComponentActivity() {
    private val model: ChatViewModel by viewModels()

    // If the answer is no, the app works the same, just without notifications.
    private val askForNotifications =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent { ChatRoot(model) }
        if (savedInstanceState == null &&
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            AndroidChatNotifier.needsPermission(this)
        ) {
            askForNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }
}
