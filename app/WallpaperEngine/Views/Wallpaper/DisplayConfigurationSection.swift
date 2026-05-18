import SwiftUI

struct DisplayConfigurationSection: View {
    let options: BridgeWallpaperOptionsSnapshot
    let snapshotRevision: UInt64
    let resetRevision: UInt64
    var displayIdFilter: String?
    var rowsAreCollapsible = true
    @Binding var pendingScalingFactors: [String: Double]
    @Binding var invalidScalingFactorDisplayIds: Set<String>
    @Binding var activeDisplayBridgeActionIds: Set<String>
    var onError: (Error) -> Void = { _ in }

    private var rows: [BridgeDisplayConfigRow] {
        if let displayIdFilter {
            return options.displayConfigurations.filter { $0.displayId == displayIdFilter }
        }
        return options.displayConfigurations
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if rows.isEmpty {
                Text("No display configuration loaded.")
                    .foregroundStyle(.secondary)
                    .padding(.top, 8)
            } else {
                ForEach(rows, id: \.displayId) { row in
                    DisplayConfigurationRow(
                        wallpaperId: options.wallpaperId,
                        row: row,
                        snapshotRevision: snapshotRevision,
                        resetRevision: resetRevision,
                        collapsible: rowsAreCollapsible,
                        pendingScalingFactors: $pendingScalingFactors,
                        invalidScalingFactorDisplayIds: $invalidScalingFactorDisplayIds,
                        activeDisplayBridgeActionIds: $activeDisplayBridgeActionIds,
                        onError: onError
                    )
                    .id("\(options.wallpaperId)-\(row.displayId)")
                }
            }
        }
        .padding(.top, 8)
    }
}

private struct DisplayConfigurationRow: View {
    @Environment(BridgeStore.self) private var store

    let wallpaperId: String
    let row: BridgeDisplayConfigRow
    let snapshotRevision: UInt64
    let resetRevision: UInt64
    let collapsible: Bool
    @Binding var pendingScalingFactors: [String: Double]
    @Binding var invalidScalingFactorDisplayIds: Set<String>
    @Binding var activeDisplayBridgeActionIds: Set<String>
    let onError: (Error) -> Void
    @State private var enabled: Bool
    @State private var scalingMode: BridgeScalingMode
    @State private var scalingFactorDraft: String
    @State private var expanded: Bool
    @State private var targetFps: Double
    @State private var bridgeActionInProgress = false
    @FocusState private var scalingFactorFocused: Bool

    init(
        wallpaperId: String,
        row: BridgeDisplayConfigRow,
        snapshotRevision: UInt64,
        resetRevision: UInt64,
        collapsible: Bool,
        pendingScalingFactors: Binding<[String: Double]>,
        invalidScalingFactorDisplayIds: Binding<Set<String>>,
        activeDisplayBridgeActionIds: Binding<Set<String>>,
        onError: @escaping (Error) -> Void
    ) {
        self.wallpaperId = wallpaperId
        self.row = row
        self.snapshotRevision = snapshotRevision
        self.resetRevision = resetRevision
        self.collapsible = collapsible
        _pendingScalingFactors = pendingScalingFactors
        _invalidScalingFactorDisplayIds = invalidScalingFactorDisplayIds
        _activeDisplayBridgeActionIds = activeDisplayBridgeActionIds
        self.onError = onError
        _enabled = State(initialValue: row.enabled)
        _scalingMode = State(initialValue: row.scalingMode)
        _scalingFactorDraft = State(initialValue: Self.formattedScalingFactor(row.scalingFactor))
        _expanded = State(initialValue: !collapsible)
        _targetFps = State(initialValue: Double(Self.clampedTargetFps(row)))
    }

    var body: some View {
        Group {
            if collapsible {
                DisclosureGroup(isExpanded: $expanded) {
                    controls
                        .padding(.top, 8)
                } label: {
                    header
                }
            } else {
                VStack(alignment: .leading, spacing: 12) {
                    header
                    controls
                }
            }
        }
        .padding(12)
        .background {
            RoundedRectangle(cornerRadius: 8)
                .fill(Color.secondary.opacity(0.08))
        }
        .onChange(of: row) { _, updatedRow in
            reset(from: updatedRow)
        }
        .onChange(of: wallpaperId) { _, _ in
            reset(from: row)
        }
        .onChange(of: snapshotRevision) { _, _ in
            reset(from: row)
        }
        .onChange(of: resetRevision) { _, _ in
            reset(from: row)
        }
    }

