package org.forgemobile.pocketlibrary.kit

/**
 * A form's problems by field (ForgeFieldErrors in Swift), shown under each field. Field names
 * are the Rust input's (`current_page`), as `forge_app::Validator` reports them. Only the first
 * message for a field is kept, and fields keep the order they were added in.
 */
class ForgeFieldErrors private constructor(private val messages: LinkedHashMap<String, String>) {
    constructor() : this(LinkedHashMap())

    constructor(pairs: Iterable<Pair<String, String>>) : this(LinkedHashMap()) {
        for ((field, message) in pairs) messages.putIfAbsent(field, message)
    }

    val fields: List<String> get() = messages.keys.toList()
    val isEmpty: Boolean get() = messages.isEmpty()

    operator fun get(field: String): String? = messages[field]

    /** A copy with [message] for [field], unless the field already has one. */
    fun plus(field: String, message: String): ForgeFieldErrors =
        ForgeFieldErrors(LinkedHashMap(messages).apply { putIfAbsent(field, message) })

    /** A copy without [field]'s message (the person changed that field). */
    fun minus(field: String): ForgeFieldErrors =
        ForgeFieldErrors(LinkedHashMap(messages).apply { remove(field) })

    /** Every message, one per line, in field order. */
    val summary: String get() = messages.values.joinToString("\n")

    override fun equals(other: Any?): Boolean =
        other is ForgeFieldErrors && messages.toList() == other.messages.toList()

    override fun hashCode(): Int = messages.toList().hashCode()

    override fun toString(): String = "ForgeFieldErrors($messages)"
}
