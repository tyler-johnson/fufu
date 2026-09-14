#!/usr/bin/env bash
# Authoritative commands and state assertions for stacked-changes.md.
source "$(dirname "${BASH_SOURCE[0]}")/guide-scene.sh"

if fresh stack; then
    remote
    run 'ff switch main -b parser-core'
    run "printf 'parser core\n' > parser.txt"
    run 'ff commit -m "parser: core"'
    run 'ff switch parser-core -b parser-cli'
    run "printf 'parser command\n' > cli.txt"
    run 'ff commit -m "cli: parser command"'
    run 'ff log'
    end

    printf '<!-- transcript:feedback -->\n```console\n'
    run 'ff switch parser-core'
    run "printf 'reviewed parser core\n' > parser.txt"
    run 'ff absorb'
    content parser-cli:parser.txt 'reviewed parser core'
    git merge-base --is-ancestor parser-core parser-cli
    run 'ff log -r parser-cli'
    end

    printf '<!-- transcript:pull-stack -->\n```console\n'
    run "printf 'teammate documentation\n' > ../teammate/notes.txt"
    run 'git -C ../teammate add notes.txt'
    run 'git -C ../teammate commit -qm "docs: teammate notes"'
    run 'git -C ../teammate push -q origin main'
    run 'ff pull'
    content parser-cli:notes.txt 'teammate documentation'
    git merge-base --is-ancestor main parser-core
    git merge-base --is-ancestor parser-core parser-cli
    end

    printf '<!-- transcript:push-stack -->\n```console\n'
    run 'ff switch main'
    core_saved=$(tip refs/fufu/snap/parser-core)
    cli_saved=$(tip refs/fufu/snap/parser-cli)
    run 'ff push parser-core parser-cli'
    [[ $(git --git-dir="$CASE/origin.git" rev-parse parser-core) == "$(tip parser-core)" ]]
    [[ $(git --git-dir="$CASE/origin.git" rev-parse parser-cli) == "$(tip parser-cli)" ]]
    # Check saved state before switching can append another operation.
    [[ $(tip refs/fufu/snap/parser-core) == "$core_saved" ]]
    [[ $(tip refs/fufu/snap/parser-cli) == "$cli_saved" ]]
    absent_ref refs/fufu/open/parser-core
    absent_ref refs/fufu/open/parser-cli
    end

    printf '<!-- transcript:after-named-push -->\n```console\n'
    run 'ff switch parser-cli'
    run 'ff status'
    [[ -z $(git status --porcelain) ]]
    [[ $(<cli.txt) == 'parser command' && $(<parser.txt) == 'reviewed parser core' ]]
    run 'ff switch parser-core'
    [[ -z $(git status --porcelain) ]]
    [[ $(<parser.txt) == 'reviewed parser core' ]]
    run 'ff switch main'
    end

    printf '<!-- transcript:merged -->\n```console\n'
    run 'git -C ../teammate fetch -q origin'
    run 'git -C ../teammate merge -q --ff-only origin/parser-core'
    run 'git -C ../teammate push -q origin main'
    run 'ff pull main'
    run 'ff restack parser-cli --onto main'
    git merge-base --is-ancestor main parser-cli
    [[ $(git rev-list --count main..parser-cli) == 1 ]]
    python3 -c 'import json; assert json.load(open(".git/fufu/branch/parser-cli"))["parent"] == "main"'
    run 'ff push parser-cli'
    end
fi

if fresh collision; then
    run "printf 'parser greeting\n' > app.txt"
    run 'ff commit -m "app: parser greeting"'
    run 'ff switch main -b renamer'
    run "printf 'renamed greeting\n' > app.txt"
    run "printf 'rename notes\n' > notes.txt"
    run 'ff commit -m "app: rename greeting and add notes"'
    run 'ff collide feature'
    [[ "$LAST" == *'✕ feature'* ]]
    run 'ff lift app.txt'
    run 'ff collide feature'
    verdict=$(ff collide feature --json)
    python3 -c 'import json, sys; data = json.load(sys.stdin)["data"]; assert data["a"]["open"] and data["pairing"] == {"kind": "collide", "paths": ["app.txt"]}' <<<"$verdict"
    run 'ff restore app.txt'
    run 'ff collide feature'
    [[ "$LAST" == *'✓ feature'* ]]
    content HEAD:notes.txt 'rename notes'
    end
fi
