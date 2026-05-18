import AppKit
import SwiftUI

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate, NSWindowDelegate {
    private var statusItem: NSStatusItem?
    private var controlPanelWindow: NSWindow?
    private var displayChangeObserver: NSObjectProtocol?
    private var store: BridgeStore?
    private var startupError: Error?
    private var lastError: Error?
    private var playbackSnapshotCurrent = false
    private var shutdownInProgress = false
    private var shutdownComplete = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        BridgeEnvironment.configureVulkanICDIfNeeded()
        do {
            store = try BridgeStore()
            startupError = nil
            playbackSnapshotCurrent = false
        } catch {
            startupError = error
            playbackSnapshotCurrent = false
        }

        NSApp.setActivationPolicy(.accessory)
        installStatusItem()
        installDisplayChangeObserver()
        bootstrapStore()
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

    private func installStatusItem() {
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        statusItem = item

        if let button = item.button {
            button.image = NSImage(named: "TrayIcon")
                ?? NSImage(systemSymbolName: "play.rectangle", accessibilityDescription: "Wallpaper Engine")
            button.image?.isTemplate = true
        }

        let menu = NSMenu()
        menu.delegate = self
        item.menu = menu
        rebuildMenu(menu)
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
            window.contentViewController = NSHostingController(rootView: AnyView(ControlPanelView(store: store)))
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
        guard let store else {
            return
        }

        Task {
            do {
                try await store.bootstrapAsync()
                startupError = nil
                lastError = nil
                playbackSnapshotCurrent = true
            } catch {
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
