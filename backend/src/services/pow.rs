//! Proof-of-work challenges: the site's "are you a person?" step.
//!
//! The server hands out a signed challenge; the client has to find a counter whose
//! SHA-256 (together with the challenge's nonce) has a run of leading zero bits. The
//! work is asymmetric: solving costs the client milliseconds-to-seconds, checking
//! costs the server one hash.
//!
//! What this actually buys, stated plainly: a browser-driven or scripted mass
//! registration has to spend real CPU per attempt, and a naive bot that just POSTs
//! the form is stopped outright. It is **not** a defence against an attacker who is
//! willing to run a native or GPU solver — one hash per attempt is cheap for them.
//! It exists because it needs no third party, works without outbound network access
//! (this deployment is behind a network where several CAPTCHA providers are
//! unreachable), and can be layered with a hosted CAPTCHA later.
//!
//! The challenge is a JWT signed with the deployment's `JWT_SECRET`, so it needs no
//! server-side storage to issue, cannot be forged, and expires on its own. Only the
//! *solved* nonces are remembered, to make a solution single-use.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode as jwt_decode, encode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::Config;

/// How long a handed-out challenge stays valid.
pub const CHALLENGE_TTL_SECS: i64 = 300;

/// Leading zero bits required by default.
///
/// 14 bits is ~16k hashes: a fraction of a second in a desktop browser, around a
/// second on a phone, and the client loop yields between hashes so the page keeps
/// painting. Trading UX against brute force is a deployment decision: raise it if
/// abuse appears (`POW_DIFFICULTY`), and remember each extra bit doubles the client's
/// work while barely touching an attacker's.
pub const DEFAULT_DIFFICULTY: u32 = 14;

/// Upper bound, so a misconfigured deployment cannot hand out a challenge that no
/// browser could ever solve (or a difficulty that overflows the counter space).
pub const MAX_DIFFICULTY: u32 = 28;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Claims {
    /// Random, single-use, and what the client hashes against.
    nonce: String,
    difficulty: u32,
    exp: usize,
    kind: String,
}

/// What the client receives.
#[derive(Debug, Clone, Serialize)]
pub struct Challenge {
    /// Opaque signed token; the client echoes it back untouched.
    pub challenge: String,
    pub difficulty: u32,
    pub expires_in_secs: i64,
}

/// What the client sends back.
#[derive(Debug, Clone, Deserialize)]
pub struct Solution {
    pub challenge: String,
    pub answer: String,
}

/// Why a solution was refused. All of these are the caller's problem, but they are
/// kept apart so the logs can tell "someone is guessing" from "someone is replaying".
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// Malformed, unsigned, expired, or of the wrong kind.
    Invalid,
    /// Correctly signed but does not meet the difficulty.
    Unsolved,
    /// Already spent.
    Replayed,
}

impl Refused {
    pub fn message(&self) -> &'static str {
        match self {
            Refused::Invalid => "the verification challenge is invalid or has expired",
            Refused::Unsolved => "the verification challenge was not solved",
            Refused::Replayed => "this verification challenge has already been used",
        }
    }
}

/// Signs a fresh challenge.
pub fn issue(cfg: &Config, difficulty: u32) -> Result<Challenge, jsonwebtoken::errors::Error> {
    let difficulty = difficulty.clamp(1, MAX_DIFFICULTY);
    let nonce = crate::services::password_reset::generate_token();
    let claims = Claims {
        nonce,
        difficulty,
        exp: (Utc::now() + Duration::seconds(CHALLENGE_TTL_SECS)).timestamp() as usize,
        kind: "pow".to_owned(),
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(cfg.jwt_secret.as_bytes()),
    )?;
    Ok(Challenge {
        challenge: token,
        difficulty,
        expires_in_secs: CHALLENGE_TTL_SECS,
    })
}

/// The nonce and expiry of a well-formed challenge, without checking the solution.
fn decode(cfg: &Config, token: &str) -> Result<Claims, Refused> {
    // `Validation::default()` allows 60s of leeway, which is the right call for
    // session tokens (clock skew between servers) but wrong here: a challenge is
    // handed out and solved within seconds, and "expired five seconds ago" should
    // mean expired.
    let mut validation = Validation::default();
    validation.leeway = 0;
    let data = jwt_decode::<Claims>(
        token,
        &DecodingKey::from_secret(cfg.jwt_secret.as_bytes()),
        &validation,
    )
    .map_err(|_| Refused::Invalid)?;
    if data.claims.kind != "pow" {
        return Err(Refused::Invalid);
    }
    Ok(data.claims)
}

