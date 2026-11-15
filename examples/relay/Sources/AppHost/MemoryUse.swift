#if os(iOS)
import Darwin
import SwiftUI

/// This app's memory footprint, the number iOS holds against its limit
/// (`phys_footprint`). Relay M1 checks a transfer's memory on the phone.
enum MemoryUse {
    static func footprint() -> UInt64? {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(
            MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size
        )
        let result = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        return result == KERN_SUCCESS ? info.phys_footprint : nil
    }
}

/// "Memory 42 MB, peak 61 MB" since the screen opened, twice a second.
struct MemoryLine: View {
    @State private var now: UInt64 = 0
    @State private var peak: UInt64 = 0

    var body: some View {
        Text("Memory \(RelayFormat.size(now)), peak \(RelayFormat.size(peak))")
            .font(.caption.monospacedDigit())
            .foregroundStyle(.secondary)
            .accessibilityIdentifier("transfer.memory")
            .task {
                while !Task.isCancelled {
                    if let footprint = MemoryUse.footprint() {
                        now = footprint
                        peak = max(peak, footprint)
                    }
                    try? await Task.sleep(for: .milliseconds(500))
                }
            }
    }
}
#endif
