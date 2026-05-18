import SwiftUI

struct AboutSection: View {
    @Environment(BridgeStore.self) private var store

    var body: some View {
        Section("About") {
            VStack(alignment: .leading, spacing: 8) {
                Text("Wallpaper Engine")
                    .font(.title3.bold())
                LabeledContent("App", value: store.settingsSnapshot.appVersion)
                LabeledContent("Bridge", value: store.settingsSnapshot.bridgeVersion)
                LabeledContent("Core", value: store.settingsSnapshot.coreVersion)
                LabeledContent("Git", value: store.settingsSnapshot.gitSha)
            }
            .padding(.vertical, 8)
        }
    }
}
