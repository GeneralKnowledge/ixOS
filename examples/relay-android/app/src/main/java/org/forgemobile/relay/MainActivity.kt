package org.forgemobile.relay

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import org.forgemobile.relay.ui.RelayApp

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val app = application as RelayApplication
        // A fresh launch, or a restore after Android ended the process (nothing open yet); not
        // a rotation, where the process kept Relay open.
        if (savedInstanceState == null || !app.launched) app.launch(intent)
        setContent { RelayApp(app) }
    }
}
