import Foundation
import Observation

@MainActor
@Observable
final class BridgeStore {
    let bridge: WallpaperBridge
    var appSnapshot: BridgeAppSnapshot
    var librarySnapshot: BridgeLibrarySnapshot
    var wallpaperOptionsSnapshot: BridgeWallpaperOptionsSnapshot?
    var monitorInformationSnapshot: BridgeMonitorInformationSnapshot
    var settingsSnapshot: BridgeSettingsSnapshot
    var snapshotRevision: UInt64
    var latestBridgeErrorMessage: String?
    var latestBridgeErrorRevision: UInt64

    convenience init() throws {
        self.init(bridge: try WallpaperBridge())
    }

    init(bridge: WallpaperBridge) {
        let snapshots = Self.emptySnapshots()

        self.bridge = bridge
        self.appSnapshot = snapshots.app
        self.librarySnapshot = snapshots.library
        self.wallpaperOptionsSnapshot = snapshots.wallpaperOptions
        self.monitorInformationSnapshot = snapshots.monitorInformation
        self.settingsSnapshot = snapshots.settings
        self.snapshotRevision = 0
        self.latestBridgeErrorMessage = nil
        self.latestBridgeErrorRevision = 0
    }

    func refreshAllAsync() async throws {
        let bundle = try await bridge.allSnapshots()
        apply(bundle)
    }

    func bootstrapAsync() async throws {
        let bundle = try await bridge.bootstrap()
        apply(bundle)
    }

    func refreshLibraryAsync() async throws {
        let bundle = try await bridge.refreshLibrary()
        apply(bundle)
    }

    func refreshDisplaysAsync() async throws {
        let bundle = try await bridge.refreshDisplays()
        apply(bundle)
    }

    func selectWallpaperAsync(id: String) async throws {
        let bundle = try await bridge.selectWallpaper(id: id)
        apply(bundle)
    }

    func wallpaperOptionsSnapshotAsync(
        wallpaperId: String
    ) async throws -> BridgeWallpaperOptionsSnapshot {
        try await bridge.wallpaperOptionsSnapshot(wallpaperId: wallpaperId)
    }

    func setFilterAsync(kind: BridgeWallpaperKind, enabled: Bool) async throws {
        let bundle = try await bridge.setFilter(kind: kind, enabled: enabled)
        apply(bundle)
    }

    func setVolumeAsync(wallpaperId: String, volume: Float) async throws {
        let bundle = try await bridge.setVolume(wallpaperId: wallpaperId, volume: volume)
        apply(bundle)
    }

    func setMutedAsync(wallpaperId: String, muted: Bool) async throws {
        let bundle = try await bridge.setMuted(wallpaperId: wallpaperId, muted: muted)
        apply(bundle)
    }

    func setAudioResponseEnabledAsync(wallpaperId: String, enabled: Bool) async throws {
        let bundle = try await bridge.setAudioResponseEnabled(wallpaperId: wallpaperId, enabled: enabled)
        apply(bundle)
    }

    func setDisplayConfigEnabledAsync(
        wallpaperId: String,
        displayId: String,
        enabled: Bool
    ) async throws {
        let bundle = try await bridge.setDisplayConfigEnabled(
            wallpaperId: wallpaperId,
            displayId: displayId,
            enabled: enabled
        )
        apply(bundle)
    }

    func setScalingModeAsync(
        wallpaperId: String,
        displayId: String,
        mode: BridgeScalingMode
    ) async throws {
        let bundle = try await bridge.setScalingMode(
            wallpaperId: wallpaperId,
            displayId: displayId,
            mode: mode
        )
        apply(bundle)
    }

    func editScalingFactorAsync(wallpaperId: String, displayId: String, factor: Double) async throws {
        let bundle = try await bridge.editScalingFactor(wallpaperId: wallpaperId, displayId: displayId, factor: factor)
        apply(bundle)
    }

