//! Match statistics: Elo estimates with confidence intervals, likelihood of superiority,
//! and the sequential probability ratio test (SPRT) used to accept or reject a change.
//!
//! **Model.** Scores map to Elo through the logistic curve `E(d) = 1 / (1 + 10^(-d/400))`.
//! Intervals and the SPRT use a normal approximation to the mean score, with the variance
//! estimated from the data. Two variance models:
//!
//! - **trinomial**: each game is an independent win/draw/loss;
//! - **pentanomial**: games come in pairs (one opening, colours swapped) and each pair's
//!   total (0, ½, 1, 1½ or 2 points) is the unit. Paired games are correlated through the
//!   shared opening, so pentanomial is the honest model for paired matches. It usually
//!   gives a narrower interval, because the opening's bias cancels within a pair.
//!
//! The SPRT is the generalized SPRT (GSPRT) approximation used by modern engine-testing
//! frameworks (after M. Van den Bergh): `LLR ≈ n · (s1 − s0) · (2s̄ − s0 − s1) / (2σ̂²)`,
//! where `s0 = E(elo0)`, `s1 = E(elo1)`, and `s̄`, `σ̂²` are the per-unit sample mean and
//! variance.
//!
//! None of this makes a small match informative. The interval is reported with every
//! estimate so that it can't be forgotten.

/// Expected score for a rating difference of `elo`.
pub fn expected_score(elo: f64) -> f64 {
    1.0 / (1.0 + 10f64.powf(-elo / 400.0))
}

/// Rating difference implied by a score fraction in (0, 1). Infinite at 0 and 1.
pub fn elo_from_score(score: f64) -> f64 {
    if score <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if score >= 1.0 {
        return f64::INFINITY;
    }
    -400.0 * (1.0 / score - 1.0).log10()
}

/// Error function, Abramowitz & Stegun 7.1.26 (|error| < 1.5e-7). The standard library
/// has no erf, and this precision is far beyond what match statistics need.
pub fn erf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736 + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    sign * (1.0 - poly * (-x * x).exp())
}

/// Two-sided 95% normal quantile.
const Z95: f64 = 1.959_963_985;

/// An Elo estimate with its 95% confidence interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EloEstimate {
    pub elo: f64,
    pub lower: f64,
    pub upper: f64,
    /// Mean score per game, in [0, 1].
    pub score: f64,
}

impl EloEstimate {
    fn from_mean_and_stderr(mean: f64, stderr: f64) -> Self {
        let clamp = |s: f64| s.clamp(1e-9, 1.0 - 1e-9);
        Self {
            elo: elo_from_score(mean),
            lower: elo_from_score(clamp(mean - Z95 * stderr)),
            upper: elo_from_score(clamp(mean + Z95 * stderr)),
            score: mean,
        }
    }

    /// Half-width of the interval, for "+X ± Y" reporting.
    pub fn margin(&self) -> f64 {
        (self.upper - self.lower) / 2.0
    }
}

/// Win/draw/loss counts from one side's point of view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Trinomial {
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
}

impl Trinomial {
    pub fn games(&self) -> u32 {
        self.wins + self.draws + self.losses
    }

    pub fn score(&self) -> f64 {
        (f64::from(self.wins) + 0.5 * f64::from(self.draws)) / f64::from(self.games())
    }

    /// Per-game variance of the score.
    pub fn variance(&self) -> f64 {
        let n = f64::from(self.games());
        let s = self.score();
        (f64::from(self.wins) * (1.0 - s).powi(2)
            + f64::from(self.draws) * (0.5 - s).powi(2)
            + f64::from(self.losses) * s.powi(2))
            / n
    }

    pub fn elo(&self) -> Option<EloEstimate> {
        (self.games() > 0).then(|| {
            EloEstimate::from_mean_and_stderr(
                self.score(),
                (self.variance() / f64::from(self.games())).sqrt(),
            )
        })
    }

