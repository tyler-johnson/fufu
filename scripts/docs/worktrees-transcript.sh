#!/usr/bin/env bash
# Authoritative commands and state assertions for worktrees.md.
source "$(dirname "${BASH_SOURCE[0]}")/guide-scene.sh"

if fresh independent; then
    run 'ff worktree ../review'
    run "printf 'review draft\n' > ../review/review.txt"
    run 'ff -C ../review status'
    run 'ff -C ../review commit -m "review: notes"'
    run "printf 'independent feature\n' > app.txt"
    run 'ff commit -m "app: independent feature"'
    before=$(tip HEAD)
    run 'ff -C ../review undo'
    [[ $(tip HEAD) == "$before" && $(<../review/review.txt) == 'review draft' ]]
    absent_object review:review.txt
    run 'ff -C ../review history'
    end
fi

if fresh resume; then
    run 'ff worktree ../review'
    run "printf 'unfinished idea\n' > idea.txt"
    run 'ff switch main'
    run 'ff -C ../review switch feature'
    [[ $(<../review/idea.txt) == 'unfinished idea' && ! -e idea.txt ]]
    run 'ff switch feature' 1
    run 'ff -C ../review switch review'
    run 'ff switch feature'
    [[ $(<idea.txt) == 'unfinished idea' ]]
    end
fi

if fresh removal; then
    run 'ff worktree ../review'
    run "printf 'unfinished review\n' > ../review/review.txt"
    run 'ff worktree -d ../review'
    [[ "$LAST" =~ captured\ first\ as\ ([0-9a-f]+) ]]
    capture=${BASH_REMATCH[1]}
    [[ ! -d ../review ]]
    run 'ff worktree'
    [[ "$LAST" == *"$capture"* ]]
    run "ff restore review.txt --at-op $capture"
    [[ $(<review.txt) == 'unfinished review' ]]
    end
fi

if fresh removal-undo; then
    run 'ff worktree ../review'
    run "printf 'unfinished review\n' > ../review/review.txt"
    run 'ff worktree -d ../review'
    run 'ff undo'
    [[ $(<../review/review.txt) == 'unfinished review' ]]
    end
fi

if fresh watch; then
    run 'ff worktree ../review'
    run 'ff watch --all -n 2'
    [[ "$LAST" == *'"worktree":"review"'* && "$LAST" == *'"worktree":"main"'* ]]
    [[ $(printf '%s\n' "$LAST" | wc -l) == 2 ]]
    end
fi
