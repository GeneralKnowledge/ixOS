package org.forgemobile.relay

import android.content.Intent
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.forgemobile.relay.host.ForgeEnvironment
import org.forgemobile.relay.host.ForgeServices
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.relay_core.CodeSend
import uniffi.relay_core.RelayCore

/**
 * Relay's Devices experience on an emulator. Controls are found by the iPhone app's identifiers
 * (`screen.element`), never by position. Each test launches on a fresh throwaway identity with
 * no servers, so the person's own Relay is never touched and nothing reaches Relay's servers.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(AndroidJUnit4::class)
class DevicesFlowTest {
    @get:Rule
    val compose = createEmptyComposeRule()

    private var scenario: ActivityScenario<MainActivity>? = null
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private var laptop: RelayCore? = null
    private var laptopNetwork: RelayNetwork? = null

    private fun launch(toDevices: Boolean = true) {
        val intent = Intent(context, MainActivity::class.java)
            .putExtra(ForgeEnvironment.TEMPORARY_DATA_EXTRA, true)
        scenario = ActivityScenario.launch(intent)
        compose.waitUntilAtLeastOneExists(hasTestTag("tab.devices") or hasTestTag("content.failure"), 30_000)
        val failure = compose.onAllNodes(hasTestTag("content.failure")).fetchSemanticsNodes().firstOrNull()
        if (failure != null) {
            val text = failure.config.getOrNull(SemanticsProperties.Text)?.joinToString { it.text }
            throw AssertionError("Relay didn't open: $text")
        }
        if (!toDevices) return
        compose.onNodeWithTag("tab.devices").performClick()
        compose.waitUntilAtLeastOneExists(hasTestTag("devices.empty"), 10_000)
    }

    @After
    fun finish() {
        val network = laptopNetwork
        if (network != null) {
            InstrumentationRegistry.getInstrumentation().runOnMainSync { network.close() }
        }
        laptop?.let {
            it.shutdown()
            it.destroy()
        }
        scenario?.close()
    }

    @Test
    fun relayOpensOnAnEmptyChats() {
        launch(toDevices = false)
        // Relay opens on Chats, as on the iPhone: with no trusted devices it has none.
        compose.waitUntilAtLeastOneExists(hasTestTag("chats.empty"), 10_000)
    }

    @Test
    fun chatsOfferToJoinAChatServer() {
        launch()
        compose.onNodeWithTag("tab.chats").performClick()
        compose.onNodeWithTag("chats.server").performClick()
        compose.waitUntilAtLeastOneExists(hasTestTag("signUp.create"), 5_000)
        compose.onNodeWithTag("signUp.create").assertIsNotEnabled()
        compose.onNodeWithTag("signUp.name").performTextInput("Test Phone")
        compose.onNodeWithTag("signUp.server").performTextInput("http://127.0.0.1:9")
        compose.onNodeWithTag("signUp.create").assertIsEnabled()
        // Nothing listens there: the form says so and stays, with what was typed.
        compose.onNodeWithTag("signUp.create").performClick()
        compose.waitUntilAtLeastOneExists(hasText("Can't Create the Account"), 15_000)
        compose.onNodeWithTag("alert.ok").performClick()
        compose.onNodeWithTag("signUp.name").assert(hasText("Test Phone"))
        compose.onNodeWithTag("chatServer.back").performClick()
        compose.waitUntilAtLeastOneExists(hasTestTag("chats.empty"), 5_000)
    }

    @Test
    fun aNewDeviceOffersToSendWithACode() {
        launch()
        compose.onNodeWithTag("devices.empty").assertIsDisplayed()
        compose.onNodeWithTag("devices.sendFirst").performClick()
        compose.onNodeWithTag("sendCode.status").assertIsDisplayed()
        compose.onNodeWithText("Choose files, photos or a folder, and Relay shows a code for the receiver.").assertIsDisplayed()
        compose.onNodeWithTag("sendCode.close").performClick()
        compose.onNodeWithTag("devices.empty").assertIsDisplayed()
    }

    @Test
    fun renamingIsCheckedAndKept() {
        launch()
        compose.onNodeWithTag("devices.rename").performClick()
        compose.onNodeWithTag("rename.name").performTextReplacement("   ")
        compose.onNodeWithTag("rename.save").performClick()
        compose.waitUntilAtLeastOneExists(hasText("Give this device a name."), 5_000)
        compose.onNodeWithTag("rename.name").performTextReplacement("Test Pixel")
        compose.onNodeWithTag("rename.save").performClick()
        compose.waitUntilDoesNotExist(hasTestTag("rename.name"), 5_000)
        compose.onNodeWithTag("devices.rename").performClick()
        compose.onNodeWithTag("rename.name").assert(hasText("Test Pixel"))
        compose.onNodeWithTag("rename.cancel").performClick()
    }

    @Test
    fun aMistypedCodeIsSaidAtOnce() {
        launch()
        compose.onNodeWithTag("relay.receive").performClick()
        compose.onNodeWithTag("receiveCode.code").performTextInput("not a code")
        compose.onNodeWithTag("receiveCode.start").performClick()
        compose.waitUntil(5_000) {
            compose.onAllNodes(hasTestTag("receiveCode.status")).fetchSemanticsNodes().any { node ->
                node.config.getOrNull(SemanticsProperties.Text)?.any { "Looking" !in it.text && "Type the code" !in it.text } == true
            }
        }
        compose.onNodeWithTag("receiveCode.close").performClick()
        compose.onNodeWithTag("devices.empty").assertIsDisplayed()
    }

    /**
     * Another device (a second Relay core in this process, as a laptop would be) sends a file
     * with a code and advertises it on the network; the app finds it by the code's number,
     * receives, trusts it, and the device's page changes what it may do.
     */
    @Test
    fun receiveWithACodeThenTrustAndRevoke() {
        launch()
        val dir = File(context.cacheDir, "test-laptop").apply {
            deleteRecursively()
            mkdirs()
        }
        // One process holds one device key (the secure store's), so the laptop gets a store of
        // its own while its node starts, once the app's own key is in place.
        val store = ForgeServices.secureStore
        val appKeys = store.throwaway!!
        compose.waitUntil(20_000) { appKeys.containsKey("relay.device-key") }
        store.throwaway = java.util.concurrent.ConcurrentHashMap()
        val other = RelayCore.withoutDefaultServers(File(dir, "data").absolutePath, "Test Laptop")
        laptop = other
        val file = File(dir, "hello.txt").apply { writeText("hello from the laptop") }
        var waiting: CodeSend.Waiting? = null
        try {
            other.sendPathsWithCode(listOf(file.absolutePath))
            compose.waitUntil(20_000) {
                waiting = other.codeSendStatus() as? CodeSend.Waiting
                waiting != null
            }
        } finally {
            store.throwaway = appKeys
        }
        val code = waiting!!
        InstrumentationRegistry.getInstrumentation().runOnMainSync {
            laptopNetwork = RelayNetwork(context).also {
                it.advertiseCode(CodeAdvert(code.number, code.port, code.endpointId))
            }
        }

        compose.onNodeWithTag("relay.receive").performClick()
        compose.onNodeWithTag("receiveCode.code").performTextInput(code.code)
        compose.onNodeWithTag("receiveCode.start").performClick()
        compose.waitUntilAtLeastOneExists(hasTestTag("receiveCode.open"), 40_000)
        compose.onNodeWithText("hello.txt", substring = true).assertIsDisplayed()
        compose.onNodeWithTag("receiveCode.close").performClick()

        // Back on the devices: Relay asks whether to trust the laptop.
        compose.waitUntilAtLeastOneExists(hasTestTag("trustOffer.trust"), 10_000)
        compose.onNodeWithText("Trust “Test Laptop”?").assertIsDisplayed()
        compose.onNodeWithTag("trustOffer.trust").performClick()
        // The new row shows at the top, even on a short screen.
        compose.waitUntilAtLeastOneExists(hasTestTag("devices.trusted"), 10_000)
        compose.onNodeWithTag("devices.trusted").assertIsDisplayed()
        compose.onNodeWithTag("devices.list").performScrollToNode(hasTestTag("history.row"))
        compose.onNodeWithTag("history.row").assertIsDisplayed()

        // Its page: it may send here until that is turned off.
        compose.onNodeWithTag("devices.list").performScrollToNode(hasTestTag("devices.trusted"))
        compose.onNodeWithTag("devices.trusted").performClick()

        // Message opens its conversation in Chats; what is written waits in Relay's outbox
        // until the laptop can be reached, and shows at once.
        compose.onNodeWithTag("trusted.message").performClick()
        compose.waitUntilAtLeastOneExists(hasTestTag("chat.composer"), 10_000)
        compose.onNodeWithText("Test Laptop").assertIsDisplayed()
        compose.onNodeWithTag("chat.composer").performTextInput("hello laptop")
        compose.onNodeWithTag("chat.send").performClick()
        // In the transcript, not just still in the composer.
        compose.waitUntilAtLeastOneExists(hasText("hello laptop") and hasAnyAncestor(hasTestTag("chat.transcript")), 10_000)
        compose.onNodeWithTag("chat.back").performClick()
        compose.waitUntilAtLeastOneExists(hasTestTag("chats.list"), 5_000)
        compose.waitUntilAtLeastOneExists(hasText("You: hello laptop"), 5_000)
        compose.onNodeWithTag("tab.devices").performClick()

        compose.onNodeWithTag("trusted.receivesFrom").assertIsOn()
        compose.onNodeWithTag("trusted.receivesFrom").performClick()
        compose.waitUntil(5_000) {
            compose.onAllNodes(hasText("Test Laptop is still trusted, but what it sends here is declined.")).fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithTag("trusted.receivesFrom").assertIsOff()

        // Revoking asks first, then the laptop is listed as revoked.
        compose.onNodeWithTag("trusted.more").performClick()
        compose.onNodeWithTag("trusted.revoke").performClick()
        compose.onNodeWithTag("trusted.confirmRevoke.confirm").performClick()
        compose.waitUntilAtLeastOneExists(hasTestTag("trusted.gone"), 5_000)
        compose.onNodeWithTag("trusted.back").performClick()
        compose.waitUntil(5_000) {
            runCatching { compose.onNodeWithTag("devices.list").performScrollToNode(hasTestTag("revoked.trustAgain")) }.isSuccess
        }
        assertTrue(compose.onAllNodes(hasTestTag("devices.trusted")).fetchSemanticsNodes().isEmpty())
    }
}
