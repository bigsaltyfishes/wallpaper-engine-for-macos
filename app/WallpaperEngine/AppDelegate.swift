import AppKit
import SwiftUI

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate, NSWindowDelegate {
    private var statusItem: NSStatusItem?
    private var controlPanelWindow: NSWindow?
    private let controlPanelNavigation = ControlPanelNavigation()
    private var displayChangeObserver: NSObjectProtocol?
    private var store: BridgeStore?
    private var startupError: Error?
    private var lastError: Error?
    private var playbackSnapshotCurrent = false
    private var shutdownInProgress = false
    private var shutdownComplete = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        logStartup("didFinishLaunching start")
        BridgeEnvironment.configureVulkanICDIfNeeded()
        logStartup("vulkan icd configured")
        do {
            store = try BridgeStore()
            AppLog.store = store
            startupError = nil
            playbackSnapshotCurrent = false
            logStartup("BridgeStore created")
        } catch {
            logStartup("BridgeStore FAILED: \(error.localizedDescription)")
            startupError = error
            playbackSnapshotCurrent = false
        }

        NSApp.setActivationPolicy(.accessory)
        logStartup("activation policy set to accessory")
        installStatusItem()
        synchronizeStatusItem()
        installDisplayChangeObserver()
        logStartup("display change observer installed")
        redirectApplicationSettingsMenu()
        logStartup("settings menu redirect dispatched")
        bootstrapStore()
        logStartup("bootstrap dispatched")
    }

    func applicationWillTerminate(_ notification: Notification) {
        if let displayChangeObserver {
            NotificationCenter.default.removeObserver(displayChangeObserver)
            self.displayChangeObserver = nil
        }
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard !shutdownComplete else {
            return .terminateNow
        }
        guard !shutdownInProgress else {
            return .terminateCancel
        }

        shutdownInProgress = true
        controlPanelWindow?.orderOut(nil)
        NSApp.setActivationPolicy(.accessory)

        Task {
            do {
                try await store?.shutdownAsync()
                lastError = nil
            } catch {
                lastError = error
            }

            shutdownInProgress = false
            shutdownComplete = true
            sender.reply(toApplicationShouldTerminate: true)
        }

        return .terminateLater
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        showControlPanel(selection: .wallpaper)
        return false
    }

    func menuWillOpen(_ menu: NSMenu) {
        refreshStoreSnapshot()
        rebuildMenu(menu)
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard sender === controlPanelWindow else {
            return true
        }
        guard !shutdownInProgress && !shutdownComplete else {
            return true
        }

        sender.orderOut(nil)
        NSApp.setActivationPolicy(.accessory)
        rebuildMenu()
        return false
    }

    func windowWillClose(_ notification: Notification) {
        guard let window = notification.object as? NSWindow,
              window === controlPanelWindow
        else {
            return
        }

        controlPanelWindow = nil
        NSApp.setActivationPolicy(.accessory)
    }

    /// Emits a startup-diagnostic line straight to stderr so it is visible when
    /// the app is launched from a terminal. Deliberately bypasses `AppLog`,
    /// whose `guard let store` blind spot silently drops every message until
    /// `BridgeStore` is constructed, and which otherwise writes only to the
    /// Rust file channel rather than stderr.
    private func logStartup(_ message: String) {
        fputs("[WE] \(message)\n", stderr)
    }

    private func installStatusItem() {
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        statusItem = item

        let button = item.button
        let trayIcon = NSImage(named: "TrayIcon")
        if let button {
            button.image = trayIcon
                ?? NSImage(systemSymbolName: "play.rectangle", accessibilityDescription: "Wallpaper Engine")
            button.image?.isTemplate = true
        }

        logStartup("statusItem installed: button=\(button != nil) trayIconAssetResolved=\(trayIcon != nil)")

        let menu = NSMenu()
        menu.delegate = self
        item.menu = menu
        rebuildMenu(menu)
    }

    /// Work around an AppKit/SwiftUI timing issue: when launched via
    /// LaunchServices (Finder double-click) in `.accessory` activation
    /// policy, a status item created in `applicationDidFinishLaunching`
    /// may be created correctly but never rendered because the SwiftUI
    /// Scene phase hasn't stabilized yet. Deferring by one runloop turn
    /// gives the Scene phase time to settle. Re-asserting the activation
    /// policy afterwards forces AppKit to re-register accessory-mode
    /// status items with the Window Server; rebuilding the menu alone
    /// only mutates `NSMenu` items and does not touch the status item's
    /// backing window, so it is insufficient by itself.
    private func synchronizeStatusItem() {
        DispatchQueue.main.async { [weak self] in
            guard let self, self.statusItem != nil else { return }
            // Re-assert the activation policy to force AppKit to
            // re-register the accessory-mode status item with the
            // Window Server after the SwiftUI Scene phase has settled.
            NSApp.setActivationPolicy(.accessory)
            self.rebuildMenu()
        }
    }

    private func installDisplayChangeObserver() {
        displayChangeObserver = NotificationCenter.default.addObserver(
            forName: NSApplication.didChangeScreenParametersNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            Task { @MainActor in
                self?.refreshDisplaysFromSystemEvent()
            }
        }
    }

    private func redirectApplicationSettingsMenu() {
        DispatchQueue.main.async { [weak self] in
            guard let self,
                  let item = NSApp.mainMenu?.items.first?.submenu?.items.first(where: { item in
                      item.keyEquivalent == ","
                  })
            else {
                return
            }

            item.target = self
            item.action = #selector(openSettings)
        }
    }

    private func rebuildMenu(_ menu: NSMenu? = nil) {
        guard let menu = menu ?? statusItem?.menu else {
            return
        }

        menu.removeAllItems()
        menu.addItem(menuItem(titleKey: "Control Panel", action: #selector(openControlPanel)))

        if let store,
           playbackSnapshotCurrent,
           !store.appSnapshot.activeWallpaperIds.isEmpty
        {
            menu.addItem(.separator())
            let playbackTitleKey = store.appSnapshot.playbackState == .paused ? "Play" : "Pause"
            let playbackItem = menuItem(titleKey: playbackTitleKey, action: #selector(togglePlayback))
            playbackItem.isEnabled = true
            menu.addItem(playbackItem)
        }

        if let error = startupError ?? lastError {
            menu.addItem(.separator())
            menu.addItem(disabledMenuItem(error.localizedDescription))
        }

        menu.addItem(.separator())
        menu.addItem(menuItem(titleKey: "Exit", action: #selector(exitApplication)))
    }

    private func menuItem(titleKey: String, action: Selector) -> NSMenuItem {
        let item = NSMenuItem(
            title: NSLocalizedString(titleKey, comment: ""),
            action: action,
            keyEquivalent: ""
        )
        item.target = self
        return item
    }

    private func disabledMenuItem(_ title: String) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.isEnabled = false
        return item
    }

    @objc private func openControlPanel() {
        showControlPanel(selection: .wallpaper)
    }

    @objc func openSettings() {
        showControlPanel(selection: .settings)
    }

    private func showControlPanel(selection: SidebarSelection) {
        controlPanelNavigation.selection = selection
        NSApp.setActivationPolicy(.regular)

        if let controlPanelWindow {
            controlPanelWindow.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            return
        }

        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 900, height: 620),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.title = NSLocalizedString("Control Panel", comment: "")
        window.titleVisibility = .visible
        window.titlebarAppearsTransparent = false
        window.toolbarStyle = .unified
        window.center()
        window.delegate = self
        window.isReleasedWhenClosed = false
        if let store {
            window.contentViewController = NSHostingController(
                rootView: AnyView(
                    ControlPanelView(
                        store: store,
                        navigation: controlPanelNavigation
                    )
                )
            )
        } else {
            window.contentViewController = NSHostingController(
                rootView: AnyView(BridgeUnavailableView(error: startupError ?? lastError))
            )
        }

        controlPanelWindow = window
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    @objc private func togglePlayback() {
        guard let store,
              !shutdownInProgress,
              !shutdownComplete,
              playbackSnapshotCurrent,
              !store.appSnapshot.activeWallpaperIds.isEmpty
        else {
            return
        }

        Task {
            do {
                if store.appSnapshot.playbackState == .paused {
                    try await store.playAllAsync()
                } else {
                    try await store.pauseAllAsync()
                }
                lastError = nil
                playbackSnapshotCurrent = true
                rebuildMenu()
            } catch {
                lastError = error
                playbackSnapshotCurrent = false
                rebuildMenu()
                NSAlert(error: error).runModal()
            }
        }
    }

    @objc private func exitApplication() {
        NSApp.terminate(nil)
    }

    private func bootstrapStore() {
        logStartup("bootstrapAsync start")
        guard let store else {
            logStartup("bootstrapAsync skipped: store is nil")
            return
        }

        Task {
            do {
                try await store.bootstrapAsync()
                startupError = nil
                lastError = nil
                playbackSnapshotCurrent = true
                logStartup("bootstrapAsync completed successfully")
            } catch {
                logStartup("bootstrapAsync FAILED: \(error.localizedDescription)")
                lastError = error
                playbackSnapshotCurrent = false
            }
            rebuildMenu()
        }
    }

    private func refreshStoreSnapshot() {
        guard let store,
              !shutdownInProgress,
              !shutdownComplete
        else {
            return
        }

        Task {
            do {
                try await store.refreshAllAsync()
                lastError = nil
                playbackSnapshotCurrent = true
            } catch {
                lastError = error
                playbackSnapshotCurrent = false
            }
            rebuildMenu()
        }
    }

    private func refreshDisplaysFromSystemEvent() {
        guard let store,
              !shutdownInProgress,
              !shutdownComplete
        else {
            return
        }

        Task {
            do {
                try await store.refreshDisplaysAsync()
                lastError = nil
                playbackSnapshotCurrent = true
            } catch {
                lastError = error
                playbackSnapshotCurrent = false
            }
            rebuildMenu()
        }
    }

}

private struct BridgeUnavailableView: View {
    let error: Error?

    var body: some View {
        ContentUnavailableView(
            "Bridge Unavailable",
            systemImage: "exclamationmark.triangle",
            description: Text(error?.localizedDescription ?? "The rendering bridge could not be started.")
        )
        .frame(minWidth: 640, minHeight: 420)
    }
}
