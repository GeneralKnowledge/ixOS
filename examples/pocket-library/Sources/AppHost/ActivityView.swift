@preconcurrency import ForgeFFI
import SwiftUI

/// The Activity tab: what you're reading, a few totals, and your reading
/// history by day. All numbers come from Rust (`Library.stats`/`history`).
struct ActivityView: View {
    @Environment(LibraryModel.self) private var model

    var body: some View {
        NavigationStack {
            List {
                let reading = model.allBooks(status: .reading)
                if !reading.isEmpty {
                    Section("Reading Now") {
                        ForEach(reading, id: \.id) { book in
                            NavigationLink(value: book.id) {
                                BookRow(book: book)
                            }
                        }
                    }
                }
                if let stats = model.stats {
                    Section("Summary") {
                        StatRow(label: "Finished this year", value: "\(stats.finishedThisYear)", symbol: "checkmark.circle", tint: .green)
                        StatRow(label: "Pages in the last 7 days", value: "\(stats.pagesLast7Days)", symbol: "book.pages", tint: .blue)
                        StatRow(label: "Want to read", value: "\(stats.wantToRead)", symbol: "bookmark", tint: .orange)
                        StatRow(label: "Finished in total", value: "\(stats.finished)", symbol: "books.vertical", tint: .secondary)
                        if let average = stats.averageRating {
                            StatRow(
                                label: "Average rating",
                                value: average.formatted(.number.precision(.fractionLength(1))) + " ★",
                                symbol: "star", tint: .yellow
                            )
                        }
                    }
                }
                if model.history.isEmpty {
                    Section("History") {
                        Text("Your reading history appears here as you add books and update your progress.")
                            .foregroundStyle(.secondary)
                    }
                } else {
                    ForEach(days, id: \.day) { group in
                        Section(group.day.formatted(.dateTime.weekday(.wide).day().month(.wide))) {
                            ForEach(group.entries, id: \.id) { entry in
                                ActivityRow(entry: entry, showTitle: true)
                            }
                        }
                    }
                }
            }
            .groupedList()
            .navigationTitle("Activity")
            .navigationDestination(for: UInt64.self) { id in
                BookDetailView(bookID: id)
            }
        }
    }

    /// History grouped by calendar day in the user's time zone, newest first.
    private var days: [(day: Date, entries: [ActivityEntry])] {
        let calendar = Calendar.current
        let grouped = Dictionary(grouping: model.history) { entry in
            calendar.startOfDay(for: Dates.date(entry.atMs))
        }
        return grouped
            .map { (day: $0.key, entries: $0.value.sorted { $0.atMs > $1.atMs }) }
            .sorted { $0.day > $1.day }
    }
}

private struct StatRow: View {
    let label: String
    let value: String
    let symbol: String
    let tint: Color

    var body: some View {
        LabeledContent {
            Text(value)
                .font(.body.monospacedDigit().weight(.semibold))
                .foregroundStyle(.primary)
        } label: {
            Label {
                Text(label)
            } icon: {
                Image(systemName: symbol).foregroundStyle(tint)
                    .accessibilityHidden(true)
            }
        }
    }
}

/// One history entry, e.g. "Read pages 12–40".
struct ActivityRow: View {
    let entry: ActivityEntry
    let showTitle: Bool

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: symbol)
                .foregroundStyle(tint)
                .frame(width: 22)
                // Decorative; its symbol name would be its identifier.
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(summary)
                if showTitle {
                    Text(entry.bookTitle)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            Spacer()
            Text(Dates.date(entry.atMs), format: .dateTime.hour().minute())
                .font(.caption)
                .foregroundStyle(.secondary)
                .monospacedDigit()
        }
        .accessibilityElement(children: .combine)
    }

    private var summary: String {
        switch entry.kind {
        case .added: return "Added to library"
        case .started: return "Started reading"
        case .progress(let from, let to):
            let pages = to - from
            return "Read \(pages) page\(pages == 1 ? "" : "s") (to page \(to))"
        case .finished: return "Finished"
        case .statusChanged(let status): return "Moved to \(status.label)"
        }
    }

    private var symbol: String {
        switch entry.kind {
        case .added: return "plus.circle"
        case .started: return "book"
        case .progress: return "book.pages"
        case .finished: return "checkmark.circle.fill"
        case .statusChanged(let status): return status.symbol
        }
    }

    private var tint: Color {
        switch entry.kind {
        case .added: return .secondary
        case .started, .progress: return .blue
        case .finished: return .green
        case .statusChanged(let status): return status.tint
        }
    }
}
