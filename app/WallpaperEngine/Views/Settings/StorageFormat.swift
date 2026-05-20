import Foundation

enum StorageFormat {
    static func bytes(_ value: UInt64) -> String {
        let units = ["B", "KB", "MB", "GB", "TB"]
        var amount = Double(value)
        var unitIndex = 0

        while amount >= 1024, unitIndex < units.count - 1 {
            amount /= 1024
            unitIndex += 1
        }

        if unitIndex == 0 {
            return "\(value) \(units[unitIndex])"
        }

        let formatted = amount >= 10
            ? String(format: "%.1f", amount)
            : String(format: "%.2f", amount)
        return "\(formatted) \(units[unitIndex])"
    }
}
