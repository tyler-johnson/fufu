#!/usr/bin/env bash
# Authoritative commands and state assertions for recovery.md. Optional recipe ID.
source "$(dirname "${BASH_SOURCE[0]}")/guide-scene.sh"

if fresh reset; then
    run "printf 'feature\n' > app.txt"
    run 'ff commit -m "app: feature"'
    before=$(tip HEAD)
    run 'git reset --hard HEAD~1'
    run 'ff undo'
    [[ $(tip HEAD) == "$before" && $(<app.txt) == feature ]]
    end
fi

if fresh file; then
    run "printf 'wrong\n' > app.txt"
    before=$(tip HEAD)
    index=$(git write-tree)
    run 'ff trigger -m "before discarding the edit"'
    saved=$(op)
    run 'ff restore app.txt'
    [[ $(<app.txt) == hello && $(tip HEAD) == "$before" && $(git write-tree) == "$index" ]]
    run "ff restore app.txt --at-op ${saved:0:12}"
    [[ $(<app.txt) == wrong ]]
    run 'ff restore app.txt --from main'
    [[ $(<app.txt) == hello ]]
    end
fi

if fresh past-files; then
    run "printf 'keep this draft\n' > app.txt"
    run 'ff trigger -m "known good draft"'
    good=$(op)
    # Date lookup must distinguish the good capture from the later bad one.
    run 'saved_time=$(date -u +%Y-%m-%dT%H:%M:%SZ)'
    run 'sleep 2'
    run "printf 'bad refactor\n' > app.txt"
    run 'ff commit -m "app: bad refactor"'
    before=$(tip HEAD)
    index=$(git write-tree)
    run 'ff restore --all --at "$saved_time"'
    [[ $(<app.txt) == 'keep this draft' && $(tip HEAD) == "$before" && $(git write-tree) == "$index" ]]
    run "ff restore app.txt --at-op ${good:0:12}"
    [[ $(<app.txt) == 'keep this draft' ]]
    end
fi

if fresh state; then
    run "printf 'good\n' > app.txt"
    run 'ff commit -m "app: good version"'
    good=$(op)
    before=$(tip HEAD)
    run "printf 'bad\n' > app.txt"
    run 'ff commit -m "app: wrong direction"'
    run 'ff history'
    run "ff op show ${good:0:12}"
    run "ff op restore ${good:0:12}"
    [[ $(tip HEAD) == "$before" && $(<app.txt) == good ]]
    end
fi

if fresh redo; then
    run "printf 'feature\n' > app.txt"
    run 'ff commit -m "app: feature"'
    before=$(tip HEAD)
    run 'ff undo'
    run 'ff redo'
    [[ $(tip HEAD) == "$before" ]]
    old=$(op)
    run 'ff undo'
    run 'ff commit -m "app: better message"'
    run 'ff redo' 1
    run "ff op log --at-op ${old:0:12} -n 3"
    run "ff op restore ${old:0:12}"
    [[ $(tip HEAD) == "$before" ]]
    end
fi

if fresh revert; then
    base=$(tip HEAD)
    run "printf 'wrong branch work\n' > app.txt"
    run 'ff commit -m "app: unwanted commit"'
    wrong=$(op)
    run 'ff switch main -b docs'
    run "printf 'useful later work\n' > notes.txt"
    run 'ff commit -m "docs: useful notes"'
    later=$(tip HEAD)
    index=$(git write-tree)
    run 'ff op log -n 6'
    run "ff op revert ${wrong:0:12}"
    [[ $(tip feature) == "$base" && $(tip HEAD) == "$later" && $(<notes.txt) == 'useful later work' && $(git write-tree) == "$index" ]]
    run 'ff undo'
    [[ $(tip feature) != "$base" ]]
    end
fi

if fresh revert-refusal; then
    run "printf 'first\n' > app.txt"
    run 'ff commit -m "app: first"'
    wrong=$(op)
    run "printf 'later\n' > app.txt"
    run 'ff commit -m "app: later"'
    before=$(tip HEAD)
    run 'ff op log -n 4'
    run "ff op revert ${wrong:0:12}" 3
    [[ $(tip HEAD) == "$before" && $(<app.txt) == later ]]
    end
fi

if fresh wrong-branch; then
    run "printf 'feature\n' > app.txt"
    run 'ff commit -m "app: feature"'
    run 'ff undo'
    run 'ff commit -b correct-branch -m "app: feature"'
    [[ $(git branch --show-current) == correct-branch ]]
    content feature:app.txt hello
    content HEAD:app.txt feature
    end
fi

if fresh session-undo; then
    run "printf 'feature\n' > app.txt"
    run 'ff commit -m "app: feature"'
    run 'ff edit HEAD'
    session=$(git branch --show-current)
    run 'ff history'
    run 'ff undo'
    [[ $(git branch --show-current) == feature ]]
    git show-ref --verify --quiet "refs/heads/$session"
    run 'ff undo'
    absent_ref "refs/heads/$session"
    end
fi

if fresh resolution-undo; then
    run "printf 'feature greeting\n' > app.txt"
    run 'ff commit -m "app: feature greeting"'
    run 'ff switch main'
    run "printf 'main greeting\n' > app.txt"
    run 'ff commit -m "app: main greeting"'
    run 'ff switch feature'
    run 'ff restack' 3
    run 'ff resolve'
    session=$(git branch --show-current)
    [[ $(<app.txt) == *'<<<<<<<'* ]]
    run 'ff undo'
    [[ $(git branch --show-current) == feature && $(<app.txt) == 'feature greeting' ]]
    git show-ref --verify --quiet "refs/heads/$session"
    run 'ff undo'
    absent_ref "refs/heads/$session"
    # The original held request remains; opening the session is what was undone.
    python3 -c 'import json; assert json.load(open(".git/fufu/branch/feature"))["held"]'
    end
fi

if fresh retention; then
    run "printf 'retained in branch history\n' > app.txt"
    run 'ff commit -m "app: keep this commit"'
    before=$(tip HEAD)
    run 'ff config keep 2s'
    run 'sleep 3'
    run 'ff switch main'
    run 'ff switch feature'
    run 'ff op trim -n'
    run 'ff op trim'
    [[ $(tip HEAD) == "$before" && $(<app.txt) == 'retained in branch history' ]]
    run 'ff history'
    run 'ff config keep 90d'
    end
fi

if fresh force-push; then
    remote
    run "printf 'feature\n' > app.txt"
    run 'ff commit -m "app: feature"'
    run 'ff push'
    run 'git -C ../teammate fetch -q origin'
    run 'git -C ../teammate switch -q feature'
    run 'git -C ../teammate commit -q --amend -m "app: reviewed feature"'
    run 'git -C ../teammate push -q --force origin feature'
    run "printf 'local follow-up\n' > notes.txt"
    run 'ff commit -m "docs: follow-up"'
    before=$(tip HEAD)
    run 'ff push' 1
    [[ $(tip HEAD) == "$before" ]]
    run 'ff pull'
    [[ $(<notes.txt) == 'local follow-up' ]]
    run 'ff push'
    [[ $(git --git-dir="$CASE/origin.git" rev-parse feature) == "$(tip HEAD)" ]]
    end
fi
