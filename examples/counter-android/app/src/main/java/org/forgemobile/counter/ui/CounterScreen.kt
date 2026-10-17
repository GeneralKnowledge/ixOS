package org.forgemobile.counter.ui

import android.content.Context
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewmodel.compose.viewModel
import org.forgemobile.counter.CounterViewModel

@Composable
fun CounterScreen(context: Context) {
    val model: CounterViewModel = viewModel(
        factory = CounterViewModelFactory(context),
    )
    MaterialTheme {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            if (model.isBlocked) {
                BlockedView(model)
            } else {
                CounterView(model)
            }
        }
    }
}

@Composable
private fun CounterView(model: CounterViewModel) {
    Text(
        text = model.count.toString(),
        fontSize = 72.sp,
        style = MaterialTheme.typography.displayLarge,
    )
    Spacer(modifier = Modifier.height(24.dp))
    Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
        Button(onClick = { model.decrement() }) { Text("Decrement") }
        Button(onClick = { model.increment() }) { Text("Increment") }
    }
    Spacer(modifier = Modifier.height(16.dp))
    Button(
        onClick = { model.reset() },
        colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.error),
    ) {
        Text("Reset")
    }
    if (model.startupLabel.isNotEmpty()) {
        Spacer(modifier = Modifier.height(16.dp))
        Text(
            text = model.startupLabel,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
    model.errorMessage?.let { message ->
        Spacer(modifier = Modifier.height(8.dp))
        Text(
            text = message,
            color = MaterialTheme.colorScheme.error,
            style = MaterialTheme.typography.bodySmall,
            textAlign = TextAlign.Center,
        )
    }
    Spacer(modifier = Modifier.height(16.dp))
    Text(
        text = model.greeting,
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.outline,
    )
}

@Composable
private fun BlockedView(model: CounterViewModel) {
    Text("Counter unavailable", style = MaterialTheme.typography.headlineSmall)
    model.errorMessage?.let { message ->
        Spacer(modifier = Modifier.height(8.dp))
        Text(
            text = message,
            style = MaterialTheme.typography.bodySmall,
            textAlign = TextAlign.Center,
        )
    }
    Spacer(modifier = Modifier.height(16.dp))
    Button(onClick = { model.retryOpen() }) { Text("Retry") }
    if (model.canRecoverFromCorruption) {
        Spacer(modifier = Modifier.height(8.dp))
        Button(
            onClick = { model.recoverFromCorruption() },
            colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.error),
        ) {
            Text("Reset storage and start fresh")
        }
    }
}
