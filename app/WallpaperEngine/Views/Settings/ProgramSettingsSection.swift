import SwiftUI

struct ProgramSettingsSection: View {
    @Environment(BridgeStore.self) private var store
    @State private var presentedError: BridgeErrorAlert?
    @State private var bridgeActionInProgress = false
    @State private var showingShaderCacheWarning = false
    @State private var updateStatus: UpdateCheckStatus = .upToDate
    @State private var availableUpdate: AvailableUpdate?

    var body: some View {
        Section("Program Settings") {
            Toggle("Launch at Login", isOn: Binding {
                store.settingsSnapshot.launchAtLoginEnabled
            } set: { enabled in
                performAsyncBridgeAction {
                    try await store.setLaunchAtLoginAsync(enabled: enabled)
                }
            })
            .disabled(!store.settingsSnapshot.launchAtLoginAvailable || bridgeActionInProgress)

            if !store.settingsSnapshot.launchAtLoginAvailable {
                Text("Move Wallpaper Engine to Applications to enable launch at login.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            LabeledContent {
                Button("Clear") {
                    showingShaderCacheWarning = true
                }
                .disabled(bridgeActionInProgress)
            } label: {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Shader Cache")
                    Text(StorageFormat.bytes(store.settingsSnapshot.storage.shaderCacheSizeBytes))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            LabeledContent("Logs") {
                HStack(spacing: 8) {
                    Button("Open") {
                        openLogFolder()
                    }
                    Button("Clear") {
                        clearLogs()
                    }
                    .disabled(bridgeActionInProgress)
                }
            }

            LabeledContent {
                Button("Check for Updates") {
                    checkForUpdates()
                }
                .disabled(updateStatus == .checking)
            } label: {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Updates")
                    Text(updateStatus.label)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .alert(item: $presentedError) { error in
            Alert(
                title: Text("Bridge Error"),
                message: Text(error.message),
                dismissButton: .default(Text("OK"))
            )
        }
        .confirmationDialog(
            "Clear Shader Cache?",
            isPresented: $showingShaderCacheWarning,
            titleVisibility: .visible
        ) {
            Button("Clear Anyway", role: .destructive) {
                performAsyncBridgeAction {
                    try await store.clearShaderCacheAsync()
                }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(
                "Clearing the shader cache may temporarily reduce performance. Currently active scene wallpapers will be rebuilt to regenerate fresh shaders."
            )
        }
        .sheet(item: $availableUpdate) { update in
            UpdateAvailableWindow(update: update) {
                availableUpdate = nil
            }
        }
    }

    private func performAsyncBridgeAction(_ action: @escaping () async throws -> Void) {
        guard !bridgeActionInProgress else {
            return
        }

        bridgeActionInProgress = true
        Task {
            do {
                try await action()
                presentedError = nil
            } catch {
                presentedError = BridgeErrorAlert(error: error)
            }
            bridgeActionInProgress = false
        }
    }

    private func openLogFolder() {
        do {
            NSWorkspace.shared.open(try store.logFolderURL())
        } catch {
            presentedError = BridgeErrorAlert(error: error)
        }
    }

    private func clearLogs() {
        do {
            try store.clearLogsAsync()
        } catch {
            presentedError = BridgeErrorAlert(error: error)
        }
    }

    private func checkForUpdates() {
        guard updateStatus != .checking else {
            return
        }

        let previousStatus = updateStatus
        updateStatus = .checking
        Task {
            do {
                if let update = try await UpdateChecker().check(currentShortHash: store.settingsSnapshot.gitSha) {
                    availableUpdate = update
                    updateStatus = .updateAvailable
                } else {
                    updateStatus = .upToDate
                }
                presentedError = nil
            } catch {
                updateStatus = previousStatus == .checking ? .upToDate : previousStatus
                presentedError = BridgeErrorAlert(error: error)
            }
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
