// Fixture-only metadata ACL renewal. No password-item data APIs are used.
// Compile privately; root must inspect and explicitly run with --apply.
#include <Security/Security.h>
#include <Security/SecACL.h>
#include <Security/SecAccess.h>
#include <Security/SecKeychainSearch.h>
#include <Security/cssmtype.h>
#include <CoreFoundation/CoreFoundation.h>
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define DB "/Users/admin/Library/Application Support/io.cdot.airs-desktop.fixture/fixture-credentials.keychain-db"
#define APP "/Users/cdot/development/redtail/p09-queue/refinement-20261104/tooling/targets/core/aarch64-apple-darwin/release/bundle/macos/Prisma AIRS Desktop Fixture.app"
#define SERVICE "io.cdot.airs-desktop.fixture.scan"
#define ACCOUNT "cr-bf6eae1f4e49ee61"
#define PASSWORD "prisma-airs-desktop-v2-fixture-keychain" /* public fixture constant */
#define CDHASH "a579105e69b36d05fc8e5f66760688e60b8bb4c2"

typedef OSStatus (*SetAccessWithPassword)(SecKeychainItemRef, SecAccessRef, UInt32, const void *);
typedef struct {
    CFArrayRef apps;
    CFStringRef description;
    SecKeychainPromptSelector prompt;
    CSSM_ACL_AUTHORIZATION_TAG tags[64];
    uint32_t count;
} Snapshot;

static void require(int ok, const char *message) {
    if (!ok) { fprintf(stderr, "REFUSED: %s\n", message); exit(1); }
}
static void check(OSStatus status, const char *step) {
    if (status) { fprintf(stderr, "FAILED: %s OSStatus=%d\n", step, (int)status); exit(1); }
}
#define CHECK(call) check((call), #call)
static int equal(CFTypeRef a, CFTypeRef b) { return a == b || (a && b && CFEqual(a, b)); }

static Snapshot snapshot(SecACLRef acl) {
    Snapshot s = {0};
    s.count = 64;
    CHECK(SecACLGetAuthorizations(acl, s.tags, &s.count));
    require(s.count <= 64, "too many ACL authorization tags");
    CHECK(SecACLCopyContents(acl, &s.apps, &s.description, &s.prompt));
    return s;
}
static int has_tag(const Snapshot *s, CSSM_ACL_AUTHORIZATION_TAG tag) {
    for (uint32_t i = 0; i < s->count; ++i) if (s->tags[i] == tag) return 1;
    return 0;
}
static void verify_metadata(SecAccessRef access, const Snapshot before[3],
                            CFIndex changed, CFArrayRef expected_apps) {
    CFArrayRef list = NULL;
    CHECK(SecAccessCopyACLList(access, &list));
    require(list && CFArrayGetCount(list) == 3, "ACL entry count changed");
    for (CFIndex i = 0; i < 3; ++i) {
        Snapshot now = snapshot((SecACLRef)CFArrayGetValueAtIndex(list, i));
        require(now.count == before[i].count &&
                memcmp(now.tags, before[i].tags, now.count * sizeof(now.tags[0])) == 0,
                "authorization tags changed");
        require(now.prompt == before[i].prompt &&
                equal(now.description, before[i].description), "ACL prompt or description changed");
        require(equal(now.apps, i == changed ? expected_apps : before[i].apps),
                "unexpected trusted-application metadata");
        if (now.apps) CFRelease(now.apps);
        if (now.description) CFRelease(now.description);
    }
    CFRelease(list);
}

