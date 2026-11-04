// Minimal macOS Accessibility helper for native acceptance (compiled once
// by the acceptance script with swiftc). It reads the real window's AX tree
// and performs AX actions; keystrokes are sent separately through System
// Events so input arrives as real keyboard events.
//
// Usage:
//   ax tree  <pid>                          JSON list of nodes in the app's windows
//   ax press <pid> <role> <label> [nth]     AXPress the nth matching element
//   ax focus <pid> <role> <label> [nth]     set AXFocused on the nth match
//   ax raise <pid>                          raise the main window
//   ax frame <pid>                          JSON [x,y,width,height] of owned window
//   ax resize <pid> <width> <height> [x y]   resize/reposition that owned window
//   ax type-file <pid> <file> [units]       type the file's text into the focused
//                                           field as key events posted to <pid>,
//                                           [units] UTF-16 units per event
//                                           (default 20; up to 30000 pastes)
// A node matches when its role equals <role> and its label (title,
// description or string value) starts with <label>. Secure text fields
// report the value the OS exposes for them (so evidence can show the key is
// not exposed).

import AppKit
import ApplicationServices
import Foundation

func attr(_ el: AXUIElement, _ name: String) -> AnyObject? {
    var value: AnyObject?
    return AXUIElementCopyAttributeValue(el, name as CFString, &value) == .success ? value : nil
}

func string(_ el: AXUIElement, _ name: String) -> String? {
    attr(el, name) as? String
}

func point(_ el: AXUIElement) -> [Double]? {
    guard let raw = attr(el, kAXPositionAttribute) else { return nil }
    var p = CGPoint.zero
    guard AXValueGetValue(raw as! AXValue, .cgPoint, &p) else { return nil }
    return [Double(p.x), Double(p.y)]
}

func size(_ el: AXUIElement) -> [Double]? {
    guard let raw = attr(el, kAXSizeAttribute) else { return nil }
    var s = CGSize.zero
    guard AXValueGetValue(raw as! AXValue, .cgSize, &s) else { return nil }
    return [Double(s.width), Double(s.height)]
}

func children(_ el: AXUIElement) -> [AXUIElement] {
    (attr(el, kAXChildrenAttribute) as? [AXUIElement]) ?? []
}

func label(_ el: AXUIElement) -> String {
    for name in [kAXTitleAttribute, kAXDescriptionAttribute] {
        if let s = string(el, name), !s.isEmpty { return s }
    }
    if let s = string(el, kAXValueAttribute) { return s }
    return ""
}

struct Node {
    let el: AXUIElement
    let depth: Int
}

func owningPid(_ el: AXUIElement) -> pid_t? {
    var pid: pid_t = 0
    return AXUIElementGetPid(el, &pid) == .success ? pid : nil
}

// BEGIN owned-window source selection (also compiled by the isolated policy probe).
func appWindows(
    _ app: AXUIElement, _ pid: pid_t,
    read: (AXUIElement, String) -> AnyObject? = attr,
    owner: (AXUIElement) -> pid_t? = owningPid
) -> [AXUIElement] {
    guard let listed = read(app, kAXWindowsAttribute) as? [AXUIElement] else { return [] }
    if !listed.isEmpty { return listed }
    guard pid > 0 else { return [] }
    // Some WKWebView application states expose a valid Main/Focused window
    // while AXWindows succeeds with an empty array. Stay in this app's AX
    // attributes; never substitute another process or a system-wide window.
    var fallback: [AXUIElement] = []
    for name in [kAXMainWindowAttribute, kAXFocusedWindowAttribute] {
        guard let raw = read(app, name), CFGetTypeID(raw) == AXUIElementGetTypeID() else { continue }
        let window = raw as! AXUIElement
        guard owner(window) == pid, read(window, kAXRoleAttribute) as? String == kAXWindowRole else { continue }
        if !fallback.contains(where: { CFEqual($0, window) }) { fallback.append(window) }
    }
    return fallback
}
// END owned-window source selection.

// BEGIN owned-window geometry policy (compiled by the isolated policy probe).
func ownedWindow(
    _ windows: [AXUIElement], _ pid: pid_t,
    read: (AXUIElement, String) -> AnyObject? = attr,
    owner: (AXUIElement) -> pid_t? = owningPid
) -> AXUIElement? {
    guard pid > 0 else { return nil }
    return windows.first { owner($0) == pid && read($0, kAXRoleAttribute) as? String == kAXWindowRole }
}

