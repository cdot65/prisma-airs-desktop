// Owner-authorized correction of this one existing local reference. The new
// secret arrives only on stdin. No value is echoed or placed in process args.
import Foundation
import Security

func stop(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}

var incoming = FileHandle.standardInput.readDataToEndOfFile()
defer { incoming.resetBytes(in: incoming.startIndex..<incoming.endIndex) }
guard !incoming.isEmpty, incoming.count <= 16 * 1024 else { stop("invalid private input") }
var login: SecKeychain?
let loginPath = NSHomeDirectory() + "/Library/Keychains/login.keychain-db"
guard SecKeychainOpen(loginPath, &login) == errSecSuccess, let login else { stop("login Keychain unavailable") }
let query: [String: Any] = [
    kSecClass as String: kSecClassGenericPassword,
    kSecAttrService as String: "io.cdot.prisma-airs-desktop.v2.live",
    kSecAttrAccount as String: "mgmt-vulture-client-secret",
    kSecMatchSearchList as String: [login],
    kSecMatchLimit as String: kSecMatchLimitOne,
    kSecReturnRef as String: true,
]
var found: CFTypeRef?
guard SecItemCopyMatching(query as CFDictionary, &found) == errSecSuccess,
      let found, CFGetTypeID(found) == SecKeychainItemGetTypeID() else { stop("existing approved reference unavailable") }
let item = found as! SecKeychainItem
let status = incoming.withUnsafeBytes {
    SecKeychainItemModifyContent(item, nil, UInt32(incoming.count), $0.baseAddress)
}
guard status == errSecSuccess else { stop("approved reference update failed") }
print("updated approved reference")