    private var header: some View {
        HStack {
            Text(row.title)
                .font(.headline)
            if row.dirty {
                Text("Modified")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var controls: some View {
        VStack(alignment: .leading, spacing: 12) {
            Toggle("Enable", isOn: Binding {
                enabled
            } set: { isEnabled in
                performAsyncBridgeAction {
                    try await store.setDisplayConfigEnabledAsync(
                        wallpaperId: wallpaperId,
                        displayId: row.displayId,
                        enabled: isEnabled
                    )
                    enabled = isEnabled
                }
            })
            .toggleStyle(.switch)
            .disabled(bridgeActionInProgress)

            Picker("Scaling Mode", selection: Binding {
                scalingMode
            } set: { mode in
                performAsyncBridgeAction {
                    try await store.setScalingModeAsync(
                        wallpaperId: wallpaperId,
                        displayId: row.displayId,
                        mode: mode
                    )
                    scalingMode = mode
                }
            }) {
                Text("None").tag(BridgeScalingMode.none)
                Text("Stretch").tag(BridgeScalingMode.stretch)
                Text("Match").tag(BridgeScalingMode.match)
                Text("Fill").tag(BridgeScalingMode.fill)
            }
            .pickerStyle(.menu)
            .disabled(!enabled || bridgeActionInProgress)

            HStack {
                Text("Scaling Factor")
                Spacer()
                TextField("", text: $scalingFactorDraft)
                    .textFieldStyle(.roundedBorder)
                    .multilineTextAlignment(.trailing)
                    .monospacedDigit()
                    .frame(width: 72)
                    .focused($scalingFactorFocused)
                    .onChange(of: scalingFactorDraft) { _, value in
                        updatePendingScalingFactor(value)
                    }
                    .onSubmit(commitScalingFactor)
            }
            .disabled(!enabled || bridgeActionInProgress)

            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    Text("Target Frame Rate")
                    Spacer()
                    EditableNumberField(
                        value: UInt32(targetFps.rounded()),
                        range: 0...row.maxFps
                    ) { editedValue in
                        setTargetFps(editedValue)
                    }
                }

                Slider(
                    value: Binding {
                        targetFps
                    } set: { value in
                        targetFps = value
                    },
                    in: 0...Double(row.maxFps),
                    step: 1,
                    onEditingChanged: { editing in
                        if !editing {
                            setTargetFps(UInt32(targetFps.rounded()))
                        }
                    }
                )
                .disabled(!enabled || bridgeActionInProgress)
            }
            .disabled(!enabled || bridgeActionInProgress)

            if row.canRestoreDefaults {
                Button("Restore Defaults") {}
                    .buttonStyle(.link)
                    .disabled(true)
            }
        }
    }

    private func reset(from row: BridgeDisplayConfigRow) {
        enabled = row.enabled
        scalingMode = row.scalingMode
        scalingFactorDraft = Self.formattedScalingFactor(row.scalingFactor)
        pendingScalingFactors.removeValue(forKey: row.displayId)
        invalidScalingFactorDisplayIds.remove(row.displayId)
        targetFps = Double(Self.clampedTargetFps(row))
    }

    private static func formattedScalingFactor(_ factor: Double) -> String {
        factor.formatted(.number.precision(.fractionLength(1...3)))
    }

    private static func clampedTargetFps(_ row: BridgeDisplayConfigRow) -> UInt32 {
        min(row.targetFps, row.maxFps)
    }

    private func commitScalingFactor() {
        guard let factor = pendingScalingFactors[row.displayId] else {
            if invalidScalingFactorDisplayIds.contains(row.displayId) {
                scalingFactorDraft = Self.formattedScalingFactor(row.scalingFactor)
                invalidScalingFactorDisplayIds.remove(row.displayId)
                onError(ScalingFactorValidationError())
            }
            return
        }

        guard factor.isFinite, factor > 0 else {
            scalingFactorDraft = Self.formattedScalingFactor(row.scalingFactor)
            pendingScalingFactors.removeValue(forKey: row.displayId)
            invalidScalingFactorDisplayIds.remove(row.displayId)
            onError(ScalingFactorValidationError())
            return
        }

        performAsyncBridgeAction {
            try await store.editScalingFactorAsync(
                wallpaperId: wallpaperId,
                displayId: row.displayId,
                factor: factor
            )
            try await store.applyWallpaperOptionsAsync(wallpaperId: wallpaperId)
            scalingFactorDraft = Self.formattedScalingFactor(factor)
            pendingScalingFactors.removeValue(forKey: row.displayId)
            invalidScalingFactorDisplayIds.remove(row.displayId)
            scalingFactorFocused = false
        }
    }

    private func updatePendingScalingFactor(_ value: String) {
        let committed = Self.formattedScalingFactor(row.scalingFactor)
        guard value != committed else {
            pendingScalingFactors.removeValue(forKey: row.displayId)
            invalidScalingFactorDisplayIds.remove(row.displayId)
            return
        }

        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        if let factor = Double(trimmed), factor.isFinite, factor > 0 {
            pendingScalingFactors[row.displayId] = factor
            invalidScalingFactorDisplayIds.remove(row.displayId)
        } else {
            pendingScalingFactors.removeValue(forKey: row.displayId)
            invalidScalingFactorDisplayIds.insert(row.displayId)
        }
    }

    private func setTargetFps(_ fps: UInt32) {
        performAsyncBridgeAction {
            try await store.setTargetFpsAsync(
                wallpaperId: wallpaperId,
                displayId: row.displayId,
                fps: fps
            )
            targetFps = Double(fps)
        }
    }

    private func performAsyncBridgeAction(_ action: @escaping () async throws -> Void) {
        guard !bridgeActionInProgress else {
            return
        }

        bridgeActionInProgress = true
        activeDisplayBridgeActionIds.insert(row.displayId)
        Task {
            do {
                try await action()
            } catch {
                onError(error)
            }
            bridgeActionInProgress = false
            activeDisplayBridgeActionIds.remove(row.displayId)
        }
    }
}

private struct ScalingFactorValidationError: LocalizedError {
    var errorDescription: String? {
        "Scaling factor must be greater than 0."
    }
}
