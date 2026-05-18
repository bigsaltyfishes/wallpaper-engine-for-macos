import SwiftUI

struct WallpaperInspectorView: View {
    @Environment(BridgeStore.self) private var store
    @State private var presentedError: BridgeErrorAlert?

    var body: some View {
        Group {
            if let options = store.wallpaperOptionsSnapshot {
                WallpaperOptionsEditorView(options: options, onError: presentError)
                    .navigationTitle("Wallpaper Options")
                    .navigationSplitViewColumnWidth(min: 360, ideal: 420)
            } else {
                ContentUnavailableView("No Wallpaper Selected", systemImage: "sidebar.right")
                    .navigationTitle("Wallpaper Options")
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
