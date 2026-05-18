import SwiftUI

enum SidebarSelection: String, CaseIterable, Identifiable {
    case wallpaper
    case display
    case settings

    var id: String { rawValue }

    var title: LocalizedStringKey {
        switch self {
        case .wallpaper: "Wallpaper"
        case .display: "Display"
        case .settings: "Settings"
        }
    }

    var systemImage: String {
        switch self {
        case .wallpaper: "photo.on.rectangle"
        case .display: "display.2"
        case .settings: "gearshape"
        }
    }
}

struct ControlPanelView: View {
    let store: BridgeStore
    @State private var selection: SidebarSelection? = .wallpaper
    @State private var presentedError: ControlPanelError?

    var body: some View {
        NavigationSplitView {
            List(SidebarSelection.allCases, selection: $selection) { item in
                Label(item.title, systemImage: item.systemImage)
                    .tag(item)
            }
            .navigationTitle("Wallpaper Engine")
            .listStyle(.sidebar)
            .navigationSplitViewColumnWidth(min: 220, ideal: 240, max: 280)
        } content: {
            switch selection {
            case .wallpaper, .none:
                WallpaperPageView()
            case .display:
                DisplayInformationView()
            case .settings:
                SettingsView()
            }
        } detail: {
            if selection == .wallpaper || selection == nil {
                WallpaperInspectorView()
            } else {
                EmptyView()
            }
        }
        .environment(store)
        .frame(minWidth: 980, minHeight: 640)
        .navigationSplitViewStyle(.balanced)
        .toolbarBackground(.visible, for: .windowToolbar)
        .toolbarBackground(.regularMaterial, for: .windowToolbar)
        .onChange(of: store.latestBridgeErrorRevision) { _, _ in
            guard let message = store.latestBridgeErrorMessage else {
                return
            }
            presentedError = ControlPanelError(message: message)
        }
        .task {
            do {
                try await store.refreshAllAsync()
            } catch {
                presentedError = ControlPanelError(error: error)
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
}

private struct ControlPanelError: Identifiable {
    let id = UUID()
    let message: String

    init(error: Error) {
        self.message = error.localizedDescription
    }

    init(message: String) {
        self.message = message
    }
}
