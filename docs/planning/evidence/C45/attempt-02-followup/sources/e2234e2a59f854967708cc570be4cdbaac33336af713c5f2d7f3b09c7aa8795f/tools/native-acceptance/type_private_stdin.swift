// A private credential is read only from stdin and posted only to the named
// frontmost process. No clipboard, command-line secret, file or echoed value.
import AppKit
import ApplicationServices
import Foundation

func refuse() -> Never {
    FileHandle.standardError.write(Data("private input refused\n".utf8))
    exit(1)
}

guard CommandLine.arguments.count == 2,
      let pid = pid_t(CommandLine.arguments[1]),
      NSWorkspace.shared.frontmostApplication?.processIdentifier == pid else { refuse() }
let data = FileHandle.standardInput.readDataToEndOfFile()
guard !data.isEmpty, data.count <= 4096,
      let text = String(data: data, encoding: .utf8),
      !text.contains("\n"), !text.contains("\r") else { refuse() }
let source = CGEventSource(stateID: .hidSystemState)
for character in text {
    guard NSWorkspace.shared.frontmostApplication?.processIdentifier == pid,
          let down = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: true),
          let up = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: false) else { refuse() }
    let units = Array(String(character).utf16)
    units.withUnsafeBufferPointer {
        down.keyboardSetUnicodeString(stringLength: units.count, unicodeString: $0.baseAddress)
    }
    down.postToPid(pid)
    up.postToPid(pid)
    usleep(10_000)
}
