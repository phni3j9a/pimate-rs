import SwiftUI

struct HorizontalRegions: PreferenceKey {
    static let defaultValue: [CGRect] = []
    static func reduce(value: inout [CGRect], nextValue: () -> [CGRect]) { value += nextValue() }
}
private extension View {
    func keepsHorizontalDrag() -> some View {
        background(GeometryReader { geo in Color.clear.preference(key: HorizontalRegions.self, value: [geo.frame(in: .named("transcript"))]) })
    }
}
private actor MarkdownParser {
    static let shared = MarkdownParser()
    func blocks(_ source: String, files: Bool) throws -> ([Block], [MarkdownImage]) {
        try Task.checkCancellation()
        let doc = parse(source: source, fileImages: files)
        return (doc.blocks(), doc.images().filter { image in
            image.kind != .remote || URL(string: image.src.value()).map { WebImage.valid($0, scheme: "https") } == true
        })
    }
}
struct MarkdownBody: View {
    let source: String, answer: String
    @Environment(\.photoContext) private var photos
    @State private var blocks: [Block] = []
    @State private var gallery: [MarkdownImage] = []
    var copyAnswer = false
    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            ForEach(blocks, id: \.id) { MarkdownBlock(block: $0, answer: answer) }
            if copyAnswer { CopyText(text: source, label: "回答全文をコピー（Markdown）") }
        }.environment(\.markdownGallery, gallery).task(id: [source, photos?.files == true ? "file" : ""]) {
            if let result = try? await MarkdownParser.shared.blocks(source, files: photos?.files == true), !Task.isCancelled { (blocks, gallery) = result }
        }
    }
}
struct CopyText: View {
    let text: String, label: String
    var body: some View {
        Button { UIPasteboard.general.string = text; UIAccessibility.post(notification: .announcement, argument: "コピーしました") } label: {
            Label(label, systemImage: "doc.on.doc").modifier(PiFont(.caption)).frame(minHeight: 44)
        }.buttonStyle(.borderless)
    }
}
// UITextView exposes the native range-selection handles, unlike SwiftUI Text on iOS 17.
struct Selectable: UIViewRepresentable {
    let spans: [Inline]
    var heading = false, mono = false, links = true
    @ScaledMetric(relativeTo: .body) private var points: CGFloat = 15
    @AppStorage("typeSize") private var size = TypeSize.standard
    func makeUIView(context: Context) -> UITextView {
        let v = UITextView(); v.isEditable = false; v.isScrollEnabled = false; v.backgroundColor = .clear
        v.textContainerInset = .zero; v.textContainer.lineFragmentPadding = 0; v.setContentCompressionResistancePriority(.defaultLow, for: .horizontal); return v
    }
    func updateUIView(_ v: UITextView, context: Context) {
        let result = NSMutableAttributedString(string: ""), scale = points * size.factor * (heading ? 1.15 : 1)
        for span in spans {
            switch span {
            case .text(let text, let style):
                let weight: UIFont.Weight = heading || style.bold ? .semibold : .regular
                var font = style.code || mono ? UIFont.monospacedSystemFont(ofSize: scale, weight: weight) : UIFont.systemFont(ofSize: scale, weight: weight)
                if style.italic, let descriptor = font.fontDescriptor.withSymbolicTraits(font.fontDescriptor.symbolicTraits.union(.traitItalic)) { font = UIFont(descriptor: descriptor, size: scale) }
                var attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: UIColor(PiTheme.text)]
                if style.strike { attrs[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }
                if style.code { attrs[.backgroundColor] = UIColor(PiTheme.surface) }
                if links, let href = style.href?.value(), let url = URL(string: href), ["https", "http"].contains(url.scheme?.lowercased() ?? "") { attrs[.link] = url }
                result.append(NSAttributedString(string: text, attributes: attrs))
            case .image(let image): result.append(Self.imageLabel(image, links: links, size: scale))
            }
        }
        if v.attributedText != result { v.attributedText = result }
        v.linkTextAttributes = [.foregroundColor: UIColor(PiTheme.accent), .underlineStyle: NSUnderlineStyle.single.rawValue]
        if heading { v.accessibilityTraits.insert(.header) }
    }
    static func imageLabel(_ image: MarkdownImage, links: Bool, size: CGFloat) -> NSAttributedString {
        var attrs: [NSAttributedString.Key: Any] = [.font: UIFont.systemFont(ofSize: size), .foregroundColor: UIColor.secondaryLabel]
        if links, image.kind == .remote, let url = URL(string: image.src.value()), WebImage.valid(url, scheme: "http") { attrs[.link] = url }
        return NSAttributedString(string: image.alt.isEmpty ? "画像" : image.alt, attributes: attrs)
    }
    func sizeThatFits(_ proposal: ProposedViewSize, uiView: UITextView, context: Context) -> CGSize? {
        guard let width = proposal.width, width.isFinite else { return nil }
        return CGSize(width: width, height: ceil(uiView.sizeThatFits(CGSize(width: width, height: CGFloat.greatestFiniteMagnitude)).height))
    }
}
private struct MarkdownBlock: View {
    let block: Block, answer: String
    var body: some View {
        switch block.content {
        case .paragraph(let spans): PhotoParagraph(spans: spans, answer: answer, heading: false)
        case .heading(_, let spans): PhotoParagraph(spans: spans, answer: answer, heading: true)
        case .rule: Divider()
        case .code(let language, let text):
            VStack(alignment: .leading, spacing: 4) {
                HStack { Text(language ?? "コード").font(.caption.monospaced()); Spacer(); Button("コードをコピー", systemImage: "doc.on.doc") { UIPasteboard.general.string = text }.labelStyle(.iconOnly).frame(minWidth: 44, minHeight: 44) }
                ScrollView(.horizontal) { Text(verbatim: text).font(.system(.callout, design: .monospaced)).textSelection(.enabled).fixedSize() }.keepsHorizontalDrag()
            }.padding(.horizontal, 12).padding(.bottom, 12).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 12))
        case .quote(let children): HStack { Rectangle().fill(PiTheme.secondary).frame(width: 2); nested(children) }
        case .blockList(let start, let items):
            VStack(alignment: .leading, spacing: 6) { ForEach(Array(items.enumerated()), id: \.element.id) { i, item in
                HStack(alignment: .top, spacing: 8) {
                    if case .item(let checked?, _) = item.content { Text(checked ? "☑" : "☐").accessibilityLabel(checked ? "完了" : "未完了") }
                    else { Text(start.map { "\(Int($0) + i)." } ?? "•") }; MarkdownBlock(block: item, answer: answer)
                }
            } }
        case .item(_, let children): nested(children)
        case .table(let table): MarkdownTable(table: table, answer: answer)
        }
    }
    private func nested(_ children: [Block]) -> some View { VStack(alignment: .leading, spacing: 6) { ForEach(children, id: \.id) { MarkdownBlock(block: $0, answer: answer) } } }
}
struct OpenTable: Identifiable {
    let id = UUID()
    let table: Table, answer: String, gallery: [MarkdownImage]
}
private struct OpenTableKey: EnvironmentKey { static let defaultValue: (OpenTable) -> Void = { _ in } }
extension EnvironmentValues {
    var openPiTable: (OpenTable) -> Void { get { self[OpenTableKey.self] } set { self[OpenTableKey.self] = newValue } }
}
struct MarkdownTable: View {
    let table: Table, answer: String
    @Environment(\.markdownGallery) private var gallery
    var full = false
    @Environment(\.openPiTable) private var open
    @State private var width: CGFloat = 300
    @ScaledMetric(relativeTo: .body) private var ratio: CGFloat = 1
    @AppStorage("typeSize") private var size = TypeSize.standard
    private func showTable() { open(OpenTable(table: table, answer: answer, gallery: gallery)) }
    private func interactive(_ cell: [Inline]) -> Bool {
        cell.contains { span in
            switch span { case .image: true; case .text(_, let style): style.href != nil }
        }
    }
    private func grid(full: Bool) -> some View {
        let rows = [table.header] + table.rows
        let natural = (0..<(rows.map(\.count).max() ?? 0)).map { c in rows.compactMap { c < $0.count ? cellWidth(cell: $0[c]) : nil }.max() ?? 0 }
        let widths = tableWidths(widths: natural, available: width, fontRatio: ratio * size.factor)
        return ScrollView(.horizontal) {
            VStack(alignment: .leading, spacing: 0) { ForEach(rows.indices, id: \.self) { i in
                HStack(alignment: .top, spacing: 0) { ForEach(rows[i].indices, id: \.self) { j in
                    VStack(alignment: .leading, spacing: 6) {
                        if full || interactive(rows[i][j]) {
                            PhotoParagraph(spans: rows[i][j], answer: answer, heading: i == 0)
                        } else {
                            Button(action: showTable) {
                                Selectable(spans: rows[i][j], heading: i == 0).allowsHitTesting(false)
                                    .frame(maxWidth: .infinity, minHeight: 28, alignment: .leading).contentShape(Rectangle())
                            }.buttonStyle(.plain).accessibilityHint("表を開く").accessibilityIdentifier("table-cell")
                        }
                    }.padding(8).frame(width: widths[j], alignment: .leading)
                } }.background(i == 0 ? PiTheme.surface : PiTheme.background); Divider()
            } }
        }.background(GeometryReader { g in Color.clear.onAppear { width = g.size.width }.onChange(of: g.size.width) { _, w in width = w } })
    }
    var body: some View {
        grid(full: full).keepsHorizontalDrag().accessibilityAction(named: "表を開く") { if !full { showTable() } }
    }
}

