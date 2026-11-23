package org.forgemobile.relay

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import org.forgemobile.relay.host.ForgeServices
import org.forgemobile.relay.ui.RelayApp

class MainActivity : ComponentActivity() {
    private val permission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        answer?.invoke(granted)
        answer = null
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val app = application as RelayApplication
        // A fresh launch, or a restore after Android ended the process (nothing open yet); not
        // a rotation, where the process kept Relay open. A restore doesn't open the
        // notification that launched the activity again: a new tap comes to onNewIntent.
        if (savedInstanceState == null) {
            app.launch(intent)
        } else if (!app.launched) {
            app.launch(intent, restored = true)
        }
        if (ForgeServices.isInstalled) {
            ForgeServices.permissions.attach { name, done ->
                answer = done
                permission.launch(name)
            }
        }
        setContent { RelayApp(app) }
    }

    /** A tap on a notification while Relay is open. */
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        (application as RelayApplication).open(intent)
    }

    override fun onDestroy() {
        if (ForgeServices.isInstalled) ForgeServices.permissions.attach(null)
        super.onDestroy()
    }

    private companion object {
        /**
         * The answer to a runtime permission Rust asked for, while one is asked. Kept for the
         * process: after a rotation the answer comes to the new activity.
         */
        var answer: ((Boolean) -> Unit)? = null
    }
}
