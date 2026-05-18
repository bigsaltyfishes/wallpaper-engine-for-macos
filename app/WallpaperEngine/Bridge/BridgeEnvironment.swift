import Darwin
import Foundation

enum BridgeEnvironment {
    static func configureVulkanICDIfNeeded() {
        if let currentValue = ProcessInfo.processInfo.environment["VK_ICD_FILENAMES"],
           !currentValue.isEmpty
        {
            return
        }

        guard let icdURL = Bundle.main.url(forResource: "MoltenVK_icd", withExtension: "json") else {
            return
        }

        setenv("VK_ICD_FILENAMES", icdURL.path, 1)
    }
}
