import AppKit
import SwiftUI

struct RichTextLabel: View {
    let html: String

    var body: some View {
        RichTextLabelRepresentable(attributedString: RichTextLabelCache.shared.attributedString(for: html))
            .fixedSize(horizontal: false, vertical: true)
    }

    static func sanitizedPropertyLabelHtml(_ html: String) -> String {
        RichTextLabelSanitizer.propertyLabelHtml(html)
    }
}

private final class RichTextLabelCache {
    static let shared = RichTextLabelCache()

    private let cache = NSCache<NSString, NSAttributedString>()

    private init() {
        cache.countLimit = 512
    }

    func attributedString(for html: String) -> NSAttributedString {
        let key = html as NSString
        if let cached = cache.object(forKey: key) {
            return cached
        }

        let parsed = parse(html)
        cache.setObject(parsed, forKey: key)
        return parsed
    }

    private func parse(_ html: String) -> NSAttributedString {
        guard let data = html.data(using: .utf8) else {
            return NSAttributedString(string: html)
        }

        if !html.contains("<") && !html.contains("&") {
            return NSAttributedString(string: html)
        }

        do {
            let parsed = try NSMutableAttributedString(
                data: data,
                options: [
                    .documentType: NSAttributedString.DocumentType.html,
                    .characterEncoding: String.Encoding.utf8.rawValue
                ],
                documentAttributes: nil
            )
            scaleImageAttachments(in: parsed)
            return parsed
        } catch {
            return NSAttributedString(string: html)
        }
    }

    private func scaleImageAttachments(in attributedString: NSMutableAttributedString) {
        let fullRange = NSRange(location: 0, length: attributedString.length)
        attributedString.enumerateAttribute(.attachment, in: fullRange) { value, _, _ in
            guard let attachment = value as? NSTextAttachment else {
                return
            }
            let size = attachment.image?.size ?? attachment.bounds.size
            guard size.width > 0, size.height > 0 else {
                return
            }

            let maxSize = NSSize(width: 280, height: 220)
            let scale = min(maxSize.width / size.width, maxSize.height / size.height, 1)
            attachment.bounds = NSRect(
                x: 0,
                y: 0,
                width: ceil(size.width * scale),
                height: ceil(size.height * scale)
            )
        }
    }
}

private enum RichTextLabelSanitizer {
    static func propertyLabelHtml(_ html: String) -> String {
        var output = html
        output = output.replacingOccurrences(
            of: #"<hr\b[^>]*>"#,
            with: "<br>",
            options: [.regularExpression, .caseInsensitive]
        )
        output = output.replacingOccurrences(
            of: #"(?i)</?(?:center|big|small|p|h[1-6])\b[^>]*>"#,
            with: "",
            options: .regularExpression
        )
        return output
    }
}

private struct RichTextLabelRepresentable: NSViewRepresentable {
    let attributedString: NSAttributedString

    func makeNSView(context: Context) -> RichTextField {
        let field = RichTextField(labelWithAttributedString: attributedString)
        field.isSelectable = true
        field.allowsEditingTextAttributes = true
        field.drawsBackground = false
        field.isBordered = false
        field.maximumNumberOfLines = 0
        field.lineBreakMode = .byWordWrapping
        field.cell?.wraps = true
        field.cell?.isScrollable = false
        field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        field.setContentHuggingPriority(.defaultLow, for: .horizontal)
        return field
    }

    func updateNSView(_ nsView: RichTextField, context: Context) {
        nsView.attributedStringValue = attributedString
        nsView.invalidateIntrinsicContentSize()
    }
}

private final class RichTextField: NSTextField {
    override func layout() {
        super.layout()
        preferredMaxLayoutWidth = bounds.width
        invalidateIntrinsicContentSize()
    }

    override var intrinsicContentSize: NSSize {
        guard bounds.width > 0,
              let cell = cell
        else {
            return super.intrinsicContentSize
        }

        let size = cell.cellSize(forBounds: NSRect(
            x: 0,
            y: 0,
            width: bounds.width,
            height: CGFloat.greatestFiniteMagnitude
        ))
        return NSSize(width: NSView.noIntrinsicMetric, height: ceil(size.height))
    }
}
