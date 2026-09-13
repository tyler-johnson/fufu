#!/usr/bin/env bash
# Authoritative commands and state assertions for plain-git-teammates.md.
source "$(dirname "${BASH_SOURCE[0]}")/guide-scene.sh"

if fresh teammate; then
    remote
    run "printf 'feature\n' > app.txt"
    run 'ff commit -m "app: feature"'
    run 'ff push'
    run 'git -C ../teammate fetch -q origin'
    run 'git -C ../teammate switch -q feature'
    run 'git -C ../teammate log --oneline -2'
    run 'git -C ../teammate status --short'
    [[ $(git -C ../teammate rev-parse HEAD) == "$(tip HEAD)" ]]
    [[ -z $(git -C ../teammate for-each-ref refs/fufu/) ]]
    end
fi

if fresh parked; then
    run "printf 'tuning pass\n' > app.txt"
    run 'ff describe -m "app: tuning pass"'
    run 'ff switch main'
    run 'git log refs/fufu/open/feature -1 --oneline'
    parked=$(tip refs/fufu/open/feature)
    run 'git log refs/fufu/wt/main/ops -1 --oneline'
    run 'git cherry-pick -n refs/fufu/open/feature'
    [[ $(<app.txt) == 'tuning pass' && $(tip HEAD) == "$(tip main)" ]]
    [[ $(tip refs/fufu/open/feature) == "$parked" ]]
    end
fi

if fresh outside; then
    run "printf 'IDE edit\n' > app.txt"
    run 'git commit -am "app: IDE edit"'
    before=$(tip HEAD)
    run 'ff status'
    [[ "$LAST" == *'change made outside fufu'* && $(tip HEAD) == "$before" ]]
    end
fi

if fresh passthrough; then
    run "printf 'feature\n' > app.txt"
    run 'ff commit -m "app: feature"'
    before=$(tip HEAD)
    run "printf 'uncommitted draft\n' > app.txt"
    run 'ff git reset --hard HEAD~1'
    run 'ff undo'
    [[ $(tip HEAD) == "$before" && $(<app.txt) == 'uncommitted draft' ]]
    end
fi

if fresh policy; then
    run 'ff config gitPolicy strict'
    before=$(tip HEAD)
    run 'ff git commit -m wip' 2
    [[ $(tip HEAD) == "$before" ]]
    run 'ff git log --oneline -1'
    end
fi
