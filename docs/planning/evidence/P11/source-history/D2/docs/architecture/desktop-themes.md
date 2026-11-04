# Desktop color themes

The desktop owns two independent preferences: appearance (`system`, `light`,
`dark`) and color theme (`prismaAirs`, `gruvbox`). Prisma AIRS is the default.
Neither preference updates an Academy account or unlocks an earned cosmetic.

Existing state without `colorTheme` defaults to Prisma AIRS and retains its
appearance. The default is omitted when saving; an alternate theme is saved as
an explicit field. Invalid theme values remain a storage error, rather than
silently overwriting the preference. Existing atomic storage writes apply.

Older binaries use strict field validation and can reject state written with a
new preference. Opening an upgraded data directory with an older release is
unsupported; this change does not claim downgrade compatibility or silently
reset data. Preserve the state before any explicitly requested downgrade.

The document's general Academy theme continues to follow appearance. Desktop
palette tokens are independent, and shared presentation colors are adapted only
inside desktop theme scopes. Academy content and its portals keep the general
Academy palette. Earned Academy themes remain under Academy's existing rules.
