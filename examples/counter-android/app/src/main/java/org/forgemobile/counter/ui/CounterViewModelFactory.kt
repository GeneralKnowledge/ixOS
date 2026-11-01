package org.forgemobile.counter.ui

import android.content.Context
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import java.io.File
import org.forgemobile.counter.CounterViewModel

class CounterViewModelFactory(private val context: Context) : ViewModelProvider.Factory {
    @Suppress("UNCHECKED_CAST")
    override fun <T : ViewModel> create(modelClass: Class<T>): T {
        if (modelClass.isAssignableFrom(CounterViewModel::class.java)) {
            return CounterViewModel(storageDir(context.applicationContext)) as T
        }
        throw IllegalArgumentException("Unknown ViewModel class")
    }

    private fun storageDir(context: Context): String {
        val base = File(context.filesDir, "counter")
        base.mkdirs()
        return base.absolutePath
    }
}
