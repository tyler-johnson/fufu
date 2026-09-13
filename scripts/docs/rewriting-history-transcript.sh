#!/usr/bin/env bash
# Authoritative commands and state assertions for rewriting-history.md.
source "$(dirname "${BASH_SOURCE[0]}")/guide-scene.sh"

if fresh reword; then
    run "printf 'feature\n' > app.txt"
    run 'ff commit -m wip'
    before=$(tip HEAD)
    tree=$(tip 'HEAD^{tree}')
    identity=$(git cat-file commit HEAD | grep '^change-id ')
    run 'ff describe HEAD -m "app: greet the reader"'
    [[ $(tip HEAD) != "$before" && $(tip 'HEAD^{tree}') == "$tree" ]]
    [[ $(git cat-file commit HEAD | grep '^change-id ') == "$identity" ]]
    end
fi

if fresh amend; then
    run "printf 'feature\n' > app.txt"
    run 'ff commit -m "app: feature"'
    run "printf 'reviewed feature\n' > app.txt"
    run "printf 'unrelated note\n' > notes.txt"
    run 'ff absorb app.txt'
    content HEAD:app.txt 'reviewed feature'
    absent_object HEAD:notes.txt
    [[ $(<notes.txt) == 'unrelated note' ]]
    run 'ff status'
    end
fi

if fresh partial; then
    run "printf 'feature\n' > app.txt"
    run "printf 'documentation\n' > notes.txt"
    run 'ff commit app.txt -m "app: feature"'
    absent_object HEAD:notes.txt
    run 'ff status'
    run 'ff commit -m "docs: feature notes"'
    content HEAD:notes.txt documentation
    end
fi

if fresh same-file; then
    run "printf 'first change\nsecond change\n' > app.txt"
    run 'ff trigger -m "complete two-part edit"'
    saved=$(op)
    run "printf 'first change\n' > app.txt"
    run 'ff commit -m "app: first change"'
    run "ff restore app.txt --at-op ${saved:0:12}"
    run 'ff commit -m "app: second change"'
    content HEAD~:app.txt 'first change'
    content HEAD:app.txt $'first change\nsecond change'
    end
fi

if fresh split; then
    run "printf 'feature\n' > app.txt"
    run "printf 'documentation\n' > notes.txt"
    run 'ff commit -m "app: feature and notes"'
    identity=$(git cat-file commit HEAD | grep '^change-id ')
    before=$(tip HEAD)
    run 'ff lift notes.txt'
    [[ $(tip HEAD) != "$before" && $(git cat-file commit HEAD | grep '^change-id ') == "$identity" ]]
    absent_object HEAD:notes.txt
    [[ $(<notes.txt) == documentation ]]
    run 'ff commit -m "docs: feature notes"'
    content HEAD~:app.txt feature
    content HEAD:notes.txt documentation
    end
fi

if fresh combine; then
    run "printf 'number\n' > app.txt"
    run 'ff commit -m "app: numbers"'
    run "printf 'signed\n' >> app.txt"
    run 'ff commit -m "app: signed numbers"'
    run "printf 'float\n' >> app.txt"
    run 'ff commit -m "app: floats"'
    tree=$(tip 'HEAD^{tree}')
    run 'ff log -r HEAD~2..HEAD'
    run 'ff absorb --from HEAD~2..HEAD -m "app: signed and floating numbers"'
    [[ $(tip 'HEAD^{tree}') == "$tree" && $(git rev-list --count main..HEAD) == 1 ]]
    end
fi

if fresh lift-range; then
    run "printf 'feature\n' > app.txt"
    run 'ff commit -m "app: feature"'
    run "printf 'documentation\n' > notes.txt"
    run 'ff commit -m "docs: feature notes"'
    run 'ff lift --from HEAD~2..HEAD'
    [[ $(tip HEAD) == "$(tip main)" && $(<app.txt) == feature && $(<notes.txt) == documentation ]]
    run 'ff commit app.txt -m "app: feature"'
    run 'ff commit notes.txt -m "docs: feature notes"'
    end
fi

if fresh edit; then
    run "printf 'feature\n' > app.txt"
    run 'ff commit -m "app: feature"'
    run "printf 'documentation\n' > notes.txt"
    run 'ff commit -m "docs: feature notes"'
    run 'ff edit HEAD~1'
    run "printf 'reviewed feature\n' > app.txt"
    run 'ff done'
    [[ $(git branch --show-current) == feature && $(<app.txt) == 'reviewed feature' && $(<notes.txt) == documentation ]]
    content HEAD~:app.txt 'reviewed feature'
    end
fi

if fresh abandon; then
    run "printf 'feature\n' > app.txt"
    run 'ff commit -m "app: feature"'
    before=$(tip HEAD)
    run 'ff edit HEAD'
    run "printf 'experiment\n' > app.txt"
    run 'ff done --abandon'
    [[ $(tip HEAD) == "$before" && $(<app.txt) == feature ]]
    absent_ref refs/stash
    run 'ff undo'
    [[ $(<app.txt) == experiment && $(git branch --show-current) != feature ]]
    end
fi

if fresh published; then
    remote
    run "printf 'feature\n' > app.txt"
    run 'ff commit -m "app: feature"'
    run 'ff push'
    remote_before=$(git --git-dir="$CASE/origin.git" rev-parse feature)
    run 'ff describe HEAD -m "app: reviewed feature"'
    [[ $(git --git-dir="$CASE/origin.git" rev-parse feature) == "$remote_before" ]]
    run 'ff push'
    [[ $(git --git-dir="$CASE/origin.git" rev-parse feature) == "$(tip HEAD)" && $(tip HEAD) != "$remote_before" ]]
    end
fi
