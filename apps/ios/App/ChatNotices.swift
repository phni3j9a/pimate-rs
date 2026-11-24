import Foundation

struct ChatReceipt {
    enum Outcome { case done, failed, cancelled, unknown }
    let instance: String, id: String
    var value: Object, dismissed = false, textHandled = false
    var outcome: Outcome {
        let event = value.obj("event")
        if value.str("outcome") == "unknown" { return .unknown }
        if event["error"] is String || event.str("type") == "compact_error" { return .failed }
        if event["cancelled"] as? Bool == true { return .cancelled }
        if value.str("outcome") == "done" || event.str("type") == "compact_done" ||
            (event.str("type") == "session_op_done" && event["cancelled"] as? Bool == false) { return .done }
        return .unknown
    }
}
struct ChatMove {
    let id = UUID(), instance: String, notice: Object, local: Bool
    var confirmed = false
}

// Per host, process the ordered diff rather than trying to infer ownership from the final view.
struct ChatNotices {
    private var pending: (id: String, instance: String, value: Object)?
    var receipt: ChatReceipt?, move: ChatMove?
    mutating func update(_ group: String, _ id: String, _ item: Item?, instance: String?, snapshot: Bool = false) {
        switch (group, id) {
        case ("busy", "operation"):
            guard let o = item?.object, let request = o["id"] as? String, !request.isEmpty,
                  ["session_op", "compact"].contains(o.str("method")) else { pending = nil; return }
            // A snapshot has no ordering: never assign a newly discovered operation to its current Pi.
            let owner = pending?.id == request ? pending?.instance : (snapshot ? nil : instance)
            pending = owner.map { (request, $0, o) }
        case ("operationResult", "latest"):
            guard let value = item?.object, let request = value["id"] as? String else { return }
            if receipt?.id == request, receipt?.value["sessionId"] as? String == value["sessionId"] as? String {
                receipt?.value = value
            } else if let p = pending, p.id == request { receipt = ChatReceipt(instance: p.instance, id: request, value: value) }
        case ("notice", "sessionChanged"):
            guard let n = item?.object, let instance else { move = nil; return }
            let from = n.obj("from").str("id"), to = n.obj("to").str("id"), reason = n.str("reason")
            let local = !snapshot && !to.isEmpty && !from.isEmpty && pending?.instance == instance && pending?.value.str("sessionId") == from && pending?.value.str("arrival") == to && (pending?.value.str("op") == reason || (reason == "resume" && pending?.value.str("op") == "switch"))
            move = ChatMove(instance: instance, notice: n, local: local)
        default: break
        }
    }
}

struct ChatEdge {
    let connection: String?, phase: String?, pcPrompt: Object?, attachCommand: String?
    let unconfirmed: [Item]
    init(_ rows: Rows, instance: String) {
        connection = rows.groups["host"]?.first { $0.id == "connection" }?.string
        phase = rows.groups["view"]?.first { $0.id == "phase" }?.string
        let here = rows.groups["view"]?.first { $0.id == "instanceId" }?.string == instance
        pcPrompt = here ? rows.groups["view"]?.first { $0.id == "pcPrompt" && $0.json != "null" }?.object : nil
        attachCommand = here ? rows.attachCommand : nil
        unconfirmed = (rows.groups["draft"] ?? []).filter { let o = $0.object; return o.str("instanceId") == instance && o.str("status") == "unconfirmed" }
    }
}
