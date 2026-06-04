import SwiftUI

private let projectURL = URL(string: "https://github.com/bigsaltyfishes/wallpaper-engine-for-macos.git")!

struct AboutSection: View {
    @Environment(BridgeStore.self) private var store

    var body: some View {
        Section("About") {
            VStack(alignment: .leading, spacing: 8) {
                Text("Wallpaper Engine")
                    .font(.title3.bold())
                LabeledContent("App", value: appVersion)
                LabeledContent("Bridge", value: store.settingsSnapshot.bridgeVersion)
                LabeledContent("Core", value: store.settingsSnapshot.coreVersion)
                LabeledContent("Web", value: store.settingsSnapshot.webVersion)
                LabeledContent("Shader Pipeline", value: store.settingsSnapshot.shaderPipelineVersion)
                LabeledContent("Git", value: gitCommitHash)
                LabeledContent("Project URL") {
                    Link("Link", destination: projectURL)
                }
            }
            .padding(.vertical, 8)
        }
    }

    private var appVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
            ?? ""
    }

    private var gitCommitHash: String {
        store.settingsSnapshot.gitSha
    }
}
