import SwiftUI

struct GeneralConfigurationSection: View {
    @Environment(BridgeStore.self) private var store

    let options: BridgeWallpaperOptionsSnapshot
    let snapshotRevision: UInt64
    let resetRevision: UInt64
    var onError: (Error) -> Void = { _ in }
    @State private var audioResponseEnabled: Bool
    @State private var muted: Bool
    @State private var volume: Double
    @State private var bridgeActionInProgress = false

    init(
        options: BridgeWallpaperOptionsSnapshot,
        snapshotRevision: UInt64,
        resetRevision: UInt64,
        onError: @escaping (Error) -> Void = { _ in }
    ) {
        self.options = options
        self.snapshotRevision = snapshotRevision
        self.resetRevision = resetRevision
        self.onError = onError
        _audioResponseEnabled = State(initialValue: options.audioResponseEnabled)
        _muted = State(initialValue: options.muted)
        _volume = State(initialValue: Double(options.volume))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Toggle("Audio Response", isOn: Binding {
                audioResponseEnabled
            } set: { enabled in
                setAudioResponseEnabled(enabled)
            })
            .toggleStyle(.switch)
            .disabled(bridgeActionInProgress)

            VStack(alignment: .leading, spacing: 6) {
                Text("Volume")

                HStack {
                    Button {
                        setMuted(!muted)
                    } label: {
                        Label(muted ? "Unmute" : "Mute", systemImage: muted ? "speaker.slash" : "speaker.wave.2")
                    }
                    .labelStyle(.iconOnly)
                    .disabled(bridgeActionInProgress)

                    Slider(
                        value: Binding {
                            volume
                        } set: { value in
                            volume = value
                        },
                        in: 0...1,
                        onEditingChanged: { editing in
                            if !editing {
                                setVolume(Float(volume))
                            }
                        }
                    )
                    .disabled(muted || bridgeActionInProgress)
                    .opacity(muted ? 0.45 : 1.0)
                }
            }
        }
        .padding(.top, 8)
        .onChange(of: options) { _, updatedOptions in
            reset(from: updatedOptions)
        }
        .onChange(of: snapshotRevision) { _, _ in
            reset(from: options)
        }
        .onChange(of: resetRevision) { _, _ in
            reset(from: options)
        }
    }

    private func reset(from options: BridgeWallpaperOptionsSnapshot) {
        audioResponseEnabled = options.audioResponseEnabled
        muted = options.muted
        volume = Double(options.volume)
    }

    private func setMuted(_ muted: Bool) {
        performAsyncBridgeAction {
            try await store.setMutedAsync(wallpaperId: options.wallpaperId, muted: muted)
            self.muted = muted
        }
    }

    private func setVolume(_ volume: Float) {
        performAsyncBridgeAction {
            try await store.setVolumeAsync(wallpaperId: options.wallpaperId, volume: volume)
            self.volume = Double(volume)
        }
    }

    private func setAudioResponseEnabled(_ enabled: Bool) {
        performAsyncBridgeAction {
            try await store.setAudioResponseEnabledAsync(wallpaperId: options.wallpaperId, enabled: enabled)
            self.audioResponseEnabled = enabled
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
            } catch {
                onError(error)
            }
            bridgeActionInProgress = false
        }
    }
}
