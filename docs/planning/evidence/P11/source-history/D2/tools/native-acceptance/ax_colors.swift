// Reads the text colors WebKit exposes through accessibility for the fixture
// window's web area, without screen capture. It asks the AXWebArea for an
// attributed string over its whole text-marker range and reports the distinct
// foreground/background colors with sample text. With a needle, it reports
// the colors of the first run containing that text instead.
// Usage: swift ax_colors.swift <pid> [needle]
import AppKit
import ApplicationServices

func attr(_ el: AXUIElement, _ name: String) -> AnyObject? {
    var value: AnyObject?
    return AXUIElementCopyAttributeValue(el, name as CFString, &value) == .success ? value : nil
}

func param(_ el: AXUIElement, _ name: String, _ arg: AnyObject) -> AnyObject? {
    var value: AnyObject?
    return AXUIElementCopyParameterizedAttributeValue(el, name as CFString, arg, &value) == .success ? value : nil
}

func hex(_ color: Any?) -> String? {
    guard let c = color, CFGetTypeID(c as CFTypeRef) == CGColor.typeID else { return nil }
    guard let rgb = NSColor(cgColor: c as! CGColor)?.usingColorSpace(.sRGB) else { return nil }
    return String(format: "#%02x%02x%02x", Int(round(rgb.redComponent * 255)), Int(round(rgb.greenComponent * 255)), Int(round(rgb.blueComponent * 255)))
}

let pid = pid_t(CommandLine.arguments[1])!
let app = AXUIElementCreateApplication(pid)
var stack: [AXUIElement] = (attr(app, kAXWindowsAttribute) as? [AXUIElement]) ?? []
var web: AXUIElement?
while let el = stack.popLast() {
    if (attr(el, kAXRoleAttribute) as? String) == "AXWebArea" { web = el; break }
    stack.append(contentsOf: (attr(el, kAXChildrenAttribute) as? [AXUIElement]) ?? [])
}
guard let area = web,
      let start = attr(area, "AXStartTextMarker"), let end = attr(area, "AXEndTextMarker"),
      let range = param(area, "AXTextMarkerRangeForUnorderedTextMarkers", [start, end] as CFArray),
      let text = param(area, "AXAttributedStringForTextMarkerRange", range) as? NSAttributedString else {
    print("{\"error\":\"web area attributed text unavailable\"}")
    exit(1)
}
if CommandLine.arguments.count > 2 {
    let needle = CommandLine.arguments[2]
    let found = (text.string as NSString).range(of: needle)
    var result: [String: Any] = ["needle": needle, "found": found.location != NSNotFound]
    if found.location != NSNotFound {
        let attrs = text.attributes(at: found.location, effectiveRange: nil)
        result["foreground"] = hex(attrs[NSAttributedString.Key("AXForegroundColor")]) ?? "none"
        result["background"] = hex(attrs[NSAttributedString.Key("AXBackgroundColor")]) ?? "none"
    }
    let data = try! JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys])
    print(String(data: data, encoding: .utf8)!)
    exit(0)
}
var runs: [String: [String: Any]] = [:]
text.enumerateAttributes(in: NSRange(location: 0, length: text.length)) { attrs, r, _ in
    let fg = hex(attrs[NSAttributedString.Key("AXForegroundColor")]) ?? "none"
    let bg = hex(attrs[NSAttributedString.Key("AXBackgroundColor")]) ?? "none"
    let key = "\(fg)|\(bg)"
    let sample = (text.string as NSString).substring(with: r).trimmingCharacters(in: .whitespacesAndNewlines)
    if runs[key] == nil && !sample.isEmpty { runs[key] = ["foreground": fg, "background": bg, "sample": String(sample.prefix(40))] }
}
let data = try! JSONSerialization.data(withJSONObject: Array(runs.values).sorted { "\($0)" < "\($1)" }, options: [.prettyPrinted, .sortedKeys])
print(String(data: data, encoding: .utf8)!)