/// Verify a solution and spend its nonce.
///
/// Order matters: the cheap cryptographic checks and the work check come first, and
/// the nonce is only recorded once the work has been proven, so a wrong answer cannot
/// burn a challenge.
pub fn verify(cfg: &Config, store: &spent::Spent, solution: &Solution) -> Result<(), Refused> {
    let claims = decode(cfg, &solution.challenge)?;

    let bits = leading_zero_bits(&claims.nonce, &solution.answer);
    if bits < claims.difficulty {
        return Err(Refused::Unsolved);
    }

    if !store.claim(&claims.nonce, claims.exp as i64) {
        return Err(Refused::Replayed);
    }
    Ok(())
}

/// Leading zero bits of `sha256(nonce || answer)`.
///
/// Exposed so the tests can assert the same number the client is looking for.
pub fn leading_zero_bits(nonce: &str, answer: &str) -> u32 {
    let mut hasher = Sha256::new();
    hasher.update(nonce.as_bytes());
    hasher.update(answer.as_bytes());
    let digest = hasher.finalize();

    let mut bits = 0;
    for byte in digest {
        if byte == 0 {
            bits += 8;
            continue;
        }
        bits += byte.leading_zeros();
        break;
    }
    bits
}

/// A reference solver. The browser does the same thing in JavaScript; the tests and
/// the E2E helpers use this so they exercise the real protocol rather than a bypass.
pub fn solve(nonce: &str, difficulty: u32) -> Option<String> {
    // The counter space is far larger than any realistic difficulty needs.
    for counter in 0u64..(1 << 32) {
        let answer = counter.to_string();
        if leading_zero_bits(nonce, &answer) >= difficulty {
            return Some(answer);
        }
    }
    None
}

/// Nonces that have already been spent, with their expiry so the map cannot grow
/// without bound on a long-running process.
pub mod spent {
    use super::{HashMap, Mutex};

    #[derive(Debug, Default)]
    pub struct Spent {
        seen: Mutex<HashMap<String, i64>>,
    }

    impl Spent {
        pub fn new() -> Self {
            Self::default()
        }

        /// Record `nonce` as used. Returns `false` if it was already spent.
        pub fn claim(&self, nonce: &str, expires_at: i64) -> bool {
            let now = chrono::Utc::now().timestamp();
            let mut seen = self.seen.lock().expect("spent-challenge map poisoned");
            // Pruning on write keeps this O(1) amortised and needs no background task.
            seen.retain(|_, exp| *exp > now);
            if seen.contains_key(nonce) {
                return false;
            }
            seen.insert(nonce.to_owned(), expires_at);
            true
        }

        /// For tests and diagnostics.
        pub fn len(&self) -> usize {
            self.seen
                .lock()
                .expect("spent-challenge map poisoned")
                .len()
        }

