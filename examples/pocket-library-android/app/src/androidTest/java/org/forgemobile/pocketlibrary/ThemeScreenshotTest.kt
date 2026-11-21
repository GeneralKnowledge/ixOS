package org.forgemobile.pocketlibrary

import android.graphics.Bitmap
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.Density
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.platform.io.PlatformTestStorageRegistry
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.forgemobile.pocketlibrary.kit.ForgeLoad
import org.forgemobile.pocketlibrary.ui.PocketLibraryApp
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The library in the Forge look, in light, dark and at 200% text, saved as PNGs for a person to
 * look at (CI attaches them to the run). They check that nothing crashes or overflows at a large
 * font scale; how it looks is a person's call.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(AndroidJUnit4::class)
class ThemeScreenshotTest {
    @get:Rule
    val compose = createComposeRule()

    private lateinit var model: LibraryModel

    @Before
    fun openWithBooks() = runBlocking {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val app = context.applicationContext as PocketLibraryApplication
        InstrumentationRegistry.getInstrumentation().runOnMainSync { app.launch(PocketLibraryApplication.TestData(temporary = true)) }
        model = app.model
        withTimeout(20_000) { model.opening.first { it is ForgeLoad.Loaded } }
        for ((title, author) in listOf(
            "The Dispossessed" to "Ursula K. Le Guin",
            "Piranesi" to "Susanna Clarke",
            "A Memory Called Empire" to "Arkady Martine",
        )) {
            model.add(LibraryModel.emptyInput().copy(title = title, author = author))
        }
    }

    private fun shoot(name: String, dark: Boolean, fontScale: Float = 1f) {
        compose.setContent {
            val density = LocalDensity.current
            CompositionLocalProvider(LocalDensity provides Density(density.density, fontScale)) {
                PocketLibraryApp(model, dark = dark)
            }
        }
        compose.waitUntilAtLeastOneExists(hasText("Piranesi"), 10_000)
        compose.waitForIdle()
        val bitmap = compose.onRoot().captureToImage().asAndroidBitmap()
        PlatformTestStorageRegistry.getInstance().openOutputFile("screenshots/$name.png").use {
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)
        }
    }

    @Test
    fun libraryLight() = shoot("library-light", dark = false)

    @Test
    fun libraryDark() = shoot("library-dark", dark = true)

    @Test
    fun libraryLargeText() = shoot("library-light-200", dark = false, fontScale = 2f)
}
