package org.forgemobile.pocketlibrary

import org.forgemobile.pocketlibrary.host.ForgeEnvironment
import uniffi.forge_ffi.setAsideUnreadableLibrary
import uniffi.pocket_library_core.LibraryException
import uniffi.pocket_library_core.PocketLibrary

/**
 * Opens Pocket Library's core for the host, and starts it fresh when the person asked to (the
 * iPhone app's `LibraryOpenRequest`). Starting fresh moves the unreadable `library.json` aside,
 * kept as a backup file, and opens an empty library.
 */
class LibraryOpener {
    /** Set by [LibraryModel.startFresh]; cleared once a library has opened. */
    @Volatile
    var fresh = false

    fun open(env: ForgeEnvironment): LibraryCore {
        // `Library.open_fresh` in one step on iOS; its Kotlin doesn't compile (uniffi.toml).
        if (fresh) setAsideUnreadableLibrary(env.dataDirectory)
        val core = LibraryCore(openWaitingForClose(env.dataDirectory))
        fresh = false
        return core
    }

    /**
     * A library this process just closed keeps its folder's lock until the last call still
     * running on it returns (a list reload, say), so a reopen straight after waits a moment for
     * it. Another copy of the app holding the folder still fails, after [CLOSE_WAIT_MS].
     */
    private fun openWaitingForClose(directory: String): PocketLibrary {
        val deadline = System.nanoTime() + CLOSE_WAIT_MS * 1_000_000
        while (true) {
            try {
                return PocketLibrary(directory)
            } catch (e: LibraryException.InUse) {
                if (System.nanoTime() >= deadline) throw e
                Thread.sleep(50)
            }
        }
    }

    private companion object {
        const val CLOSE_WAIT_MS = 2_000L
    }
}