        pub fn is_empty(&self) -> bool {
            self.len() == 0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn cfg() -> Config {
        let mut cfg = crate::config::Config::from_env();
        cfg.jwt_secret = "test-secret-for-pow-only".into();
        cfg
    }

    fn solution_for(cfg: &Config, difficulty: u32) -> (String, Solution) {
        let challenge = issue(cfg, difficulty).unwrap();
        let claims = decode(cfg, &challenge.challenge).unwrap();
        let answer = solve(&claims.nonce, difficulty).expect("solvable");
        (
            claims.nonce.clone(),
            Solution {
                challenge: challenge.challenge.clone(),
                answer,
            },
        )
    }

    #[test]
    fn a_solved_challenge_is_accepted_once() {
        let cfg = cfg();
        let store = spent::Spent::new();
        let (_, solution) = solution_for(&cfg, 10);

        assert_eq!(verify(&cfg, &store, &solution), Ok(()));
        // The same solution cannot be replayed.
        assert_eq!(verify(&cfg, &store, &solution), Err(Refused::Replayed));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn a_wrong_answer_is_refused_without_burning_the_challenge() {
        let cfg = cfg();
        let store = spent::Spent::new();
        let challenge = issue(&cfg, 12).unwrap();
        let wrong = Solution {
            challenge: challenge.challenge.clone(),
            answer: "not-a-solution".into(),
        };

        assert_eq!(verify(&cfg, &store, &wrong), Err(Refused::Unsolved));
        // Nothing recorded: the honest client can still submit the right answer.
        assert!(store.is_empty());

        let claims = decode(&cfg, &challenge.challenge).unwrap();
        let right = Solution {
            challenge: challenge.challenge.clone(),
            answer: solve(&claims.nonce, 12).unwrap(),
        };
        assert_eq!(verify(&cfg, &store, &right), Ok(()));
    }

    #[test]
    fn forged_and_foreign_tokens_are_invalid() {
        let cfg = cfg();
        let store = spent::Spent::new();

        // A challenge signed with another key.
        let mut other = cfg.clone();
        other.jwt_secret = "a-different-secret".into();
        let foreign = issue(&other, 8).unwrap();
        assert_eq!(
            verify(
                &cfg,
                &store,
                &Solution {
                    challenge: foreign.challenge,
                    answer: "0".into()
                }
            ),
            Err(Refused::Invalid)
        );

        // Garbage, and an empty token.
        for token in ["", "not.a.jwt", "a.b.c"] {
            assert_eq!(
                verify(
                    &cfg,
                    &store,
                    &Solution {
                        challenge: token.into(),
                        answer: "0".into()
                    }
                ),
                Err(Refused::Invalid)
            );
        }
    }

    /// A token that is valid for `require_auth` must not be usable as a challenge:
    /// otherwise an access token could be turned into a free "solved" pass.
    #[test]
    fn an_access_token_is_not_a_challenge() {
        let cfg = cfg();
        let store = spent::Spent::new();
        let access = crate::services::auth::issue(
            &cfg,
            1,
            "alice",
            crate::models::user::Role::User,
            0,
            "access",
        )
        .unwrap();

        assert_eq!(
            verify(
                &cfg,
                &store,
                &Solution {
                    challenge: access,
                    answer: "0".into()
                }
            ),
            Err(Refused::Invalid)
        );
    }

    #[test]
    fn difficulty_bounds_are_enforced() {
        let cfg = cfg();
        // Too high a difficulty would be unsolvable in practice; it is clamped rather
        // than handed out.
        let absurd = issue(&cfg, 999).unwrap();
        assert_eq!(absurd.difficulty, MAX_DIFFICULTY);
        let zero = issue(&cfg, 0).unwrap();
        assert_eq!(zero.difficulty, 1);
    }

    #[test]
    fn leading_zero_bits_counts_the_way_the_client_expects() {
        // sha256("" || "0") = 5feceb66ffc86f38d9... whose first byte is 0x5f
        // (0101_1111) — one leading zero bit.
        assert_eq!(leading_zero_bits("", "0"), 1);
        // And sha256("nonce" || "0") starts 0x64 (0110_0100) — also one.
        assert_eq!(leading_zero_bits("nonce", "0"), 1);
        // A digest that starts with a zero byte scores at least 8.
        let mut found = 0;
        for counter in 0..1000u32 {
            if leading_zero_bits("nonce", &counter.to_string()) >= 8 {
                found += 1;
            }
        }
        // ~1/256 of a thousand tries: non-zero, and nowhere near all of them.
        assert!(found > 0, "expected a few 8-bit solutions in 1000 tries");
        assert!(found < 50, "8 bits should not be that common: {found}");
    }

    #[test]
    fn the_reference_solver_meets_the_requested_difficulty() {
        for difficulty in [1, 4, 8, 12] {
            let nonce = crate::services::password_reset::generate_token();
            let answer = solve(&nonce, difficulty).expect("solvable");
            assert!(
                leading_zero_bits(&nonce, &answer) >= difficulty,
                "solver returned {answer} for difficulty {difficulty}"
            );
        }
    }

    #[test]
    fn the_spent_store_prunes_expired_nonces_on_every_write() {
        let store = spent::Spent::new();
        let past = chrono::Utc::now().timestamp() - 10;
        let future = chrono::Utc::now().timestamp() + 60;

        assert!(store.claim("old", past));
        assert_eq!(store.len(), 1);
        // Recording the next one prunes what can no longer matter.
        assert!(store.claim("fresh", future));
        assert_eq!(store.len(), 1, "the expired nonce should have been dropped");

        // A live nonce is single-use…
        assert!(!store.claim("fresh", future));
        // …and expiry itself is enforced by the signed token, not by this map: a
        // challenge whose `exp` has passed is rejected when it is decoded, before the
        // store is ever consulted. No leeway: `exp` is exact for a challenge (session
        // tokens keep the 60s grace that clock skew needs).
        let cfg = cfg();
        let expired = {
            let claims = Claims {
                nonce: "expired".into(),
                difficulty: 1,
                exp: (chrono::Utc::now() - chrono::Duration::seconds(5)).timestamp() as usize,
                kind: "pow".to_owned(),
            };
            encode(
                &Header::default(),
                &claims,
                &EncodingKey::from_secret(cfg.jwt_secret.as_bytes()),
            )
            .unwrap()
        };
        assert_eq!(
            verify(
                &cfg,
                &store,
                &Solution {
                    challenge: expired,
                    answer: solve("expired", 1).unwrap()
                }
            ),
            Err(Refused::Invalid)
        );
    }
}
