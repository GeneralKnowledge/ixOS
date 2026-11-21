package org.forgemobile.pocketlibrary

import org.forgemobile.pocketlibrary.host.ForgeEnvironment
import uniffi.forge_ffi.setAsideUnreadableLibrary
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
        val core = LibraryCore(PocketLibrary(env.dataDirectory))
        fresh = false
        return core
    }
}