func validFrame(_ values: [Double]) -> Bool {
    values.count == 4 && values.allSatisfy { $0.isFinite } && values[2] > 0 && values[3] > 0
}

func resizeValues(_ values: [String]) -> [Double]? {
    guard values.count == 2 || values.count == 4 else { return nil }
    let numbers = values.compactMap(Double.init)
    guard numbers.count == values.count, numbers.allSatisfy({ $0.isFinite }),
          numbers[0] > 0, numbers[1] > 0 else { return nil }
    return numbers
}
// END owned-window geometry policy.

func windowFrame(_ window: AXUIElement) -> [Double]? {
    guard let rawPosition = attr(window, kAXPositionAttribute),
          let rawSize = attr(window, kAXSizeAttribute),
          CFGetTypeID(rawPosition) == AXValueGetTypeID(),
          CFGetTypeID(rawSize) == AXValueGetTypeID() else { return nil }
    let positionValue = rawPosition as! AXValue
    let sizeValue = rawSize as! AXValue
    var position = CGPoint.zero
    var dimensions = CGSize.zero
    guard AXValueGetType(positionValue) == .cgPoint, AXValueGetType(sizeValue) == .cgSize,
          AXValueGetValue(positionValue, .cgPoint, &position),
          AXValueGetValue(sizeValue, .cgSize, &dimensions) else { return nil }
    let result = [Double(position.x), Double(position.y), Double(dimensions.width), Double(dimensions.height)]
    return validFrame(result) ? result : nil
}

func setWindowAttribute(_ window: AXUIElement, _ name: String, _ value: AXValue) {
    var settable = DarwinBoolean(false)
    guard AXUIElementIsAttributeSettable(window, name as CFString, &settable) == .success,
          settable.boolValue else { fail("owned window attribute is not settable: \(name)") }
    let status = AXUIElementSetAttributeValue(window, name as CFString, value)
    guard status == .success else { fail("owned window attribute failed: \(name) (\(status.rawValue))") }
}

func walk(_ pid: pid_t) -> [Node] {
    let app = AXUIElementCreateApplication(pid)
    // WebKit builds a web page's accessibility tree only for an AX client
    // that announces itself (as assistive technologies do).
    AXUIElementSetAttributeValue(app, "AXEnhancedUserInterface" as CFString, kCFBooleanTrue)
    var out: [Node] = []
    func visit(_ el: AXUIElement, _ depth: Int, panel: Bool = false) {
        if out.count > 20000 || depth > 60 { return }
        out.append(Node(el: el, depth: depth))
        // Native file panels contain remote folder browsers. Describing their
        // rows can stall AX for minutes and reads unrelated operator filenames.
        // Keep actual panel controls (including nested Go to Folder sheets),
        // with their roles, depths, geometry and focus. WebView trees retain
        // their full traversal whenever no native sheet is present.
        if panel && string(el, kAXRoleAttribute) == "AXScrollArea" { return }
        for child in children(el) { visit(child, depth + 1, panel: panel) }
    }
    for window in appWindows(app, pid) {
        let sheets = children(window).filter { string($0, kAXRoleAttribute) == "AXSheet" }
        if sheets.isEmpty {
            visit(window, 0)
        } else {
            out.append(Node(el: window, depth: 0))
            for sheet in sheets { visit(sheet, 1, panel: true) }
        }
    }
    return out
}

func describe(_ node: Node) -> [String: Any] {
    let el = node.el
    var d: [String: Any] = ["depth": node.depth]
    d["role"] = string(el, kAXRoleAttribute) ?? NSNull()
    d["subrole"] = string(el, kAXSubroleAttribute) ?? NSNull()
    d["label"] = label(el)
    d["title"] = string(el, kAXTitleAttribute) ?? NSNull()
    d["description"] = string(el, kAXDescriptionAttribute) ?? NSNull()
    if let value = attr(el, kAXValueAttribute) {
        if let s = value as? String { d["value"] = s }
        else if let n = value as? NSNumber { d["value"] = n }
    }
    d["focused"] = (attr(el, kAXFocusedAttribute) as? Bool) ?? false
    d["enabled"] = (attr(el, kAXEnabledAttribute) as? Bool) ?? true
    d["current"] = string(el, "AXARIACurrent") ?? NSNull()
    if let url = attr(el, kAXURLAttribute) as? URL { d["url"] = url.absoluteString }
    d["pos"] = point(el) ?? NSNull()
    d["size"] = size(el) ?? NSNull()
    return d
}

