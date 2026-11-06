package FORGE_JAVA_PACKAGE.ui

import android.content.Context
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import java.io.File
import FORGE_JAVA_PACKAGE.NotesViewModel

class NotesViewModelFactory(private val context: Context) : ViewModelProvider.Factory {
    @Suppress("UNCHECKED_CAST")
    override fun <T : ViewModel> create(modelClass: Class<T>): T {
        if (modelClass.isAssignableFrom(NotesViewModel::class.java)) {
            val dir = File(context.applicationContext.filesDir, "app").apply { mkdirs() }
            return NotesViewModel(dir.absolutePath) as T
        }
        throw IllegalArgumentException("Unknown ViewModel class")
    }
}
