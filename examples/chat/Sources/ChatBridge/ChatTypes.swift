import Foundation
@preconcurrency import ForgeFFI

// The chat records and enums are used as UniFFI generates them (Sendable,
// Equatable, Hashable structs and enums); the bridge only re-exports them so
// the app never imports ForgeFFI.

public typealias ChatConversation = ForgeFFI.ChatConversation
public typealias ChatMessage = ForgeFFI.ChatMessage
public typealias ChatMessageStatus = ForgeFFI.ChatMessageStatus
public typealias ChatEvent = ForgeFFI.ChatEvent

extension ChatEvent {
    /// The room this event is about, if it is about one room.
    /// Only `eventsDropped` carries none.
    public var conversationId: String? {
        switch self {
        case let .conversationUpdated(conversation): return conversation.id
        case let .messageReceived(message): return message.conversationId
        case let .notify(message): return message.conversationId
        case let .streamConnected(conversationId): return conversationId
        case let .streamDisconnected(conversationId, _, _): return conversationId
        case let .messageStatusChanged(conversationId, _, _): return conversationId
        case .eventsDropped: return nil
        }
    }
}

extension ChatMessage {
    /// Server time (or device time while unsent) as a `Date`.
    public var sentDate: Date { Date(timeIntervalSince1970: TimeInterval(sentAt) / 1000) }
}

extension ChatConversation {
    public var lastMessageDate: Date? {
        lastMessageAt.map { Date(timeIntervalSince1970: TimeInterval($0) / 1000) }
    }
}