    func setTargetFpsAsync(wallpaperId: String, displayId: String, fps: UInt32) async throws {
        let bundle = try await bridge.setTargetFps(wallpaperId: wallpaperId, displayId: displayId, fps: fps)
        apply(bundle)
    }

    func editPropertyAsync(
        wallpaperId: String,
        propertyId: String,
        value: BridgePropertyValue
    ) async throws {
        let bundle = try await bridge.editProperty(wallpaperId: wallpaperId, propertyId: propertyId, value: value)
        apply(bundle)
    }

    func restorePropertyDefaultAsync(wallpaperId: String, propertyId: String) async throws {
        let bundle = try await bridge.restorePropertyDefault(wallpaperId: wallpaperId, propertyId: propertyId)
        apply(bundle)
    }

    func setDisplayEnabledAsync(displayId: String, enabled: Bool) async throws {
        let bundle = try await bridge.setDisplayEnabled(displayId: displayId, enabled: enabled)
        apply(bundle)
    }

    func setDisplayModeAsync(displayId: String, mode: BridgeDisplayMode) async throws {
        let bundle = try await bridge.setDisplayMode(displayId: displayId, mode: mode)
        apply(bundle)
    }

    func setDisplayHorizontalFlipAsync(displayId: String, enabled: Bool) async throws {
        let bundle = try await bridge.setDisplayHorizontalFlip(displayId: displayId, enabled: enabled)
        apply(bundle)
    }

    func setMirrorTargetAsync(displayId: String, targetDisplayId: String) async throws {
        let bundle = try await bridge.setMirrorTarget(displayId: displayId, targetDisplayId: targetDisplayId)
        apply(bundle)
    }

    func setMirrorScalingModeAsync(displayId: String, mode: BridgeScalingMode) async throws {
        let bundle = try await bridge.setMirrorScalingMode(displayId: displayId, mode: mode)
        apply(bundle)
    }

    func setMirrorScalingFactorAsync(displayId: String, factor: Double) async throws {
        let bundle = try await bridge.setMirrorScalingFactor(displayId: displayId, factor: factor)
        apply(bundle)
    }

    func setMirrorTargetFpsAsync(displayId: String, fps: UInt32) async throws {
        let bundle = try await bridge.setMirrorTargetFps(displayId: displayId, fps: fps)
        apply(bundle)
    }

    func setMirrorVolumeAsync(displayId: String, volume: Float) async throws {
        let bundle = try await bridge.setMirrorVolume(displayId: displayId, volume: volume)
        apply(bundle)
    }

    func setMirrorMutedAsync(displayId: String, muted: Bool) async throws {
        let bundle = try await bridge.setMirrorMuted(displayId: displayId, muted: muted)
        apply(bundle)
    }

    func setLaunchAtLoginAsync(enabled: Bool) async throws {
        let bundle = try await bridge.setLaunchAtLogin(enabled: enabled)
        apply(bundle)
    }

    func setPauseOnBatteryPowerAsync(enabled: Bool) async throws {
        let bundle = try await bridge.setPauseOnBatteryPower(enabled: enabled)
        apply(bundle)
    }

    func setWorkshopDirAsync(dir: String) async throws {
        let bundle = try await bridge.setWorkshopDir(dir: dir)
        apply(bundle)
    }

    func setAssetsDirAsync(dir: String) async throws {
        let bundle = try await bridge.setAssetsDir(dir: dir)
        apply(bundle)
    }

    func applyWallpaperOptionsAsync(wallpaperId: String) async throws {
        let bundle = try await bridge.applyWallpaperOptions(wallpaperId: wallpaperId)
        apply(bundle)
    }

    func cancelWallpaperOptionsAsync(wallpaperId: String) async throws {
        let bundle = try await bridge.cancelWallpaperOptions(wallpaperId: wallpaperId)
        apply(bundle)
    }

    func pauseAllAsync() async throws {
        let bundle = try await bridge.pauseAll()
        apply(bundle)
    }

    func playAllAsync() async throws {
        let bundle = try await bridge.playAll()
        apply(bundle)
    }

