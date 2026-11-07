package FORGE_JAVA_PACKAGE

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import org.forgemobile.ffi.ForgeAndroid
import FORGE_JAVA_PACKAGE.ui.NotesScreen
import FORGE_JAVA_PACKAGE.ui.NotesViewModelFactory

class MainActivity : ComponentActivity() {
    // The same instance NotesScreen gets from `viewModel()`: one Rust core per activity scope.
    private val model: NotesViewModel by viewModels { NotesViewModelFactory(applicationContext) }

    override fun onCreate(savedInstanceState: Bundle?) {
        // Before any Rust object exists: gives the HTTPS certificate verifier the app Context.
        ForgeAndroid.init(applicationContext)
        super.onCreate(savedInstanceState)
        // Tells Rust when the app is on screen, inactive or in the background (AppLifecycle.kt).
        reportLifecycle { model.setPhase(it) }
        enableEdgeToEdge()
        setContent {
            NotesScreen(applicationContext)
        }
    }
}
