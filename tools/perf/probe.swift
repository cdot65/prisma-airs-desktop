// Timing probe for the registered C03 budgets (docs/architecture/budgets.json),
// compiled by tools/perf/measure_c03.py with swiftc.
//
//   probe launch <executable> <heading prefix> <timeout ms>
//       Spawns the executable (stdio to /dev/null) and polls the new
//       process's Accessibility tree every 5 ms until an AXHeading whose
//       title/description starts with the prefix exists. Prints
//       {"pid": n, "ms": elapsed, "polls": n}.
//   probe type <pid> <count>
//       The app must be frontmost with a text field focused. For each
//       sample, posts one key event to the process (CGEventPostToPid, so it
//       can reach no other application) and polls the focused element's
//       AXValue every 0.5 ms until the character arrived. Prints
//       {"samples": [ms, ...]}.
//   probe activate <pid> <role> <label prefix> <heading> <text prefix> <timeout ms>
//       AXPresses the first element with that role whose label starts with
//       the prefix, then polls the Accessibility tree every 2 ms until an
//       AXHeading titled exactly <heading> and an AXStaticText whose value
//       starts with <text prefix> both exist. Prints {"ms": elapsed, "polls": n}.

import AppKit
import ApplicationServices
import Foundation

func attr(_ el: AXUIElement, _ name: String) -> AnyObject? {
    var value: AnyObject?
    return AXUIElementCopyAttributeValue(el, name as CFString, &value) == .success ? value : nil
}

func label(_ el: AXUIElement) -> String {
    for name in [kAXTitleAttribute, kAXDescriptionAttribute] {
        if let s = attr(el, name) as? String, !s.isEmpty { return s }
    }
    return ""
}

func findHeading(_ app: AXUIElement, _ prefix: String) -> Bool {
    var stack = (attr(app, kAXWindowsAttribute) as? [AXUIElement]) ?? []
    var visited = 0
    while let el = stack.popLast(), visited < 4000 {
        visited += 1
        if (attr(el, kAXRoleAttribute) as? String) == "AXHeading", label(el).hasPrefix(prefix) { return true }
        stack.append(contentsOf: (attr(el, kAXChildrenAttribute) as? [AXUIElement]) ?? [])
    }
    return false
}

func fail(_ message: String) -> Never {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(1)
}

func ms(since start: UInt64) -> Double {
    Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000
}

func emit(_ object: [String: Any]) {
    let data = try! JSONSerialization.data(withJSONObject: object, options: [])
    FileHandle.standardOutput.write(data)
}

let args = CommandLine.arguments
guard args.count >= 3 else { fail("usage: probe launch|type|activate ...") }
switch args[1] {
case "launch":
    guard args.count == 5, let timeout = Double(args[4]) else { fail("launch <executable> <prefix> <timeout ms>") }
    let process = Process()
    process.executableURL = URL(fileURLWithPath: args[2])
    process.standardInput = FileHandle.nullDevice
    process.standardOutput = FileHandle.nullDevice
    process.standardError = FileHandle.nullDevice
    let start = DispatchTime.now().uptimeNanoseconds
    do { try process.run() } catch { fail("spawn failed: \(error)") }
    let app = AXUIElementCreateApplication(process.processIdentifier)
    var polls = 0
    while ms(since: start) < timeout {
        polls += 1
        // WebKit builds a page's tree only for an AX client that announces itself.
        AXUIElementSetAttributeValue(app, "AXEnhancedUserInterface" as CFString, kCFBooleanTrue)
        if findHeading(app, args[3]) {
            emit(["pid": Int(process.processIdentifier), "ms": ms(since: start), "polls": polls])
            exit(0)
        }
        usleep(5_000)
    }
    emit(["pid": Int(process.processIdentifier), "ms": NSNull(), "polls": polls])
    exit(2)
case "type":
    guard args.count == 4, let pid = pid_t(args[2]), let count = Int(args[3]) else { fail("type <pid> <count>") }
    let app = AXUIElementCreateApplication(pid)
    let source = CGEventSource(stateID: .hidSystemState)
    var samples: [Double] = []
    for _ in 0..<count {
        guard let focused = attr(app, kAXFocusedUIElementAttribute) else { fail("no focused element") }
        let element = focused as! AXUIElement
        let before = (attr(element, kAXValueAttribute) as? String)?.count ?? 0
        let down = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: true)!
        let up = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: false)!
        let start = DispatchTime.now().uptimeNanoseconds
        down.postToPid(pid)
        up.postToPid(pid)
        var arrived = false
        while ms(since: start) < 2000 {
            if ((attr(element, kAXValueAttribute) as? String)?.count ?? 0) > before {
                arrived = true
                break
            }
            usleep(500)
        }
        guard arrived else { fail("a key event did not reach the focused field within 2 s") }
        samples.append(ms(since: start))
        usleep(40_000)
    }
    emit(["samples": samples])
case "activate":
    guard args.count == 8, let pid = pid_t(args[2]), let timeout = Double(args[7]) else {
        fail("activate <pid> <role> <label prefix> <heading> <text prefix> <timeout ms>")
    }
    let app = AXUIElementCreateApplication(pid)
    func elements() -> [AXUIElement] {
        var out: [AXUIElement] = []
        var stack = (attr(app, kAXWindowsAttribute) as? [AXUIElement]) ?? []
        while let el = stack.popLast(), out.count < 20000 {
            out.append(el)
            stack.append(contentsOf: (attr(el, kAXChildrenAttribute) as? [AXUIElement]) ?? [])
        }
        return out
    }
    guard let target = elements().first(where: {
        (attr($0, kAXRoleAttribute) as? String) == args[3] && label($0).hasPrefix(args[4])
    }) else { fail("no \(args[3]) labelled \(args[4])") }
    let start = DispatchTime.now().uptimeNanoseconds
    guard AXUIElementPerformAction(target, kAXPressAction as CFString) == .success else { fail("press failed") }
    var polls = 0
    while ms(since: start) < timeout {
        polls += 1
        var heading = false
        var text = false
        for el in elements() {
            let role = attr(el, kAXRoleAttribute) as? String
            if role == "AXHeading", label(el) == args[5] { heading = true }
            if role == "AXStaticText", (attr(el, kAXValueAttribute) as? String)?.hasPrefix(args[6]) == true { text = true }
            if heading && text { break }
        }
        if heading && text {
            emit(["ms": ms(since: start), "polls": polls])
            exit(0)
        }
        usleep(2_000)
    }
    emit(["ms": NSNull(), "polls": polls])
    exit(2)
default:
    fail("unknown command \(args[1])")
}
