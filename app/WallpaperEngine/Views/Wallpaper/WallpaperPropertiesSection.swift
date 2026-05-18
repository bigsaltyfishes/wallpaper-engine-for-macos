import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct WallpaperPropertiesSection: View {
    let options: BridgeWallpaperOptionsSnapshot
    var onError: (Error) -> Void = { _ in }

    private var groups: [WallpaperPropertyGroup] {
        WallpaperPropertyGroup.groups(from: options.properties)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if options.properties.isEmpty {
                Text(options.supported ? "No editable properties loaded." : "This wallpaper type is not editable.")
                    .foregroundStyle(.secondary)
            } else {
                LazyVStack(alignment: .leading, spacing: 12) {
                    ForEach(groups) { group in
                        if group.isUngrouped {
                            ForEach(group.properties, id: \.id) { property in
                                WallpaperPropertyRow(
                                    wallpaperId: options.wallpaperId,
                                    property: property,
                                    onError: onError
                                )
                                .id("\(options.wallpaperId)-\(property.id)")
                            }
                        } else {
                            WallpaperPropertyDisclosureGroup(
                                wallpaperId: options.wallpaperId,
                                group: group,
                                onError: onError
                            )
                            .id("\(options.wallpaperId)-group-\(group.id)")
                        }
                    }
                }
            }
        }
        .padding(.top, 8)
    }
}

private struct WallpaperPropertyGroup: Identifiable {
    let id: String
    let titleHtml: String?
    let properties: [BridgePropertyDescriptor]

    var isUngrouped: Bool {
        titleHtml == nil
    }

    static func groups(from properties: [BridgePropertyDescriptor]) -> [Self] {
        var groups: [Self] = []
        var currentId = "ungrouped"
        var currentTitle: String?
        var currentProperties: [BridgePropertyDescriptor] = []
        var groupOrdinal = 0

        func flush() {
            guard !currentProperties.isEmpty else {
                return
            }
            groups.append(Self(
                id: currentId,
                titleHtml: currentTitle,
                properties: currentProperties
            ))
            currentProperties = []
        }

        for property in properties {
            if property.kind == .group {
                flush()
                groupOrdinal += 1
                currentId = "\(groupOrdinal)-\(property.id)"
                currentTitle = property.labelHtml
            } else {
                currentProperties.append(property)
            }
        }

        flush()
        return groups
    }
}

private struct WallpaperPropertyDisclosureGroup: View {
    let wallpaperId: String
    let group: WallpaperPropertyGroup
    let onError: (Error) -> Void
    @State private var expanded = false

    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            VStack(alignment: .leading, spacing: 12) {
                ForEach(group.properties, id: \.id) { property in
                    WallpaperPropertyRow(
                        wallpaperId: wallpaperId,
                        property: property,
                        onError: onError
                    )
                }
            }
            .padding(.top, 8)
        } label: {
            propertyGroupLabel
                .font(.headline)
        }
        .padding(12)
        .background {
            RoundedRectangle(cornerRadius: 8)
                .fill(Color.secondary.opacity(0.08))
        }
    }

    @ViewBuilder
    private var propertyGroupLabel: some View {
        if let titleHtml = group.titleHtml,
           !titleHtml.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        {
            RichTextLabel(html: RichTextLabel.sanitizedPropertyLabelHtml(titleHtml))
        } else {
            Text("Group")
        }
    }
}

private struct WallpaperPropertyRow: View {
    @Environment(BridgeStore.self) private var store

    let wallpaperId: String
    let property: BridgePropertyDescriptor
    let onError: (Error) -> Void
    @State private var boolValue: Bool
    @State private var numberValue: Double
    @State private var textValue: String
    @State private var colorValue: Color
    @State private var bridgeActionInProgress = false

    init(
        wallpaperId: String,
        property: BridgePropertyDescriptor,
        onError: @escaping (Error) -> Void
    ) {
        self.wallpaperId = wallpaperId
        self.property = property
        self.onError = onError
        _boolValue = State(initialValue: property.value.boolValue ?? false)
        _numberValue = State(initialValue: property.value.numberValue ?? 0)
        _textValue = State(initialValue: property.value.stringValue ?? "")
        _colorValue = State(initialValue: property.value.colorValue ?? .white)
    }

    var body: some View {
        Group {
            if property.kind == .bool {
                HStack(alignment: .center, spacing: 12) {
                    propertyLabel

                    if property.dirty {
                        Text("Modified")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    Spacer()

                    if property.canRestoreDefaults {
                        Button("Restore Defaults") {
                            restoreDefault()
                        }
                        .buttonStyle(.link)
                        .disabled(bridgeActionInProgress)
                    }

                    Toggle("", isOn: Binding {
                        boolValue
                    } set: { value in
                        edit(.bool(value: value)) {
                            boolValue = value
                        }
                    })
                    .labelsHidden()
                    .toggleStyle(.switch)
                    .disabled(!property.enabled || bridgeActionInProgress)
                }
            } else {
                VStack(alignment: .leading, spacing: 8) {
                    HStack(alignment: .firstTextBaseline) {
                        propertyLabel

                        if property.dirty {
                            Text("Modified")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }

                        Spacer()

                        if property.canRestoreDefaults {
                            Button("Restore Defaults") {
                                restoreDefault()
                            }
                            .buttonStyle(.link)
                            .disabled(bridgeActionInProgress)
                        }
                    }

                    control
                        .disabled(!property.enabled || bridgeActionInProgress)
                }
            }
        }
        .onChange(of: property) { _, updatedProperty in
            reset(from: updatedProperty)
        }
    }

    @ViewBuilder
    private var propertyLabel: some View {
        if property.labelHtml.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            Text(property.id)
        } else {
            RichTextLabel(html: RichTextLabel.sanitizedPropertyLabelHtml(property.labelHtml))
        }
    }

