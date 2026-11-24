import SwiftUI

private actor MarkdownParser {
    static let shared = MarkdownParser()
    func blocks(_ source: String) throws -> [Block] {
        try Task.checkCancellation()
        return parse(source: source, fileImages: false).blocks()
    }
}
struct MarkdownBody: View {
    let source: String
    @State private var blocks: [Block] = []
    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            ForEach(blocks, id: \.id) { MarkdownBlock(block: $0) }
        }.task(id: source) {
            if let result = try? await MarkdownParser.shared.blocks(source), !Task.isCancelled { blocks = result }
        }
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
            case .image(let image): result.append(NSAttributedString(string: image.alt.isEmpty ? "画像" : image.alt, attributes: [.font: UIFont.systemFont(ofSize: scale), .foregroundColor: UIColor.secondaryLabel]))
            }
        }
        if v.attributedText != result { v.attributedText = result }
        v.linkTextAttributes = [.foregroundColor: UIColor(PiTheme.accent), .underlineStyle: NSUnderlineStyle.single.rawValue]
        if heading { v.accessibilityTraits.insert(.header) }
    }
    func sizeThatFits(_ proposal: ProposedViewSize, uiView: UITextView, context: Context) -> CGSize? {
        guard let width = proposal.width, width.isFinite else { return nil }
        return CGSize(width: width, height: ceil(uiView.sizeThatFits(CGSize(width: width, height: CGFloat.greatestFiniteMagnitude)).height))
    }
}
private struct MarkdownBlock: View {
    let block: Block
    var body: some View {
        switch block.content {
        case .paragraph(let spans): Selectable(spans: spans)
        case .heading(_, let spans): Selectable(spans: spans, heading: true)
        case .rule: Divider()
        case .code(let language, let text):
            VStack(alignment: .leading, spacing: 4) {
                HStack { Text(language ?? "コード").font(.caption.monospaced()); Spacer(); Button("コードをコピー", systemImage: "doc.on.doc") { UIPasteboard.general.string = text }.labelStyle(.iconOnly).frame(minWidth: 44, minHeight: 44) }
                ScrollView(.horizontal) { Text(verbatim: text).font(.system(.callout, design: .monospaced)).textSelection(.enabled).fixedSize() }
            }.padding(.horizontal, 12).padding(.bottom, 12).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 12))
        case .quote(let children): HStack { Rectangle().fill(PiTheme.secondary).frame(width: 2); nested(children) }
        case .blockList(let start, let items):
            VStack(alignment: .leading, spacing: 6) { ForEach(Array(items.enumerated()), id: \.element.id) { i, item in
                HStack(alignment: .top, spacing: 8) {
                    if case .item(let checked?, _) = item.content { Text(checked ? "☑" : "☐").accessibilityLabel(checked ? "完了" : "未完了") }
                    else { Text(start.map { "\(Int($0) + i)." } ?? "•") }; MarkdownBlock(block: item)
                }
            } }
        case .item(_, let children): nested(children)
        case .table(let table): MarkdownTable(table: table)
        }
    }
    private func nested(_ children: [Block]) -> some View { VStack(alignment: .leading, spacing: 6) { ForEach(children, id: \.id) { MarkdownBlock(block: $0) } } }
}
private struct MarkdownTable: View {
    let table: Table
    @State private var expanded = false
    @State private var width: CGFloat = 300
    @ScaledMetric(relativeTo: .body) private var ratio: CGFloat = 1
    @AppStorage("typeSize") private var size = TypeSize.standard
    private func grid(full: Bool) -> some View {
        let rows = [table.header] + table.rows
        let natural = (0..<(rows.map(\.count).max() ?? 0)).map { c in rows.compactMap { c < $0.count ? cellWidth(cell: $0[c]) : nil }.max() ?? 0 }
        let widths = tableWidths(widths: natural, available: width, fontRatio: ratio * size.factor)
        return ScrollView(.horizontal) {
            VStack(alignment: .leading, spacing: 0) { ForEach(rows.indices, id: \.self) { i in
                HStack(alignment: .top, spacing: 0) { ForEach(rows[i].indices, id: \.self) { j in
                    Selectable(spans: rows[i][j], heading: i == 0, links: full).allowsHitTesting(full).padding(8).frame(width: widths[j], alignment: .leading)
                } }.background(i == 0 ? PiTheme.surface : PiTheme.background); Divider()
            } }
        }.background(GeometryReader { g in Color.clear.onAppear { width = g.size.width }.onChange(of: g.size.width) { _, w in width = w } })
    }
    var body: some View {
        grid(full: false).contentShape(Rectangle()).onTapGesture { expanded = true }.accessibilityAction(named: "表を開く") { expanded = true }
            .sheet(isPresented: $expanded) { NavigationStack { ScrollView { grid(full: true).padding() }.navigationTitle("表").navigationBarTitleDisplayMode(.inline).toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("閉じる") { expanded = false } }
                ToolbarItem(placement: .primaryAction) { Button("表をコピー", systemImage: "doc.on.doc") { UIPasteboard.general.string = table.markdown } }
            } } }
    }
}