int main(int argc, char **argv) {
    require(argc == 2 && strcmp(argv[1], "--apply") == 0, "explicit --apply is required");
    require(getuid() == 501 && geteuid() == 501, "only the VM admin UID501 may run this fixture tool");
    struct stat st;
    require(lstat(DB, &st) == 0 && S_ISREG(st.st_mode) && st.st_uid == getuid(),
            "explicit fixture DB must be an owned regular file");
    SetAccessWithPassword set_access = (SetAccessWithPassword)dlsym(RTLD_DEFAULT, "SecKeychainItemSetAccessWithPassword");
    require(set_access != NULL, "required Security ACL API unavailable");

    CFURLRef url = CFURLCreateFromFileSystemRepresentation(NULL, (const UInt8 *)APP, strlen(APP), true);
    require(url != NULL, "fixture application URL unavailable");
    CFBundleRef bundle = CFBundleCreate(NULL, url);
    require(bundle && equal(CFBundleGetIdentifier(bundle), CFSTR("io.cdot.airs-desktop.fixture")),
            "application is not the exact fixture namespace");
    SecStaticCodeRef code = NULL;
    CHECK(SecStaticCodeCreateWithPath(url, kSecCSDefaultFlags, &code));
    // This linker-ad-hoc fixture has no bundle resource seal. Root externally
    // binds the prepared full bundle/executable hashes before this tool runs.
    // Skip ONLY resources; do NOT use BasicValidateOnly (it also skips code).
    CHECK(SecStaticCodeCheckValidity(code,
          kSecCSCheckAllArchitectures | kSecCSDoNotValidateResources, NULL));
    CFDictionaryRef signing = NULL;
    CHECK(SecCodeCopySigningInformation(code, kSecCSSigningInformation, &signing));
    CFDataRef digest = (CFDataRef)CFDictionaryGetValue(signing, kSecCodeInfoUnique);
    require(digest && CFGetTypeID(digest) == CFDataGetTypeID() && CFDataGetLength(digest) == 20,
            "expected exact 20-byte ad-hoc CDHash");
    char hex[41] = {0};
    for (CFIndex i = 0; i < 20; ++i) snprintf(hex + 2*i, 3, "%02x", CFDataGetBytePtr(digest)[i]);
    require(strcmp(hex, CDHASH) == 0, "candidate CDHash differs from the reviewed new fixture");
    require(CFDictionaryGetValue(signing, kSecCodeInfoTeamIdentifier) == NULL, "expected ad-hoc fixture only");

    CHECK(SecKeychainSetUserInteractionAllowed(false));
    SecKeychainRef keychain = NULL;
    CHECK(SecKeychainOpen(DB, &keychain));
    CHECK(SecKeychainUnlock(keychain, (UInt32)strlen(PASSWORD), PASSWORD, true));
    SecKeychainAttribute attributes[] = {
        {kSecServiceItemAttr, (UInt32)strlen(SERVICE), (void *)SERVICE},
        {kSecAccountItemAttr, (UInt32)strlen(ACCOUNT), (void *)ACCOUNT}
    };
    SecKeychainAttributeList query = {2, attributes};
    SecKeychainSearchRef search = NULL;
    CHECK(SecKeychainSearchCreateFromAttributes(keychain, kSecGenericPasswordItemClass, &query, &search));
    SecKeychainItemRef item = NULL, extra = NULL;
    CHECK(SecKeychainSearchCopyNext(search, &item));
    require(SecKeychainSearchCopyNext(search, &extra) == errSecItemNotFound,
            "fixture profile item must be unique");
    SecAccessRef access = NULL;
    CHECK(SecKeychainItemCopyAccess(item, &access));
    CFArrayRef list = NULL;
    CHECK(SecAccessCopyACLList(access, &list));
    require(list && CFArrayGetCount(list) == 3, "expected the reviewed three ACL entries");
    Snapshot before[3];
    CFIndex target = -1;
    for (CFIndex i = 0; i < 3; ++i) {
        before[i] = snapshot((SecACLRef)CFArrayGetValueAtIndex(list, i));
        if (has_tag(&before[i], CSSM_ACL_AUTHORIZATION_DECRYPT)) {
            require(target == -1, "ambiguous decrypt ACL");
            target = i;
        }
    }
    require(target >= 0, "reviewed decrypt ACL missing");
    Snapshot *s = &before[target];
    require(s->count == 6 && has_tag(s, CSSM_ACL_AUTHORIZATION_DERIVE) &&
            has_tag(s, CSSM_ACL_AUTHORIZATION_EXPORT_CLEAR) && has_tag(s, CSSM_ACL_AUTHORIZATION_EXPORT_WRAPPED) &&
            has_tag(s, CSSM_ACL_AUTHORIZATION_MAC) && has_tag(s, CSSM_ACL_AUTHORIZATION_SIGN),
            "decrypt ACL authorizations differ from reviewed metadata");
    require(s->apps && CFArrayGetCount(s->apps) == 1 &&
            equal(s->description, CFSTR(SERVICE)), "expected one existing trusted app in fixture decrypt ACL");
    SecTrustedApplicationRef trusted = NULL;
    CHECK(SecTrustedApplicationCreateFromPath(APP, &trusted));
    CFMutableArrayRef apps = CFArrayCreateMutableCopy(NULL, 0, s->apps);
    require(apps != NULL, "cannot retain original trusted-application list");
    CFArrayAppendValue(apps, trusted);
    require(CFArrayGetCount(apps) == 2 && CFArrayGetValueAtIndex(apps, 0) == CFArrayGetValueAtIndex(s->apps, 0),
            "original trusted application was not retained");
    // This edits only the existing ACL in memory, with unchanged tags/prompt/description.
    CHECK(SecACLSetContents((SecACLRef)CFArrayGetValueAtIndex(list, target), apps, s->description, s->prompt));
    verify_metadata(access, before, target, apps);
    // The only persistent item mutation: ACL metadata, never item attributes/data.
    CHECK(set_access(item, access, (UInt32)strlen(PASSWORD), PASSWORD));
    SecAccessRef after = NULL;
    CHECK(SecKeychainItemCopyAccess(item, &after));
    verify_metadata(after, before, target, apps);
    printf("{\"status\":\"updated\",\"fixture_account\":\"%s\",\"service\":\"%s\",\"added_cdhash\":\"%s\",\"trusted_apps_before\":1,\"trusted_apps_after\":2,\"acl_entries\":3,\"item_values_read\":false}\n", ACCOUNT, SERVICE, CDHASH);
    return 0;
}
