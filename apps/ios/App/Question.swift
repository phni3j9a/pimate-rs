import SwiftUI

private struct Choice: Codable { var selected: [String] = []; var custom = "" }
struct QuestionCard: View {
    let model: SessionModel, key: QuestionKey, item: Item
    private var q: Object { item.object }
    private var kind: String { q.str("kind") }
    private var text: String { model.answerText(key) }
    private var questions: [Object] { q.list("questions") }
    private var choices: [Choice] { (try? JSONDecoder().decode([Choice].self, from: Data(text.utf8))).flatMap { $0.count == questions.count ? $0 : nil } ?? Array(repeating: Choice(), count: questions.count) }
    private func write(_ s: String) { model.editAnswer(key, shown: item.json, text: s) }
    private func edit(_ i: Int, _ change: (inout Choice) -> Void) { var a = choices; change(&a[i]); if let data = try? JSONEncoder().encode(a) { write(String(decoding: data, as: UTF8.self)) } }
    private func submit(_ fields: Object) { Task { try? await model.answer(key, shown: item.json, fields: fields) } }
    private func option(_ title: String, _ detail: String, checked: Bool, multi: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) { HStack(alignment: .top, spacing: 10) {
            SwiftUI.Image(systemName: multi ? (checked ? "checkmark.square.fill" : "square") : (checked ? "largecircle.fill.circle" : "circle")).foregroundStyle(checked ? PiTheme.accent : PiTheme.secondary)
            VStack(alignment: .leading, spacing: 4) { Text(title); if !detail.isEmpty { Text(detail).modifier(PiFont(.caption)).foregroundStyle(PiTheme.secondary) } }; Spacer()
        }.padding(.vertical, 6).frame(minHeight: 44).contentShape(Rectangle()) }.buttonStyle(.plain).accessibilityAddTraits(checked ? .isSelected : [])
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Label("入力待ち", systemImage: "circle.inset.filled").font(.caption.bold()).foregroundStyle(PiTheme.waiting)
            Text(q.str("title")).font(.headline).accessibilityAddTraits(.isHeader)
            if !q.str("message").isEmpty && kind != "editor" { Text(q.str("message")) }
            switch kind {
            case "select": ForEach(q["options"] as? [String] ?? [], id: \.self) { s in option(s, "", checked: text == s, multi: false) { write(s) } }
            case "editor", "input": TextField("回答を入力", text: Binding(get: { text }, set: write), axis: .vertical).textFieldStyle(.roundedBorder)
            case "questionnaire":
                ForEach(questions.indices, id: \.self) { i in
                    let part = questions[i], multi = part["multiSelect"] as? Bool == true
                    if questions.count > 1 || part.str("question") != q.str("title") { Text(part.str("question")).fontWeight(.semibold) }
                    ForEach(Array(part.list("options").enumerated()), id: \.offset) { _, o in
                        let label = o.str("label"), checked = choices[i].selected.contains(label)
                        option(label, o.str("description"), checked: checked, multi: multi) { edit(i) { choice in
                            if !multi { choice.selected = [label]; choice.custom = "" } else if checked { choice.selected.removeAll { $0 == label } } else { choice.selected.append(label) }
                        } }
                    }
                    TextField("その他（自由に入力）", text: Binding(get: { choices[i].custom }, set: { s in edit(i) { $0.custom = s; if !multi && !s.isEmpty { $0.selected = [] } } })).textFieldStyle(.roundedBorder)
                }
            case "confirm": EmptyView()
            default: Text("PC で回答してください")
            }
            if ["confirm", "select", "editor", "input", "questionnaire"].contains(kind) {
                HStack {
                    Button(kind == "confirm" ? "いいえ" : "キャンセル") { submit(kind == "confirm" ? ["confirmed": false] : ["cancelled": true]) }.frame(minHeight: 44)
                    Button(kind == "confirm" ? "はい" : "回答する") {
                        switch kind {
                        case "confirm": submit(["confirmed": true])
                        case "questionnaire": submit(["answers": choices.map { c -> Object in var o: Object = ["selected": c.selected]; if !c.custom.isEmpty { o["custom"] = c.custom }; return o }])
                        default: submit(["value": text])
                        }
                    }.buttonStyle(PiPrimary()).disabled(kind == "select" && text.isEmpty || kind == "questionnaire" && questions.indices.contains { i in questions[i]["multiSelect"] as? Bool != true && choices[i].selected.isEmpty && choices[i].custom.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty })
                }
            }
        }.padding(16).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 18)).disabled(model.busy)
    }
}