    /// Likelihood of superiority: the probability that the true Elo difference is positive,
    /// from decisive games only (draws carry no information about who is stronger).
    pub fn los(&self) -> f64 {
        let decisive = f64::from(self.wins + self.losses);
        if decisive == 0.0 {
            return 0.5;
        }
        0.5 * (1.0 + erf((f64::from(self.wins) - f64::from(self.losses)) / (2.0 * decisive).sqrt()))
    }
}

/// Counts of game pairs by the pair's total score: index `i` holds pairs worth `i/2` points
/// out of 2 (0, ½, 1, 1½, 2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pentanomial(pub [u32; 5]);

impl Pentanomial {
    pub fn pairs(&self) -> u32 {
        self.0.iter().sum()
    }

    /// Record a pair from the two game scores (each 0, 0.5 or 1).
    pub fn add_pair(&mut self, first: f64, second: f64) {
        let index = ((first + second) * 2.0).round() as usize;
        self.0[index.min(4)] += 1;
    }

    /// Mean score per game.
    pub fn score(&self) -> f64 {
        let total: f64 = self
            .0
            .iter()
            .enumerate()
            .map(|(i, &n)| f64::from(n) * i as f64 / 4.0)
            .sum();
        total / f64::from(self.pairs())
    }

    /// Variance of the per-pair mean score.
    pub fn variance(&self) -> f64 {
        let s = self.score();
        let sum: f64 = self
            .0
            .iter()
            .enumerate()
            .map(|(i, &n)| f64::from(n) * (i as f64 / 4.0 - s).powi(2))
            .sum();
        sum / f64::from(self.pairs())
    }

    pub fn elo(&self) -> Option<EloEstimate> {
        (self.pairs() > 0).then(|| {
            EloEstimate::from_mean_and_stderr(
                self.score(),
                (self.variance() / f64::from(self.pairs())).sqrt(),
            )
        })
    }
}

/// Parameters of a sequential probability ratio test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sprt {
    /// H0: the change is worth `elo0` (usually 0: "no better").
    pub elo0: f64,
    /// H1: the change is worth `elo1` (the smallest gain worth detecting).
    pub elo1: f64,
    /// False-positive rate: accepting H1 when H0 is true.
    pub alpha: f64,
    /// False-negative rate: accepting H0 when H1 is true.
    pub beta: f64,
}

/// The state of an SPRT after the data so far.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SprtVerdict {
    AcceptH1,
    AcceptH0,
    Continue,
}

impl Sprt {
    /// Lower and upper log-likelihood-ratio bounds.
    pub fn bounds(&self) -> (f64, f64) {
        (
            (self.beta / (1.0 - self.alpha)).ln(),
            ((1.0 - self.beta) / self.alpha).ln(),
        )
    }

    /// GSPRT log-likelihood ratio from `n` units with mean score `mean` and per-unit
    /// variance `variance`. Returns 0 when the variance is zero (e.g. only draws so far):
    /// such data cannot tell the hypotheses apart.
    pub fn llr(&self, n: f64, mean: f64, variance: f64) -> f64 {
        if n <= 0.0 || variance <= 0.0 {
            return 0.0;
        }
        let s0 = expected_score(self.elo0);
        let s1 = expected_score(self.elo1);
        n * (s1 - s0) * (2.0 * mean - s0 - s1) / (2.0 * variance)
    }

    pub fn llr_pentanomial(&self, p: &Pentanomial) -> f64 {
        if p.pairs() == 0 {
            return 0.0;
        }
        self.llr(f64::from(p.pairs()), p.score(), p.variance())
    }

    pub fn llr_trinomial(&self, t: &Trinomial) -> f64 {
        if t.games() == 0 {
            return 0.0;
        }
        self.llr(f64::from(t.games()), t.score(), t.variance())
    }

