// User-facing text for Pocket Library's Rust errors: the same words as the iPhone app
// (examples/pocket-library/Sources/AppHost/ErrorText.swift). `reason` fields are diagnostics,
// shown as detail.
package org.forgemobile.pocketlibrary

import org.forgemobile.pocketlibrary.kit.ForgeErrorDescriber
import org.forgemobile.pocketlibrary.kit.ForgeFailure
import org.forgemobile.pocketlibrary.kit.ForgeFieldErrors
import uniffi.pocket_library_core.LibraryException

object ErrorText : ForgeErrorDescriber {
    fun title(error: LibraryException): String = when (error) {
        is LibraryException.InvalidInput, is LibraryException.InvalidFields -> "Check the Details"
        is LibraryException.NotFound -> "Book Not Found"
        is LibraryException.AlreadyInLibrary -> "Already in Your Library"
        is LibraryException.Storage, is LibraryException.NewerLibrary -> "Couldn't Save"
        is LibraryException.CatalogueDisabled -> "Online Search Is Off"
        is LibraryException.Offline -> "You're Offline"
        is LibraryException.Timeout -> "Catalogue Didn't Respond"
        is LibraryException.CatalogueUnavailable -> "Catalogue Unavailable"
        is LibraryException.BadResponse -> "Unexpected Catalogue Response"
        is LibraryException.Certificate -> "Secure Connection Failed"
        is LibraryException.Network -> "Network Problem"
        is LibraryException.UnsupportedFile -> "Can't Read That File"
        is LibraryException.NotReadable -> "Can't Open This Book"
        is LibraryException.Cancelled -> "Search Stopped"
        is LibraryException.Interrupted -> "Stopped"
        is LibraryException.ShutDown -> "Library Closed"
        is LibraryException.InUse -> "Library Already Open"
        is LibraryException.SummaryUnavailable -> "Summaries Unavailable"
        is LibraryException.SummaryRefused -> "Couldn't Summarise"
        is LibraryException.SummaryFailed -> "Summary Failed"
    }

    fun message(error: LibraryException): String = when (error) {
        is LibraryException.InvalidInput -> sentence(error.reason)
        is LibraryException.InvalidFields -> error.fields.joinToString("\n") { it.message }
        is LibraryException.NotFound -> "It may have been deleted on another screen."
        is LibraryException.AlreadyInLibrary -> "This book is already in your library."
        is LibraryException.Storage -> "Your library was not changed. ${error.reason}"
        is LibraryException.NewerLibrary ->
            "Your library was saved by a newer version of Pocket Library. Update the app to open it; nothing has been changed."
        is LibraryException.CatalogueDisabled -> "Turn on online search in Settings, or add the book by hand."
        is LibraryException.Offline ->
            "Your library still works offline. Try searching again when you're connected, or add the book by hand."
        is LibraryException.Timeout -> "Try again in a moment, or add the book by hand."
        is LibraryException.CatalogueUnavailable ->
            "Open Library is having trouble (${error.reason}). Your library is unaffected."
        is LibraryException.BadResponse ->
            "Open Library sent something Pocket Library couldn't read (${error.reason})."
        is LibraryException.Certificate -> "The server's certificate was rejected: ${error.reason}"
        is LibraryException.Network -> error.reason
        is LibraryException.UnsupportedFile -> "Pocket Library imports EPUB books. ${error.reason}"
        is LibraryException.NotReadable -> sentence(error.reason)
        is LibraryException.Cancelled -> "Nothing was changed."
        is LibraryException.Interrupted ->
            "Pocket Library went to the background before this finished. Nothing was changed; try again."
        is LibraryException.ShutDown -> "Restart the app to keep going."
        is LibraryException.InUse -> "Another copy of Pocket Library has this library open. Close it and try again."
        is LibraryException.SummaryUnavailable -> error.reason
        // iOS names Apple Intelligence; Android has no on-device model yet (plan milestone A7).
        is LibraryException.SummaryRefused -> "The on-device model declined to summarise this chapter."
        is LibraryException.SummaryFailed -> "The on-device model couldn't finish the summary (${error.reason}). Try again."
    }

    /** Whether trying the same thing again might work. */
    fun isRetryable(error: LibraryException): Boolean = when (error) {
        is LibraryException.Offline,
        is LibraryException.Timeout,
        is LibraryException.CatalogueUnavailable,
        is LibraryException.Network,
        is LibraryException.Interrupted,
        is LibraryException.SummaryFailed,
        -> true
        else -> false
    }

    override fun failure(error: Throwable): ForgeFailure? {
        if (error !is LibraryException) return null
        return ForgeFailure(title(error), message(error), isRetryable(error))
    }

    override fun isCancellation(error: Throwable): Boolean = error is LibraryException.Cancelled

    /** The form's errors from Rust's validation, by `BookInput` field name. */
    fun fieldErrors(error: LibraryException.InvalidFields): ForgeFieldErrors =
        ForgeFieldErrors(error.fields.map { it.field to it.message })

    private fun sentence(reason: String): String =
        reason.replaceFirstChar { it.uppercaseChar() } + "."
}
