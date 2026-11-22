@preconcurrency import PocketLibraryFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// The Activity tab: what you're reading, a few totals, and your reading
/// history by day. All numbers come from Rust (`Library.stats`/`history`).
struct ActivityView: View {
    @Environment(LibraryModel.self) private var model

    private var bindable: Bindable<LibraryModel> { Bindable(model) }

    var body: some View {
        NavigationStack {
            List {
                let reading = model.allBooks(status: .reading)
                if !reading.isEmpty {
                    Section("Reading Now") {
                        ForEach(reading, id: \.id) { book in
                            NavigationRow { model.activityDestination = .book(book.id) } label: {
                                BookRow(book: book)
                            }
                            .accessibilityIdentifier("activity.book.\(book.id)")
                        }
                    }
                }
                if let stats = model.stats {
                    Section("Summary") {
                        ForgeRow("Finished this year", systemImage: "checkmark.circle", accent: .green,
                                 value: stats.finishedThisYear.formatted())
                        ForgeRow("Pages in the last 7 days", systemImage: "book.pages", accent: .blue,
                                 value: stats.pagesLast7Days.formatted())
                        ForgeRow("Want to read", systemImage: "bookmark", accent: .orange,
                                 value: stats.wantToRead.formatted())
                        ForgeRow("Finished in total", systemImage: "books.vertical", accent: .graphite,
                                 value: stats.finished.formatted())
                        if let average = stats.averageRating {
                            ForgeRow("Average rating", systemImage: "star", accent: .orange,
                                     value: average.formatted(.number.precision(.fractionLength(1))) + " ★")
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
            .forgeNavigationDestination(bindable.activityDestination, \.book) { id in
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

/// One history entry, e.g. "Read pages 12–40".
struct ActivityRow: View {
    let entry: ActivityEntry
    let showTitle: Bool
    @Environment(\.forgeTheme) private var theme

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
        case .added: return theme.colors.textSecondary
        case .started, .progress: return theme.colors.tone(ReadingStatus.reading.tone).solid
        case .finished: return theme.colors.tone(ReadingStatus.finished.tone).solid
        case .statusChanged(let status): return theme.colors.tone(status.tone).solid
        }
    }
}