    @ViewBuilder
    private var control: some View {
        switch property.kind {
        case .bool:
            EmptyView()
        case .slider:
            let metadata = sliderMetadata
            HStack {
                Slider(
                    value: Binding {
                        numberValue
                    } set: { value in
                        numberValue = value
                    },
                    in: metadata.range,
                    step: metadata.step,
                    onEditingChanged: { editing in
                        if !editing {
                            edit(.number(value: numberValue))
                        }
                    }
                )
                Text(numberValue.formatted(.number.precision(.fractionLength(metadata.precision))))
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
                    .frame(width: 48, alignment: .trailing)
            }
        case .textInput:
            TextField("Value", text: Binding {
                textValue
            } set: { value in
                textValue = value
            })
            .textFieldStyle(.roundedBorder)
            .onSubmit {
                edit(.string(value: textValue))
            }
        case .color:
            ColorPicker("Color", selection: Binding {
                colorValue
            } set: { value in
                colorValue = value
                let color = NSColor(value).usingColorSpace(.sRGB) ?? .white
                edit(
                    .colorRgb(
                        red: Double(color.redComponent),
                        green: Double(color.greenComponent),
                        blue: Double(color.blueComponent)
                    )
                )
            })
        case .directory:
            HStack(spacing: 8) {
                Text(textValue.isEmpty ? "No image selected" : textValue)
                    .font(.caption)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .foregroundStyle(textValue.isEmpty ? .secondary : .primary)
                    .frame(maxWidth: .infinity, alignment: .leading)

                if !textValue.isEmpty {
                    Button("Clear") {
                        edit(.string(value: "")) {
                            textValue = ""
                        }
                    }
                    .disabled(bridgeActionInProgress)
                }

                Button {
                    chooseTexture()
                } label: {
                    Label("Choose Image", systemImage: "photo.badge.plus")
                }
                .disabled(bridgeActionInProgress)
            }
        case .combo, .text, .group, .unknown:
            Text("Unsupported property type.")
                .foregroundStyle(.secondary)
        }
    }

    private func reset(from property: BridgePropertyDescriptor) {
        boolValue = property.value.boolValue ?? false
        numberValue = property.value.numberValue ?? 0
        textValue = property.value.stringValue ?? ""
        colorValue = property.value.colorValue ?? .white
    }

    private func restoreDefault() {
        performAsyncBridgeAction {
            try await store.restorePropertyDefaultAsync(wallpaperId: wallpaperId, propertyId: property.id)
        }
    }

    private func edit(_ value: BridgePropertyValue, afterSuccess: (() -> Void)? = nil) {
        performAsyncBridgeAction {
            try await store.editPropertyAsync(wallpaperId: wallpaperId, propertyId: property.id, value: value)
            afterSuccess?()
        }
    }

    private func chooseTexture() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.canChooseFiles = true
        panel.allowedContentTypes = [.image]
        panel.title = String(localized: "Choose Image")

        if panel.runModal() == .OK,
           let url = panel.url
        {
            let path = url.path
            edit(.string(value: path)) {
                textValue = path
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
            } catch {
                onError(error)
            }
            bridgeActionInProgress = false
        }
    }

    private var sliderMetadata: SliderMetadata {
        SliderMetadata(property.slider)
    }
}

private struct SliderMetadata {
    let range: ClosedRange<Double>
    let step: Double
    let precision: Int

    init(_ metadata: BridgeSliderMetadata?) {
        let lowerBound = metadata?.min ?? 0
        let upperBound = metadata?.max ?? 1
        range = lowerBound <= upperBound ? lowerBound...upperBound : 0...1
        let rawStep = metadata?.step ?? 0.01
        step = rawStep.isFinite && rawStep > 0 ? rawStep : 0.01
        precision = Swift.min(Int(metadata?.precision ?? 2), 12)
    }
}

private extension BridgePropertyValue {
    var boolValue: Bool? {
        if case let .bool(value) = self {
            return value
        }
        return nil
    }

    var numberValue: Double? {
        if case let .number(value) = self {
            return value
        }
        return nil
    }

    var stringValue: String? {
        if case let .string(value) = self {
            return value
        }
        return nil
    }

    var colorValue: Color? {
        if case let .colorRgb(red, green, blue) = self {
            return Color(red: red, green: green, blue: blue)
        }
        return nil
    }
}