func matches(_ pid: pid_t, _ role: String, _ prefix: String) -> [AXUIElement] {
    walk(pid).map(\.el).filter { string($0, kAXRoleAttribute) == role && label($0).hasPrefix(prefix) }
}

func fail(_ message: String) -> Never {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(1)
}

let args = CommandLine.arguments
guard args.count >= 3, let pid = pid_t(args[2]) else { fail("usage: ax tree|press|focus|raise <pid> ...") }
switch args[1] {
case "tree":
    let nodes = walk(pid).map(describe)
    let data = try! JSONSerialization.data(withJSONObject: nodes, options: [])
    FileHandle.standardOutput.write(data)
case "frame", "resize":
    guard args[1] != "frame" || args.count == 3 else { fail("frame needs only <pid>") }
    let values = args[1] == "resize" ? resizeValues(Array(args.dropFirst(3))) : nil
    guard args[1] != "resize" || values != nil else { fail("resize needs positive finite <width> <height> [finite x y]") }
    let app = AXUIElementCreateApplication(pid)
    guard let window = ownedWindow(appWindows(app, pid), pid) else { fail("no AXWindow owned by requested PID") }
    if let values = values {
        if values.count == 4 {
            var position = CGPoint(x: values[2], y: values[3])
            guard let encoded = AXValueCreate(.cgPoint, &position) else { fail("cannot encode owned window position") }
            setWindowAttribute(window, kAXPositionAttribute, encoded)
        }
        var dimensions = CGSize(width: values[0], height: values[1])
        guard let encoded = AXValueCreate(.cgSize, &dimensions) else { fail("cannot encode owned window size") }
        setWindowAttribute(window, kAXSizeAttribute, encoded)
    }
    guard let frame = windowFrame(window) else { fail("owned window has no finite positive frame") }
    FileHandle.standardOutput.write(try! JSONSerialization.data(withJSONObject: frame, options: []))
case "press", "focus":
    guard args.count >= 5 else { fail("\(args[1]) needs <role> <label>") }
    let nth = args.count >= 6 ? Int(args[5]) ?? 0 : 0
    let found = matches(pid, args[3], args[4])
    guard nth < found.count else { fail("no \(args[3]) labelled \(args[4]) (\(found.count) found)") }
    let el = found[nth]
    let status = args[1] == "press"
        ? AXUIElementPerformAction(el, kAXPressAction as CFString)
        : AXUIElementSetAttributeValue(el, kAXFocusedAttribute as CFString, kCFBooleanTrue)
    guard status == .success else { fail("\(args[1]) failed: \(status.rawValue)") }
    print("ok")
case "type-file":
    // Real keyboard events posted to this process only (CGEventPostToPid, so
    // they can reach no other application), each carrying up to 20 UTF-16
    // units of the file's text into the focused field. Used for inputs too
    // large to type through System Events; the caller confirms the app is
    // frontmost and verifies what arrived.
    guard args.count >= 4, let text = try? String(contentsOfFile: args[3], encoding: .utf8) else { fail("type-file needs <file> [units per event]") }
    // 20 UTF-16 units per event mimics fast typing; larger events (WebKit
    // truncates one event at 36,864 units) insert like a paste.
    let perEvent = args.count >= 5 ? Int(args[4]) ?? 20 : 20
    let source = CGEventSource(stateID: .hidSystemState)
    var chunk: [UniChar] = []
    func post(_ units: [UniChar]) {
        guard let down = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: true),
              let up = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: false) else { fail("cannot create key events") }
        units.withUnsafeBufferPointer { down.keyboardSetUnicodeString(stringLength: units.count, unicodeString: $0.baseAddress) }
        down.postToPid(pid)
        up.postToPid(pid)
        // Large (paste-like) events get time to be applied before the next.
        usleep(units.count > 20 ? 500_000 : 1_000)
    }
    for character in text {
        let units = Array(String(character).utf16)
        if chunk.count + units.count > perEvent { post(chunk); chunk = [] }
        chunk.append(contentsOf: units)
    }
    if !chunk.isEmpty { post(chunk) }
    print("ok")
case "raise":
    let app = AXUIElementCreateApplication(pid)
    if let window = (attr(app, kAXWindowsAttribute) as? [AXUIElement])?.first {
        AXUIElementPerformAction(window, kAXRaiseAction as CFString)
    }
    NSRunningApplication(processIdentifier: pid)?.activate()
    print("ok")
default:
    fail("unknown command \(args[1])")
}
