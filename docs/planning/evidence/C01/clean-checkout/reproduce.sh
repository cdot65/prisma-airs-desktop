#!/bin/sh
# Clean-checkout reproduction: fresh Academy clone from origin, fresh v2 tree.
set -u
S="$1"; SRC=/Users/cdot/development/redtail/prisma-airs-desktop-v2
C="$S/clean"; rm -rf "$C"; mkdir -p "$C"
H="$HOME"; BASE="PATH=/usr/bin:/bin"
echo "== academy clone"; env -i HOME="$H" PATH=/usr/bin:/bin git clone --quiet --branch v2/desktop-presentation --single-branch ssh://git@git-ssh.cdot.io/cdot.io/ai-security-academy.git "$C/academy" && git -C "$C/academy" rev-parse HEAD
echo "== v2 tree"; mkdir -p "$C/v2"
(cd "$SRC" && git ls-files -z --cached --others --exclude-standard | grep -zv '^docs/planning/evidence/' | grep -zv '^docs/planning/reviews/' | xargs -0 -I{} rsync -R -a "{}" "$C/v2/")
cd "$C/v2" && env -i HOME="$H" PATH=/usr/bin:/bin git init -q || exit 1
# Same origin and branch tracking as the reviewed checkout (.git/config is part of the source fingerprint).
G="env -i HOME=$H PATH=/usr/bin:/bin git"
$G remote add origin ssh://git@git-ssh.cdot.io/cdot/prisma-airs-desktop-v2.git && $G config branch.main.remote origin && $G config branch.main.merge refs/heads/main || exit 1
cmp .git/config "$SRC/.git/config" && echo "git config identical to the reviewed checkout"
mkdir -p docs/planning/reviews/G01/attempt-05 && cp "$SRC/docs/planning/reviews/G01/attempt-05/receipt.json" docs/planning/reviews/G01/attempt-05/
ls -a | head -30
echo "== corepack"; mkdir -p .tools/bin && env -i HOME="$H" PATH="$H/.nvm/versions/node/v24.21.0/bin:/usr/bin:/bin" COREPACK_ENABLE_DOWNLOAD_PROMPT=0 corepack enable pnpm --install-directory .tools/bin
X="env -i HOME=$H USER=$USER LOGNAME=$LOGNAME TMPDIR=$TMPDIR LANG=en_US.UTF-8 PATH=$H/.cargo/bin:/usr/bin:/bin RUSTUP_TOOLCHAIN=1.99.0 CARGO_TERM_COLOR=never"
echo "== fetch-inputs"; $X cargo run -q --locked -p xtask -- fetch-inputs --source "$C/academy" || exit 1
echo "== check"; $X cargo run -q --locked -p xtask -- check --chunk C01 --manifest docs/architecture/checks.json; echo "check exit $?"
echo "== test"; $X cargo run -q --locked -p xtask -- test --chunk C01 --manifest docs/architecture/checks.json; echo "test exit $?"
