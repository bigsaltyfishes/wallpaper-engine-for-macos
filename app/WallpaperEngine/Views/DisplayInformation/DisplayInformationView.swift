import SwiftUI

struct DisplayInformationView: View {
    @Environment(BridgeStore.self) private var store
    @State private var expandedDisplayIds = Set<String>()
    @State private var optionsByDisplayId: [String: BridgeWallpaperOptionsSnapshot] = [:]
    @State private var loadingDisplayIds = Set<String>()
    @State private var presentedError: BridgeErrorAlert?
    @State private var actionInProgress = false

    var body: some View {
        Group {
            if store.monitorInformationSnapshot.rows.isEmpty {
                ContentUnavailableView("No Active Wallpapers", systemImage: "display")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                Form {
                    Section("Active Wallpapers") {
                        ForEach(store.monitorInformationSnapshot.rows, id: \.displayId) { row in
                            DisclosureGroup(
                                isExpanded: binding(for: row.displayId)
                            ) {
                                activeWallpaperSettings(for: row)
                            } label: {
                                activeWallpaperHeader(for: row)
                            }
                        }
                    }
                }
                .formStyle(.grouped)
            }
        }
        .navigationTitle("Display")
        .alert(item: $presentedError) { error in
            Alert(
                title: Text("Bridge Error"),
                message: Text(error.message),
                dismissButton: .default(Text("OK"))
            )
        }
        .onChange(of: store.snapshotRevision) { _, _ in
            pruneCachedOptions()
        }
    }

    @ViewBuilder
    private func activeWallpaperSettings(for row: BridgeMonitorInfoRow) -> some View {
        if loadingDisplayIds.contains(row.displayId) {
            ProgressView()
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.vertical, 12)
        } else if let options = optionsByDisplayId[row.displayId] {
            WallpaperOptionsEditorView(
                options: options,
                displayIdFilter: row.displayId,
                displayRowsAreCollapsible: false,
                showsTitle: false,
                showsActions: true,
                scrollsContent: false,
                onError: presentError
            )
            .padding(14)
            .background {
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color.secondary.opacity(0.08))
            }
            .padding(.vertical, 8)
        } else {
            Text("Wallpaper settings could not be loaded.")
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.vertical, 10)
        }
    }

    private func activeWallpaperHeader(for row: BridgeMonitorInfoRow) -> some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 3) {
                Text(row.title)
                    .font(.headline)
                    .lineLimit(2)
                Text(row.wallpaperTitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer()

            Button {
                eject(row)
            } label: {
                Label("Eject", systemImage: "eject")
            }
            .disabled(actionInProgress)
        }
    }

    private func binding(for displayId: String) -> Binding<Bool> {
        Binding {
            expandedDisplayIds.contains(displayId)
        } set: { expanded in
            if expanded {
                expandedDisplayIds.insert(displayId)
                loadOptionsIfNeeded(for: displayId)
            } else {
                expandedDisplayIds.remove(displayId)
            }
        }
    }

    private func loadOptionsIfNeeded(for displayId: String) {
        guard optionsByDisplayId[displayId] == nil,
              !loadingDisplayIds.contains(displayId),
              let row = store.monitorInformationSnapshot.rows.first(where: { $0.displayId == displayId })
        else {
            return
        }

        loadingDisplayIds.insert(displayId)
        Task {
            do {
                let options = try await store.wallpaperOptionsSnapshotAsync(wallpaperId: row.wallpaperId)
                optionsByDisplayId[displayId] = options
                presentedError = nil
            } catch {
                presentError(error)
            }
            loadingDisplayIds.remove(displayId)
        }
    }

    private func eject(_ row: BridgeMonitorInfoRow) {
        guard !actionInProgress else {
            return
        }

        actionInProgress = true
        Task {
            do {
                try await store.ejectWallpaperFromDisplayAsync(
                    displayId: row.displayId,
                    wallpaperId: row.wallpaperId
                )
                expandedDisplayIds.remove(row.displayId)
                optionsByDisplayId[row.displayId] = nil
                presentedError = nil
            } catch {
                presentError(error)
            }
            actionInProgress = false
        }
    }

    private func pruneCachedOptions() {
        let activeDisplayIds = Set(store.monitorInformationSnapshot.rows.map(\.displayId))
        expandedDisplayIds.formIntersection(activeDisplayIds)
        optionsByDisplayId = optionsByDisplayId.filter { activeDisplayIds.contains($0.key) }
        loadingDisplayIds.formIntersection(activeDisplayIds)
    }

    private func presentError(_ error: Error) {
        presentedError = BridgeErrorAlert(error: error)
    }
}

private struct BridgeErrorAlert: Identifiable {
    let id = UUID()
    let message: String

    init(error: Error) {
        self.message = error.localizedDescription
    }
}
