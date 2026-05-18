import SwiftUI

struct ProgramSettingsSection: View {
    @Environment(BridgeStore.self) private var store
    @State private var presentedError: BridgeErrorAlert?
    @State private var launchAtLoginInProgress = false

    var body: some View {
        Section("Program Settings") {
            Toggle("Launch at Login", isOn: Binding {
                store.settingsSnapshot.launchAtLoginEnabled
            } set: { enabled in
                performAsyncBridgeAction {
                    try await store.setLaunchAtLoginAsync(enabled: enabled)
                }
            })
            .disabled(!store.settingsSnapshot.launchAtLoginAvailable || launchAtLoginInProgress)

            if !store.settingsSnapshot.launchAtLoginAvailable {
                Text("Move Wallpaper Engine to Applications to enable launch at login.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .alert(item: $presentedError) { error in
            Alert(
                title: Text("Bridge Error"),
                message: Text(error.message),
                dismissButton: .default(Text("OK"))
            )
        }
    }

    private func performAsyncBridgeAction(_ action: @escaping () async throws -> Void) {
        guard !launchAtLoginInProgress else {
            return
        }

        launchAtLoginInProgress = true
        Task {
            do {
                try await action()
                presentedError = nil
            } catch {
                presentedError = BridgeErrorAlert(error: error)
            }
            launchAtLoginInProgress = false
        }
    }
}

private struct BridgeErrorAlert: Identifiable {
    let id = UUID()
    let message: String

    init(error: Error) {
        self.message = error.localizedDescription
    }
}
