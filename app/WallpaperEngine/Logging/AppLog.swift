import Foundation

enum AppLog {
    static weak var store: BridgeStore?

    static func trace(_ message: @autoclosure () -> String, file: StaticString = #fileID, line: UInt = #line) {
        emit(level: .trace, message: message(), file: file, line: line)
    }

    static func debug(_ message: @autoclosure () -> String, file: StaticString = #fileID, line: UInt = #line) {
        emit(level: .debug, message: message(), file: file, line: line)
    }

    static func info(_ message: @autoclosure () -> String, file: StaticString = #fileID, line: UInt = #line) {
        emit(level: .info, message: message(), file: file, line: line)
    }

    static func warn(_ message: @autoclosure () -> String, file: StaticString = #fileID, line: UInt = #line) {
        emit(level: .warn, message: message(), file: file, line: line)
    }

    static func error(_ message: @autoclosure () -> String, file: StaticString = #fileID, line: UInt = #line) {
        emit(level: .error, message: message(), file: file, line: line)
    }

    private static func emit(level: BridgeLogLevel, message: String, file: StaticString, line: UInt) {
        guard let store else {
            return
        }

        Task { @MainActor in
            do {
                try store.emitLog(level: level, file: "\(file)", line: UInt32(clamping: line), message: message)
            } catch {
                fputs("ERROR AppLog.swift:0 failed to emit GUI log: \(error)\n", stderr)
            }
        }
    }
}
