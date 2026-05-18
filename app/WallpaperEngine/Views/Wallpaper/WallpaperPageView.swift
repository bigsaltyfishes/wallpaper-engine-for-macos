import SwiftUI

struct WallpaperPageView: View {
    @Environment(BridgeStore.self) private var store
    @State private var presentedError: BridgeErrorAlert?
    @State private var bridgeActionInProgress = false
    @State private var visibleKinds = WallpaperKindFilter.allCases.reduce(into: Set<WallpaperKindFilter>()) {
        $0.insert($1)
    }
    @State private var showActiveOnly = false
    @State private var searchText = ""

    private let columns = [
        GridItem(.adaptive(minimum: 220, maximum: 280), spacing: 20, alignment: .top)
    ]

    private var wallpapers: [BridgeWallpaperEntry] {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        return store.librarySnapshot.wallpapers.filter { wallpaper in
            visibleKinds.contains(WallpaperKindFilter(kind: wallpaper.kind))
                &&
            (!showActiveOnly || wallpaper.active)
                && (query.isEmpty || wallpaper.title.localizedStandardContains(query))
        }
    }

    var body: some View {
        Group {
            if wallpapers.isEmpty {
                ContentUnavailableView("No Wallpapers", systemImage: "photo.on.rectangle")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVGrid(columns: columns, alignment: .leading, spacing: 20) {
                        ForEach(wallpapers, id: \.id) { wallpaper in
                            WallpaperCardView(wallpaper: wallpaper)
                                .contentShape(Rectangle())
                                .onTapGesture {
                                    performAsyncBridgeAction {
                                        try await store.selectWallpaperAsync(id: wallpaper.id)
                                    }
                                }
                                .disabled(bridgeActionInProgress)
                        }
                    }
                    .padding(24)
                }
            }
        }
        .navigationTitle("Wallpaper")
        .searchable(text: $searchText, placement: .toolbar, prompt: Text("Search Wallpapers"))
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Button {
                    performAsyncBridgeAction {
                        try await store.refreshLibraryAsync()
                    }
                } label: {
                    Label("Refresh", systemImage: "arrow.clockwise")
                }
                .disabled(bridgeActionInProgress)

                Menu {
                    Toggle("Active Wallpapers", isOn: $showActiveOnly)
                    Divider()
                    ForEach(WallpaperKindFilter.allCases) { filter in
                        Toggle(filter.title, isOn: binding(for: filter))
                    }
                } label: {
                    Label("Filter", systemImage: "line.3.horizontal.decrease.circle")
                }
                .disabled(bridgeActionInProgress)
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

    private func binding(for filter: WallpaperKindFilter) -> Binding<Bool> {
        Binding {
            visibleKinds.contains(filter)
        } set: { isVisible in
            performAsyncBridgeAction {
                try await store.setFilterAsync(kind: filter.kind, enabled: isVisible)
                if isVisible {
                    visibleKinds.insert(filter)
                } else {
                    visibleKinds.remove(filter)
                }
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
}

private struct BridgeErrorAlert: Identifiable {
    let id = UUID()
    let message: String

    init(error: Error) {
        self.message = error.localizedDescription
    }
}

private enum WallpaperKindFilter: String, CaseIterable, Identifiable, Hashable {
    case projectScene
    case video
    case webpage
    case unknown

    var id: String { rawValue }

    init(kind: BridgeWallpaperKind) {
        switch kind {
        case .projectScene:
            self = .projectScene
        case .video:
            self = .video
        case .webpage:
            self = .webpage
        case .unknown:
            self = .unknown
        }
    }

    var title: LocalizedStringKey {
        switch self {
        case .projectScene: "Project Scene"
        case .video: "Video"
        case .webpage: "Webpage"
        case .unknown: "Unknown"
        }
    }

    var kind: BridgeWallpaperKind {
        switch self {
        case .projectScene: .projectScene
        case .video: .video
        case .webpage: .webpage
        case .unknown: .unknown
        }
    }
}
