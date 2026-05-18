import SwiftUI

struct WallpaperOptionsEditorView: View {
    @Environment(BridgeStore.self) private var store

    let options: BridgeWallpaperOptionsSnapshot
    var displayIdFilter: String?
    var displayRowsAreCollapsible = true
    var showsTitle = true
    var showsActions = true
    var scrollsContent = true
    var onError: (Error) -> Void = { _ in }

    @State private var displayExpanded = false
    @State private var generalExpanded = false
    @State private var propertiesExpanded = false
    @State private var currentWallpaperId: String?
    @State private var localResetRevision: UInt64 = 0
    @State private var applyInProgress = false
    @State private var pendingScalingFactors: [String: Double] = [:]
    @State private var invalidScalingFactorDisplayIds: Set<String> = []
    @State private var activeDisplayBridgeActionIds: Set<String> = []

    var body: some View {
        VStack(spacing: 0) {
            if scrollsContent {
                ScrollView {
                    editorContent
                        .padding(24)
                }
            } else {
                editorContent
            }

            if showsActions {
                Divider()

                HStack {
                    if options.dirty {
                        Button("Restore Defaults") {}
                            .disabled(true)
                    }

                    Spacer()

                    Button("Revert") {
                        performAsyncBridgeAction {
                            try await store.cancelWallpaperOptionsAsync(wallpaperId: options.wallpaperId)
                            clearPendingDisplayEdits()
                            localResetRevision &+= 1
                        }
                    }
                    .disabled(applyInProgress)

                    Button("Apply") {
                        performAsyncBridgeAction {
                            try await commitPendingDisplayEdits()
                            try await store.applyWallpaperOptionsAsync(wallpaperId: options.wallpaperId)
                        }
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(
                        (!options.dirty && pendingScalingFactors.isEmpty)
                            || !invalidScalingFactorDisplayIds.isEmpty
                            || applyInProgress
                            || !activeDisplayBridgeActionIds.isEmpty
                    )
                }
                .padding(14)
            }
        }
        .onAppear {
            resetExpansionIfNeeded(options.wallpaperId)
        }
        .onChange(of: options.wallpaperId) { _, wallpaperId in
            resetExpansionIfNeeded(wallpaperId)
        }
    }

    private var editorContent: some View {
        VStack(alignment: .leading, spacing: 20) {
            if showsTitle {
                Text(options.title)
                    .font(.largeTitle.bold())
                    .lineLimit(2)
            }

            DisclosureGroup(isExpanded: $displayExpanded) {
                DisplayConfigurationSection(
                    options: options,
                    snapshotRevision: store.snapshotRevision,
                    resetRevision: localResetRevision,
                    displayIdFilter: displayIdFilter,
                    rowsAreCollapsible: displayRowsAreCollapsible,
                    pendingScalingFactors: $pendingScalingFactors,
                    invalidScalingFactorDisplayIds: $invalidScalingFactorDisplayIds,
                    activeDisplayBridgeActionIds: $activeDisplayBridgeActionIds,
                    onError: onError
                )
            } label: {
                Label("Display Configuration", systemImage: "display.2")
                    .font(.headline)
            }

            DisclosureGroup(isExpanded: $generalExpanded) {
                GeneralConfigurationSection(
                    options: options,
                    snapshotRevision: store.snapshotRevision,
                    resetRevision: localResetRevision,
                    onError: onError
                )
                .id("\(options.wallpaperId)-general-\(displayIdFilter ?? "all")")
            } label: {
                Label("General Configuration", systemImage: "slider.horizontal.3")
                    .font(.headline)
            }

            DisclosureGroup(isExpanded: $propertiesExpanded) {
                WallpaperPropertiesSection(options: options, onError: onError)
            } label: {
                Label("Wallpaper Properties", systemImage: "info.circle")
                    .font(.headline)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func resetExpansionIfNeeded(_ wallpaperId: String) {
        guard currentWallpaperId != wallpaperId else {
            return
        }

        currentWallpaperId = wallpaperId
        displayExpanded = false
        generalExpanded = false
        propertiesExpanded = false
        localResetRevision = 0
        applyInProgress = false
        activeDisplayBridgeActionIds.removeAll()
        clearPendingDisplayEdits()
    }

    private func clearPendingDisplayEdits() {
        pendingScalingFactors.removeAll()
        invalidScalingFactorDisplayIds.removeAll()
    }

    private func commitPendingDisplayEdits() async throws {
        for (displayId, factor) in pendingScalingFactors {
            try await store.editScalingFactorAsync(
                wallpaperId: options.wallpaperId,
                displayId: displayId,
                factor: factor
            )
        }
        clearPendingDisplayEdits()
    }

    private func performAsyncBridgeAction(_ action: @escaping () async throws -> Void) {
        guard !applyInProgress else {
            return
        }

        applyInProgress = true
        Task {
            do {
                try await action()
            } catch {
                onError(error)
            }
            applyInProgress = false
        }
    }
}
