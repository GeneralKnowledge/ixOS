package org.forgemobile.pocketlibrary.kit

import java.net.SocketTimeoutException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.runBlocking
import org.forgemobile.pocketlibrary.ErrorText
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import uniffi.pocket_library_core.LibraryException

/** The ForgeAppKit counterparts behave as their Swift originals do. */
class ForgeKitTest {
    @Before
    fun describeTheAppsErrors() {
        ForgeFailure.describer = ErrorText
    }

    @Test
    fun failuresUseTheAppsWordsThenTheNetworksThenAFallback() {
        val offline = ForgeFailure.from(LibraryException.Offline("no route"))
        assertEquals("You're Offline", offline.title)
        assertTrue(offline.isRetryable)
        assertEquals("No Response", ForgeFailure.from(SocketTimeoutException()).title)
        assertEquals("Something Went Wrong", ForgeFailure.from(IllegalStateException("x")).title)
    }

    @Test
    fun cancellationIsNotAFailure() {
        assertTrue(ForgeFailure.isCancellation(CancellationException("left")))
        assertTrue(ForgeFailure.isCancellation(LibraryException.Cancelled()))
        assertNull(ForgeAlert.failure<Unit>(LibraryException.Cancelled()))
        assertFalse(ForgeFailure.isCancellation(LibraryException.ShutDown()))
    }

    @Test
    fun aRetryableFailureOffersTryAgain() {
        val alert = ForgeAlert.failure(ForgeFailure.from(LibraryException.Timeout()), retry = "again")
        assertEquals(listOf("alert.retry", "alert.ok"), alert.buttons.map { it.identifier })
        assertEquals("again", alert.action("Try Again"))
        val plain = ForgeAlert.failure(ForgeFailure.from(LibraryException.NotFound(7u)), retry = "again")
        assertEquals(listOf("alert.ok"), plain.buttons.map { it.identifier })
    }

    @Test
    fun aConfirmationNamesItsButtonsFromItsIdentifier() {
        val alert = ForgeAlert.confirm("Delete?", confirm = "Delete Book", action = 1, identifier = "bookDetail.delete", destructive = true)
        assertEquals(listOf("bookDetail.delete.confirm", "bookDetail.delete.cancel"), alert.buttons.map { it.identifier })
        assertEquals(ForgeAlertButton.Role.DESTRUCTIVE, alert.buttons.first().role)
        assertEquals(ForgeAlert.Style.CONFIRMATION, alert.style)
        assertEquals(1, alert.action("Delete Book"))
    }

    @Test
    fun loadCapturesValuesFailuresAndCancellation() = runBlocking {
        assertEquals(ForgeLoad.Loaded(3), ForgeLoad.capture { 3 })
        val failed = ForgeLoad.capture<Int> { throw LibraryException.NotFound(1u) }
        assertEquals("Book Not Found", failed.failure?.title)
        assertEquals(ForgeLoad.Idle, ForgeLoad.capture<Int> { throw CancellationException() })
        // A cancelled reload keeps what was showing.
        val loaded: ForgeLoad<Int> = ForgeLoad.Loaded(1)
        assertEquals(loaded, loaded.reloaded { throw CancellationException() })
        assertEquals(loaded, loaded.whileReloading)
        assertEquals(ForgeLoad.Loading, ForgeLoad.Idle.whileReloading)
        assertEquals(ForgeLoad.Loaded("1"), loaded.map { it.toString() })
    }

    @Test
    fun aSubmissionKeepsItsFailureAndRunsOnceAtATime() = runBlocking {
        val submission = ForgeSubmission()
        assertTrue(submission.run {})
        assertFalse(submission.run { throw LibraryException.Storage("disk full") })
        assertEquals("Couldn't Save", submission.failure.value?.title)
        assertFalse(submission.isWorking.value)
        submission.dismissFailure()
        assertNull(submission.failure.value)

        var inner: Boolean? = null
        submission.run { inner = submission.run {} }
        assertEquals(false, inner)
    }

    @Test
    fun fieldErrorsKeepTheFirstMessageInOrder() {
        val errors = ForgeFieldErrors(listOf("title" to "Enter a title.", "rating" to "1 to 5", "title" to "later"))
        assertEquals(listOf("title", "rating"), errors.fields)
        assertEquals("Enter a title.", errors["title"])
        assertEquals("Enter a title.\n1 to 5", errors.summary)
        assertEquals(listOf("rating"), errors.minus("title").fields)
        assertTrue(ForgeFieldErrors().isEmpty)
    }
}
