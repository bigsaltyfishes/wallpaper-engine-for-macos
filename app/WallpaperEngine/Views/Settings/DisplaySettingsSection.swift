import SwiftUI

struct DisplaySettingsSection: View {
    @Environment(BridgeStore.self) private var store
    @State private var presentedError: BridgeErrorAlert?
    @State private var displaySettingsInProgress = false

    var body: some View {
        Section("Display Settings") {
            ForEach(store.settingsSnapshot.displays, id: \.displayId) { display in
                let isPrimary = display.displayId == "primary"
                VStack(alignment: .leading, spacing: 10) {
                    Text(display.title)
                        .font(.headline)

                    Toggle("Enable", isOn: Binding {
                        display.enabled
                    } set: { enabled in
                        performAsyncBridgeAction {
                            try await store.setDisplayEnabledAsync(displayId: display.displayId, enabled: enabled)
                        }
                    })
                    .disabled(isPrimary || displaySettingsInProgress)

                    Picker("Display Mode", selection: Binding {
                        display.mode
                    } set: { mode in
                        performAsyncBridgeAction {
                            try await store.setDisplayModeAsync(displayId: display.displayId, mode: mode)
                        }
                    }) {
                        Text("Standalone").tag(BridgeDisplayMode.standalone)
                        Text("Mirror").tag(BridgeDisplayMode.mirror)
                    }
                    .pickerStyle(.menu)
                    .disabled(isPrimary || displaySettingsInProgress)

                    Toggle("Horizontal Flip", isOn: Binding {
                        display.horizontalFlip
                    } set: { enabled in
                        performAsyncBridgeAction {
                            try await store.setDisplayHorizontalFlipAsync(
                                displayId: display.displayId,
                                enabled: enabled
                            )
                        }
                    })
                    .disabled(displaySettingsInProgress)

                    if !isPrimary && display.mode == .mirror {
                        Picker(
                            "Mirror Target",
                            selection: Binding {
                                display.selectedMirrorTarget ?? ""
                            } set: { targetDisplayId in
                                guard !targetDisplayId.isEmpty else {
                                    return
                                }
                                performAsyncBridgeAction {
                                    try await store.setMirrorTargetAsync(
                                        displayId: display.displayId,
                                        targetDisplayId: targetDisplayId
                                    )
                                }
                            }
                        ) {
                            Text("None").tag("")
                            ForEach(display.mirrorTargets, id: \.self) { targetDisplayId in
                                Text(title(for: targetDisplayId)).tag(targetDisplayId)
                            }
                        }
                        .pickerStyle(.menu)
                        .disabled(displaySettingsInProgress)
                    }
                }
                .padding(.vertical, 6)
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

    private func title(for displayId: String) -> String {
        store.settingsSnapshot.displays
            .first { $0.displayId == displayId }?
            .title ?? displayId
    }

    private func performAsyncBridgeAction(_ action: @escaping () async throws -> Void) {
        guard !displaySettingsInProgress else {
            return
        }

        displaySettingsInProgress = true
        Task {
            do {
                try await action()
                presentedError = nil
            } catch {
                presentedError = BridgeErrorAlert(error: error)
            }
            displaySettingsInProgress = false
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