    pub fn verdict(&self, llr: f64) -> SprtVerdict {
        let (lower, upper) = self.bounds();
        if llr >= upper {
            SprtVerdict::AcceptH1
        } else if llr <= lower {
            SprtVerdict::AcceptH0
        } else {
            SprtVerdict::Continue
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn elo_and_score_are_inverse_and_symmetric() {
        assert!(close(expected_score(0.0), 0.5, 1e-12));
        // 0.75 -> -400 log10(1/3) = 190.848...
        assert!(close(elo_from_score(0.75), 190.848_501_9, 1e-6));
        for elo in [-800.0, -120.5, -1.0, 0.0, 3.0, 57.0, 400.0] {
            assert!(close(elo_from_score(expected_score(elo)), elo, 1e-9), "{elo}");
            assert!(close(expected_score(-elo), 1.0 - expected_score(elo), 1e-12));
        }
        assert_eq!(elo_from_score(1.0), f64::INFINITY);
        assert_eq!(elo_from_score(0.0), f64::NEG_INFINITY);
    }

    #[test]
    fn erf_matches_reference_values() {
        for (x, want) in [(0.0, 0.0), (0.5, 0.520_499_877_8), (1.0, 0.842_700_792_9), (2.0, 0.995_322_265_0)] {
            assert!(close(erf(x), want, 2e-7), "erf({x}) = {}", erf(x));
            assert!(close(erf(-x), -want, 2e-7));
        }
    }

    #[test]
    fn trinomial_estimate() {
        let even = Trinomial { wins: 30, draws: 40, losses: 30 };
        let e = even.elo().unwrap();
        assert!(close(e.elo, 0.0, 1e-9));
        assert!(close(e.upper, -e.lower, 1e-9), "symmetric interval");
        assert!(close(even.los(), 0.5, 2e-7), "within erf's documented error");

        // 60 wins, 20 draws, 20 losses: score 0.7.
        let strong = Trinomial { wins: 60, draws: 20, losses: 20 };
        assert!(close(strong.score(), 0.7, 1e-12));
        // variance = (60*0.09 + 20*0.04 + 20*0.49)/100 = 0.16
        assert!(close(strong.variance(), 0.16, 1e-12));
        let e = strong.elo().unwrap();
        assert!(e.lower > 0.0 && e.elo > e.lower && e.upper > e.elo);
        assert!(strong.los() > 0.999);
    }

    #[test]
    fn more_games_narrow_the_interval() {
        let small = Trinomial { wins: 6, draws: 8, losses: 6 }.elo().unwrap();
        let large = Trinomial { wins: 600, draws: 800, losses: 600 }.elo().unwrap();
        assert!(large.margin() < small.margin() / 5.0);
    }

    #[test]
    fn pentanomial_estimate() {
        let mut p = Pentanomial::default();
        p.add_pair(1.0, 0.0); // 1 point  -> index 2
        p.add_pair(1.0, 0.5); // 1.5      -> index 3
        p.add_pair(0.5, 0.5); // 1        -> index 2
        p.add_pair(0.0, 0.0); // 0        -> index 0
        assert_eq!(p.0, [1, 0, 2, 1, 0]);
        // per-game mean = (0 + 0.5 + 0.5 + 0.75)/4 = 0.4375
        assert!(close(p.score(), 0.4375, 1e-12));
        assert!(p.elo().unwrap().elo < 0.0);
    }

    #[test]
    fn sprt_llr_formula_and_bounds() {
        let sprt = Sprt { elo0: 0.0, elo1: 10.0, alpha: 0.05, beta: 0.05 };
        let (lower, upper) = sprt.bounds();
        assert!(close(lower, -2.944_438_979, 1e-8));
        assert!(close(upper, 2.944_438_979, 1e-8));
        // Hand-computed: s0 = 0.5, s1 = E(10) = 0.514387..., n = 1000, mean 0.53, var 0.05.
        let s1 = expected_score(10.0);
        let want = 1000.0 * (s1 - 0.5) * (2.0 * 0.53 - 0.5 - s1) / (2.0 * 0.05);
        assert!(close(sprt.llr(1000.0, 0.53, 0.05), want, 1e-9));
        assert!(want > 6.0 && want < 7.0);
        assert_eq!(sprt.verdict(want), SprtVerdict::AcceptH1);
        assert_eq!(sprt.verdict(-3.0), SprtVerdict::AcceptH0);
        assert_eq!(sprt.verdict(0.0), SprtVerdict::Continue);
    }

    #[test]
    fn sprt_is_neutral_without_information() {
        let sprt = Sprt { elo0: 0.0, elo1: 5.0, alpha: 0.05, beta: 0.05 };
        let all_draws = Pentanomial([0, 0, 50, 0, 0]);
        assert_eq!(sprt.llr_pentanomial(&all_draws), 0.0);
        assert_eq!(sprt.llr_pentanomial(&Pentanomial::default()), 0.0);
        // Data exactly between the hypotheses gives an LLR of zero.
        let mid = (expected_score(0.0) + expected_score(5.0)) / 2.0;
        assert!(close(sprt.llr(500.0, mid, 0.1), 0.0, 1e-12));
    }

    /// Simulate matches from known true strengths with a deterministic RNG. The SPRT must
    /// accept H1 for a clearly better engine and H0 for an equal one, and the 95% interval
    /// must cover the truth in about 95% of runs. This is where the implementation meets
    /// its own claims.
    #[test]
    fn simulated_matches_behave_as_advertised() {
        struct Rng(u64);
        impl Rng {
            fn unit(&mut self) -> f64 {
                self.0 ^= self.0 << 13;
                self.0 ^= self.0 >> 7;
                self.0 ^= self.0 << 17;
                (self.0 >> 11) as f64 / (1u64 << 53) as f64
            }
        }
        // One game with win/draw probabilities chosen to give the target expected score.
        fn game(rng: &mut Rng, true_elo: f64, draw_rate: f64) -> f64 {
            let s = expected_score(true_elo);
            let win = s - draw_rate / 2.0;
            let u = rng.unit();
            if u < win { 1.0 } else if u < win + draw_rate { 0.5 } else { 0.0 }
        }
        let sprt = Sprt { elo0: 0.0, elo1: 20.0, alpha: 0.05, beta: 0.05 };
        let mut rng = Rng(0x1234_5678_9ABC_DEF1);
        let mut run = |true_elo: f64| {
            let mut p = Pentanomial::default();
            for _ in 0..20_000 {
                let (a, b) = (game(&mut rng, true_elo, 0.4), game(&mut rng, true_elo, 0.4));
                p.add_pair(a, b);
                match sprt.verdict(sprt.llr_pentanomial(&p)) {
                    SprtVerdict::Continue => {}
                    v => return v,
                }
            }
            SprtVerdict::Continue
        };
        let better: Vec<_> = (0..20).map(|_| run(40.0)).collect();
        let equal: Vec<_> = (0..20).map(|_| run(0.0)).collect();
        let h1 = better.iter().filter(|v| **v == SprtVerdict::AcceptH1).count();
        let h0 = equal.iter().filter(|v| **v == SprtVerdict::AcceptH0).count();
        assert!(h1 >= 18, "a +40 Elo engine passed only {h1}/20 SPRTs");
        assert!(h0 >= 18, "an equal engine was rejected only {h0}/20 times");

        let mut covered = 0;
        for _ in 0..200 {
            let mut t = Trinomial::default();
            for _ in 0..400 {
                match game(&mut rng, 30.0, 0.4) {
                    s if s == 1.0 => t.wins += 1,
                    s if s == 0.5 => t.draws += 1,
                    _ => t.losses += 1,
                }
            }
            let e = t.elo().unwrap();
            covered += usize::from(e.lower <= 30.0 && 30.0 <= e.upper);
        }
        assert!((180..=198).contains(&covered), "95% interval covered the truth {covered}/200 times");
    }
}
