/// The checks for the next phone sitting, shown on the Checks screen
/// (Tools tab). Whoever prepares a sitting replaces this text in the same
/// PR that the build comes from: `# title`, `## group`, then numbered or
/// `- ` items, one line each. Answers are kept per build number.
enum PhoneChecks {
    static let text = """
    # Next Device Lab sitting

    ## Web images (#298)
    1. Pocket Library covers, Soundscape artwork and a Commonplace article with pictures load.
    2. Scroll away and back: nothing blinks.
    3. Airplane mode on, reopen them: pictures already seen still show.

    ## Soundscape lock screen and interruptions (#306)
    4. Play a song and lock the phone: the lock screen shows title, artwork, play time and a draggable progress bar.
    5. Start audio in another app while a song plays: Soundscape's button switches to Play, and Play continues it.
    6. Pocket Library Read Aloud, then lock the phone: the lock screen shows it.

    ## Device Lab home (#307)
    7. Device Lab opens on Apps, with This Build first and its new icon.
    8. This iPhone: the sensors still update.
    9. Workshop button: Switch App goes straight to another app, and moving the button no longer flashes the menu.

    ## Device Lab tools
    10. Tools, Logs: records show, the filter and search work, and Share sends them as text.
    11. Tools, Servers: the chat server and the take server show their addresses, Up with a time when running, Down with a reason when stopped.
    12. Tools, Checks: this list; tick a few, add a note, Share the results.

    ## This iPhone additions
    13. Network: Wi-Fi and Tailscale addresses show (Tailscale on), and change when you switch Wi-Fi off.
    14. Audio: Output says Speaker, then Headphones or AirPods when connected; the volume follows the buttons.
    15. Audio: play a song in Soundscape, take or fake a call, come back: Interruptions and Changes lists it.
    16. Battery & Power: after 10 minutes on battery, Drain shows a rate; Heat Changes lists the start state.
    17. What iOS Keeps from Apps: the entry now says MAC Address only.
    """
}
