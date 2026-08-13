//! Bounded, seeded fuzz over the capture contract: random worktree mutations,
//! each followed by the full differential assertion. Deterministic (fixed
//! seed, hand-rolled LCG) so failures replay exactly.

use ff_testsupport::Fixture;
use ff_testsupport::capture::assert_snapshot_matches;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        // Numerical Recipes constants; plenty for fixture shuffling.
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

#[test]
fn random_mutation_rounds_stay_in_contract() {
    let fx = Fixture::new();
    let mut rng = Lcg(0xf0f0_5eed);
    let mut paths: Vec<String> = Vec::new();

    for round in 0..40 {
        // 1–3 mutations per round, then one capture + differential check.
        for _ in 0..(1 + rng.below(3)) {
            match rng.below(5) {
                // Write a fresh file (sometimes nested).
                0 => {
                    let path = if rng.below(2) == 0 {
                        format!("f{}.txt", rng.below(1000))
                    } else {
                        format!("d{}/f{}.txt", rng.below(5), rng.below(1000))
                    };
                    fx.write(
                        &path,
                        &format!("round {round} content {}\n", rng.below(1000)),
                    );
                    if !paths.contains(&path) {
                        paths.push(path);
                    }
                }
                // Append to an existing file.
                1 if !paths.is_empty() => {
                    let path = &paths[rng.below(paths.len() as u64) as usize];
                    let full = fx.path().join(path);
                    if full.exists() {
                        let mut content = std::fs::read_to_string(&full).unwrap();
                        content.push_str(&format!("appended {}\n", rng.below(1000)));
                        std::fs::write(&full, content).unwrap();
                    }
                }
                // Delete a file.
                2 if !paths.is_empty() => {
                    let i = rng.below(paths.len() as u64) as usize;
                    let path = paths[i].clone();
                    if fx.path().join(&path).exists() {
                        fx.remove(&path);
                    }
                }
                // Stage a random subset.
                3 if !paths.is_empty() => {
                    let path = &paths[rng.below(paths.len() as u64) as usize];
                    if fx.path().join(path).exists() {
                        fx.git(&["add", "--", path]);
                    }
                }
                // Commit everything.
                4 => {
                    fx.commit(&format!("commit round {round}"));
                }
                _ => {}
            }
        }
        assert_snapshot_matches(&fx);
    }
}
