package org.forgemobile.pocketlibrary

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import org.forgemobile.pocketlibrary.ui.PocketLibraryApp

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val app = application as PocketLibraryApplication
        // A fresh launch, not a rotation or a restore after the process was kept.
        if (savedInstanceState == null) {
            app.launch(PocketLibraryApplication.TestData.from(intent))
        }
        setContent { PocketLibraryApp(app.model) }
    }
}