    func ejectWallpaperFromDisplayAsync(
        displayId: String,
        wallpaperId: String
    ) async throws {
        let bundle = try await bridge.ejectWallpaperFromDisplay(displayId: displayId, wallpaperId: wallpaperId)
        apply(bundle)
    }

    func shutdownAsync() async throws {
        try await bridge.shutdown()
    }

    func clearShaderCacheAsync() async throws {
        settingsSnapshot = try await bridge.clearShaderCache()
        finishSnapshotApply()
    }

    func clearLogsAsync() throws {
        let status = try bridge.clearLogs()
        settingsSnapshot.storage = BridgeStorageStatus(
            shaderCacheSizeBytes: settingsSnapshot.storage.shaderCacheSizeBytes,
            logs: status
        )
        finishSnapshotApply()
    }

    func logFolderURL() throws -> URL {
        URL(fileURLWithPath: try bridge.logFolderPath(), isDirectory: true)
    }

    func emitLog(level: BridgeLogLevel, file: String, line: UInt32, message: String) throws {
        try bridge.emitGuiLog(level: level, file: file, line: line, message: message)
    }

    private struct Snapshots {
        let app: BridgeAppSnapshot
        let library: BridgeLibrarySnapshot
        let wallpaperOptions: BridgeWallpaperOptionsSnapshot?
        let monitorInformation: BridgeMonitorInformationSnapshot
        let settings: BridgeSettingsSnapshot
    }

    private static func emptySnapshots() -> Snapshots {
        Snapshots(
            app: BridgeAppSnapshot(
                playbackState: .paused,
                selectedWallpaperId: nil,
                activeWallpaperIds: [],
                errors: []
            ),
            library: BridgeLibrarySnapshot(
                wallpapers: [],
                scanStatus: BridgeLibraryScanStatus(scanning: false, done: 0, total: 0),
                sceneCount: 0,
                videoCount: 0,
                webpageCount: 0,
                unknownCount: 0
            ),
            wallpaperOptions: nil,
            monitorInformation: BridgeMonitorInformationSnapshot(rows: []),
            settings: BridgeSettingsSnapshot(
                displays: [],
                launchAtLoginAvailable: false,
                launchAtLoginEnabled: false,
                pauseOnBatteryPower: false,
                gitSha: "",
                bridgeVersion: "",
                coreVersion: "",
                shaderPipelineVersion: "",
                storage: BridgeStorageStatus(
                    shaderCacheSizeBytes: 0,
                    logs: BridgeLogStatus(
                        logsRoot: "",
                        activeSession: "",
                        activeFile: "",
                        activeFileSizeBytes: 0
                    )
                ),
                workshopDir: "",
                assetsDir: ""
            )
        )
    }

    private func apply(_ bundle: BridgeSnapshotBundle) {
        self.appSnapshot = bundle.app
        self.librarySnapshot = bundle.library
        self.wallpaperOptionsSnapshot = bundle.wallpaperOptions
        self.monitorInformationSnapshot = bundle.monitorInformation
        self.settingsSnapshot = bundle.settings
        finishSnapshotApply()
    }

    private func apply(_ bundle: BridgeWallpaperMutationBundle) {
        self.appSnapshot = bundle.app
        self.librarySnapshot = bundle.library
        self.wallpaperOptionsSnapshot = bundle.wallpaperOptions
        self.monitorInformationSnapshot = bundle.monitorInformation
        self.settingsSnapshot = bundle.settings
        finishSnapshotApply()
    }

    private func apply(_ bundle: BridgeDisplayMutationBundle) {
        self.appSnapshot = bundle.app
        self.librarySnapshot = bundle.library
        self.monitorInformationSnapshot = bundle.monitorInformation
        self.settingsSnapshot = bundle.settings
        finishSnapshotApply()
    }

    private func finishSnapshotApply() {
        self.snapshotRevision &+= 1
        if let message = appSnapshot.errors.last,
           message != latestBridgeErrorMessage {
            latestBridgeErrorMessage = message
            latestBridgeErrorRevision &+= 1
        }
    }
}
