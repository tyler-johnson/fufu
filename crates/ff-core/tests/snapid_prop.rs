//! Seeded fuzz over the snapshot id spelling: encode/decode roundtrip on
//! random hex strings, output confined to k–z, and the two alphabets stay
//! disjoint. Deterministic (fixed seed, hand-rolled LCG) so failures replay.

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        // Numerical Recipes constants; plenty for string shuffling.
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
fn random_hex_roundtrips_within_disjoint_alphabet() {
    let mut rng = Lcg(0xf0f0_5eed);
    for _ in 0..500 {
        let len = 1 + rng.below(40) as usize;
        let hex: String = (0..len)
            .map(|_| char::from_digit(rng.below(16) as u32, 16).unwrap())
            .collect();

        let letters = ff_core::snapid::encode(&hex);
        assert_eq!(letters.len(), hex.len(), "bijective per character");
        assert!(
            letters.chars().all(|c| ('k'..='z').contains(&c)),
            "output confined to k–z: {letters:?}"
        );
        assert!(
            !letters.chars().any(|c| c.is_ascii_hexdigit()),
            "letters spelling shares no character with hex: {letters:?}"
        );
        assert!(ff_core::snapid::is_encoded(&letters));
        assert_eq!(
            ff_core::snapid::decode(&letters).as_deref(),
            Some(hex.as_str()),
            "roundtrip"
        );
    }
}
