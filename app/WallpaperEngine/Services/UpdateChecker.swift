import Foundation

private let githubAPIBase = URL(string: "https://api.github.com")!
private let repositorySlug = "bigsaltyfishes/wallpaper-engine-for-macos"
let releasesPageURL = URL(string: "https://github.com/bigsaltyfishes/wallpaper-engine-for-macos/releases")!

enum UpdateCheckStatus: Equatable {
    case upToDate
    case checking
    case updateAvailable

    var label: String {
        switch self {
        case .upToDate:
            "Up to date"
        case .checking:
            "Checking"
        case .updateAvailable:
            "Update available"
        }
    }
}

struct AvailableUpdate: Identifiable, Equatable {
    let id = UUID()
    let version: String
    let releaseNotes: String
    let releaseURL: URL
}

struct UpdateChecker {
    private let session: URLSession

    init(session: URLSession = .shared) {
        self.session = session
    }

    func check(currentShortHash: String) async throws -> AvailableUpdate? {
        let releases = try await fetchReleases()
        guard let release = releases.first(where: { !$0.prerelease && !$0.draft }) else {
            return nil
        }

        let resolvedHash = try await resolveCommitHash(for: release)
        let current = currentShortHash.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        let remote = resolvedHash.lowercased()

        if !current.isEmpty, remote.hasPrefix(current) || current.hasPrefix(String(remote.prefix(current.count))) {
            return nil
        }

        return AvailableUpdate(
            version: release.tagName.isEmpty ? String(remote.prefix(12)) : release.tagName,
            releaseNotes: release.body,
            releaseURL: release.htmlURL
        )
    }

    private func fetchReleases() async throws -> [GitHubRelease] {
        let url = githubAPIBase
            .appending(path: "repos")
            .appending(path: repositorySlug)
            .appending(path: "releases")
        return try await fetch([GitHubRelease].self, from: url)
    }

    private func resolveCommitHash(for release: GitHubRelease) async throws -> String {
        let reference = try await fetchReference(named: release.tagName)
        switch reference.object.type {
        case "commit":
            return reference.object.sha
        case "tag":
            let tag = try await fetchAnnotatedTag(sha: reference.object.sha)
            return tag.object.sha
        default:
            return reference.object.sha
        }
    }

    private func fetchReference(named tagName: String) async throws -> GitReference {
        let url = githubAPIBase
            .appending(path: "repos")
            .appending(path: repositorySlug)
            .appending(path: "git")
            .appending(path: "ref")
            .appending(path: "tags/\(tagName)")
        return try await fetch(GitReference.self, from: url)
    }

    private func fetchAnnotatedTag(sha: String) async throws -> GitTag {
        let url = githubAPIBase
            .appending(path: "repos")
            .appending(path: repositorySlug)
            .appending(path: "git")
            .appending(path: "tags")
            .appending(path: sha)
        return try await fetch(GitTag.self, from: url)
    }

    private func fetch<T: Decodable>(_ type: T.Type, from url: URL) async throws -> T {
        var request = URLRequest(url: url)
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse,
              200..<300 ~= http.statusCode
        else {
            throw UpdateCheckError.requestFailed
        }
        return try JSONDecoder.github.decode(type, from: data)
    }
}

enum UpdateCheckError: LocalizedError {
    case requestFailed

    var errorDescription: String? {
        "GitHub did not return a successful update response."
    }
}

private struct GitHubRelease: Decodable {
    let tagName: String
    let prerelease: Bool
    let draft: Bool
    let body: String
    let htmlURL: URL

    enum CodingKeys: String, CodingKey {
        case tagName = "tag_name"
        case prerelease
        case draft
        case body
        case htmlURL = "html_url"
    }
}

private struct GitReference: Decodable {
    let object: GitObject
}

private struct GitTag: Decodable {
    let object: GitObject
}

private struct GitObject: Decodable {
    let sha: String
    let type: String
}

private extension JSONDecoder {
    static var github: JSONDecoder {
        JSONDecoder()
    }
}
