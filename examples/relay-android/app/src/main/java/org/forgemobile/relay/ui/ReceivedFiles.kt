// Opening what arrived: a received file opens in whichever app shows its type, through a
// FileProvider (Quick Look on the iPhone). Received files stay in Relay's private folder;
// Save to Device (SaveToDevice.kt) copies one out.
package org.forgemobile.relay.ui

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.webkit.MimeTypeMap
import androidx.core.content.FileProvider
import java.io.File
import org.forgemobile.relay.kit.ForgeFailure

object ReceivedFiles {
    /** Opens the file at [path] (of type [mime] when its name doesn't say); tells [fail] why it couldn't. */
    fun open(context: Context, path: String, fail: (ForgeFailure) -> Unit, mime: String? = null) {
        val file = File(path)
        if (file.isDirectory) {
            fail(ForgeFailure("A Folder", "Several items arrived together in this folder. To open them, long-press it in History or in its conversation, choose Save to Device…, then open it in the Files app."))
            return
        }
        if (!file.exists()) {
            fail(ForgeFailure("Not Found", "The file is no longer on this device."))
            return
        }
        val uri = try {
            FileProvider.getUriForFile(context, "${context.packageName}.files", file)
        } catch (e: IllegalArgumentException) {
            fail(ForgeFailure("Can't Open This", "The file is outside Relay's folder."))
            return
        }
        // A file kept under its attachment id has no extension: its type comes with it.
        // octet-stream says nothing, and as a type it would hide the apps "*/*" offers.
        val type = mime?.takeIf { it != "application/octet-stream" }
            ?: MimeTypeMap.getSingleton().getMimeTypeFromExtension(file.extension.lowercase())
            ?: "*/*"
        val intent = Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, type)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        try {
            context.startActivity(Intent.createChooser(intent, file.name).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        } catch (e: ActivityNotFoundException) {
            fail(ForgeFailure("No App for This", "No app on this phone opens ${file.name}."))
        }
    }

}
