import SwiftUI

struct UpdateAvailableWindow: View {
    let update: AvailableUpdate
    let onClose: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text("New Version Detected")
                .font(.title2.bold())

            LabeledContent("Version", value: update.version)

            VStack(alignment: .leading, spacing: 8) {
                Text("Release Notes:")
                    .font(.headline)
                ScrollView {
                    releaseNotes
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.vertical, 4)
                }
                .frame(minHeight: 220)
            }

            HStack {
                Spacer()
                Button("Close") {
                    onClose()
                }
                Button("Open Release Page") {
                    NSWorkspace.shared.open(update.releaseURL)
                    onClose()
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(24)
        .frame(width: 560)
        .frame(minHeight: 380)
    }

    @ViewBuilder
    private var releaseNotes: some View {
        if let markdown = try? AttributedString(markdown: update.releaseNotes) {
            Text(markdown)
                .textSelection(.enabled)
        } else {
            Text(update.releaseNotes)
                .textSelection(.enabled)
        }
    }
}
