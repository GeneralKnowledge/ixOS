package org.forgemobile.counter

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import org.forgemobile.ffi.ForgeAndroid
import org.forgemobile.counter.ui.CounterScreen

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        // Before any Rust object exists: gives the HTTPS certificate verifier the app Context.
        ForgeAndroid.init(applicationContext)
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            CounterScreen(applicationContext)
        }
    }
}
