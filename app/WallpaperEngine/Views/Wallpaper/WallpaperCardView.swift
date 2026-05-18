import SwiftUI

struct WallpaperCardView: View {
    let wallpaper: BridgeWallpaperEntry

    private var borderColor: Color {
        if wallpaper.active {
            return .green
        }
        if wallpaper.selected {
            return .accentColor
        }
        return Color(nsColor: .separatorColor)
    }

    private var borderWidth: CGFloat {
        wallpaper.active || wallpaper.selected ? 3 : 1
    }

    private var kindTitle: String {
        switch wallpaper.kind {
        case .projectScene:
            String(localized: "Project Scene")
        case .video:
            String(localized: "Video")
        case .webpage:
            String(localized: "Webpage")
        case .unknown:
            String(localized: "Unknown")
        }
    }

    private var fallbackSystemImage: String {
        switch wallpaper.kind {
        case .projectScene:
            "photo.on.rectangle"
        case .video:
            "play.rectangle"
        case .webpage:
            "globe"
        case .unknown:
            "questionmark.square"
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ZStack {
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color(nsColor: .controlBackgroundColor))

                if let previewPath = wallpaper.previewPath,
                   let image = NSImage(contentsOfFile: previewPath)
                {
                    Image(nsImage: image)
                        .resizable()
                        .scaledToFill()
                } else {
                    Image(systemName: fallbackSystemImage)
                        .font(.system(size: 34, weight: .regular))
                        .foregroundStyle(.secondary)
                }

                Image(systemName: "play.circle.fill")
                    .font(.system(size: 28))
                    .foregroundStyle(.white)
                    .shadow(radius: 3)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
                    .padding(12)
            }
            .aspectRatio(16.0 / 10.0, contentMode: .fit)
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .opacity(wallpaper.supported ? 1.0 : 0.55)

            Text(wallpaper.title)
                .font(.headline)
                .lineLimit(2)
                .foregroundStyle(.primary)
                .frame(maxWidth: .infinity, alignment: .leading)

            Text(kindTitle)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(12)
        .background {
            RoundedRectangle(cornerRadius: 8)
                .fill(Color(nsColor: .windowBackgroundColor))
        }
        .overlay {
            RoundedRectangle(cornerRadius: 8)
                .stroke(borderColor, lineWidth: borderWidth)
        }
        .contentShape(RoundedRectangle(cornerRadius: 8))
    }
}
