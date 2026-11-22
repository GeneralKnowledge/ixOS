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
        // A fresh launch, not a rotation or a restore after the process was kept.
        if (savedInstanceState == null) app.launch(intent)
        setContent { RelayApp(app) }
    }
}
