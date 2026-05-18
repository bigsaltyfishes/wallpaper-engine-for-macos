import SwiftUI

struct SettingsView: View {
    var body: some View {
        Form {
            ProgramSettingsSection()
            DisplaySettingsSection()
            AboutSection()
        }
        .formStyle(.grouped)
        .navigationTitle("Settings")
    }
}
