import SwiftUI

struct EditableNumberField: View {
    let value: UInt32
    let range: ClosedRange<UInt32>
    var onCommit: (UInt32) -> Void = { _ in }

    @State private var isEditing = false
    @State private var draft = ""
    @FocusState private var focused: Bool

    var body: some View {
        Group {
            if isEditing {
                TextField("", text: $draft)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 58)
                    .focused($focused)
                    .onSubmit(commit)
                    .onChange(of: focused) { _, isFocused in
                        if !isFocused {
                            commit()
                        }
                    }
            } else {
                Text(value.formatted())
                    .monospacedDigit()
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color(nsColor: .controlBackgroundColor))
                    .clipShape(RoundedRectangle(cornerRadius: 4))
                    .onTapGesture(count: 2, perform: beginEditing)
            }
        }
        .accessibilityLabel("Target Frame Rate Value")
    }

    private func beginEditing() {
        draft = value.formatted()
        isEditing = true
        focused = true
    }

    private func commit() {
        guard isEditing else {
            return
        }

        let parsed = UInt32(draft.trimmingCharacters(in: .whitespacesAndNewlines)) ?? value
        let clamped = min(max(parsed, range.lowerBound), range.upperBound)
        onCommit(clamped)
        isEditing = false
        focused = false
    }
}