private struct GalleryKey: EnvironmentKey { static let defaultValue: [MarkdownImage] = [] }
private extension EnvironmentValues {
    var markdownGallery: [MarkdownImage] { get { self[GalleryKey.self] } set { self[GalleryKey.self] = newValue } }
}
// Preserve native range selection across adjacent text; only an image splits a run.
private struct PhotoParagraph: View {
    let spans: [Inline], answer: String, heading: Bool
    @Environment(\.photoContext) private var context
    @Environment(\.markdownGallery) private var images
    private enum Part { case text([Inline]), photo(MarkdownImage) }
    private var parts: [Part] {
        var parts: [Part] = [], run: [Inline] = []
        for span in spans {
            if case .image(let image) = span {
                if !run.isEmpty { parts.append(.text(run)); run = [] }; parts.append(.photo(image))
            } else { run.append(span) }
        }
        if !run.isEmpty { parts.append(.text(run)) }; return parts
    }
    var body: some View {
        ForEach(Array(parts.enumerated()), id: \.offset) { _, part in
            switch part {
            case .text(let text): Selectable(spans: text, heading: heading)
            case .photo(let image):
                if image.kind == .remote, let url = URL(string: image.src.value()), WebImage.valid(url, scheme: "http") {
                    Link(image.alt.isEmpty ? "画像をブラウザで開く" : image.alt, destination: url)
                } else if let context, let start = images.firstIndex(where: { $0.index == image.index }) {
                    let gallery = images // Capture the opened answer, not subsequent streamed text.
                    PhotoTile(context: context, gallery: OpenPhoto(start: start, count: gallery.count, label: { gallery[$0].alt.isEmpty ? "画像" : gallery[$0].alt }, source: { i, _ in try? ChatImageSource(markdown: gallery[i], answer: answer) }), answer: answer)
                        .frame(maxWidth: 300).frame(height: 144)
                } else { Text(image.alt.isEmpty ? "画像を表示できません" : image.alt).foregroundStyle(PiTheme.secondary) }
            }
        }
    }
}

struct TableViewer: View {
    let shown: OpenTable, context: PhotoContext?
    @Environment(\.dismiss) private var dismiss
    @State private var photo: OpenPhoto?
    var body: some View {
        NavigationStack {
            ScrollView { MarkdownTable(table: shown.table, answer: shown.answer, full: true).padding() }
                .modifier(PhotoViewport()).environment(\.markdownGallery, shown.gallery)
                .environment(\.photoContext, context.map { PhotoContext(model: $0.model, key: $0.key, entries: $0.entries, files: $0.files, open: { photo = $0 }) })
                .navigationTitle("表").navigationBarTitleDisplayMode(.inline).toolbar {
                    ToolbarItem(placement: .cancellationAction) { Button("閉じる") { dismiss() }.accessibilityLabel("表を閉じる") }
                    ToolbarItem(placement: .primaryAction) { Button("表をコピー", systemImage: "doc.on.doc") {
                        UIPasteboard.general.string = shown.table.markdown; UIAccessibility.post(notification: .announcement, argument: "表を Markdown でコピーしました")
                    } }
                }
        }.accessibilityHidden(photo != nil).fullScreenCover(item: $photo) { gallery in if let context { PhotoViewer(gallery: gallery, context: context) } }
    }
}
