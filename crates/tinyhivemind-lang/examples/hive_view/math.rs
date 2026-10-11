//! The problem bank: small problems with answers computed exactly here.
//!
//! "Correct" in every math run means equal to [`Problem::answer`], which is
//! arithmetic in this file rather than a value written into `sim.json`.

use serde::Deserialize;
use serde_json::Value;

use crate::Result;

/// One problem as `sim.json` declares it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct Problem {
    /// Stable problem id, e.g. `lattice-6x6`.
    pub(crate) id: String,
    /// Which computation answers it.
    pub(crate) kind: String,
    /// That computation's arguments.
    pub(crate) args: Vec<u64>,
    /// The statement the room is posed.
    pub(crate) statement: String,
}

impl Problem {
    /// The exact answer.
    ///
    /// # Errors
    ///
    /// Fails for an unknown `kind`, the wrong number of arguments, a zero
    /// argument the computation cannot take, or an answer that does not fit
    /// in 64 bits.
    pub(crate) fn answer(&self) -> Result<u64> {
        match (self.kind.as_str(), self.args.as_slice()) {
            ("lattice", [right, up]) => {
                self.nonzero(*right, "lattice dimension")?;
                self.nonzero(*up, "lattice dimension")?;
                self.fit(binomial(u128::from(*right) + u128::from(*up), *right))
            }
            ("modsum", [_, _, 0]) => Err(format!("problem {} has a zero modulus", self.id).into()),
            ("multiples", [_, 0, _] | [_, _, 0]) => {
                Err(format!("problem {} has a zero divisor", self.id).into())
            }
            ("modsum", [n, power, modulus]) => {
                self.nonzero(*n, "modsum length")?;
                self.fit(power_sum(*n, *power, *modulus))
            }
            ("digitsum", [base, exponent]) => {
                self.nonzero(*exponent, "digitsum exponent")?;
                Ok(digit_sum(self.power(*base, *exponent)?))
            }
            ("multiples", [limit, a, b]) => self.fit(
                (1..*limit)
                    .filter(|k| k % a == 0 || k % b == 0)
                    .map(u128::from)
                    .sum(),
            ),
            ("sqdiff", [n]) => {
                self.nonzero(*n, "sqdiff n")?;
                self.fit(square_difference(*n))
            }
            ("primes", [limit]) => Ok(primes_below(*limit)),
            _ => Err(format!("problem {} has an unknown kind or arity", self.id).into()),
        }
    }

    /// The mistake a careless solver characteristically makes on this problem.
    ///
    /// Wrong answers in the simulation are correlated, as real ones are: most
    /// wrong seats land on the same plausible mistake rather than on noise.
    ///
    /// # Errors
    ///
    /// As [`Problem::answer`].
    pub(crate) fn classic_mistake(&self) -> Result<u64> {
        let answer = self.answer()?;
        let mistake = match (self.kind.as_str(), self.args.as_slice()) {
            // Counting grid points instead of steps: one fewer in each direction.
            ("lattice", [right, up]) => self.fit(binomial(
                u128::from(*right) + u128::from(*up) - 2,
                right - 1,
            ))?,
            // Stopping one term early.
            ("modsum", [n, power, modulus]) => self.fit(power_sum(n - 1, *power, *modulus))?,
            // One power too few.
            ("digitsum", [base, exponent]) => digit_sum(self.power(*base, exponent - 1)?),
            // Forgetting inclusion-exclusion: multiples of both counted twice.
            ("multiples", [limit, a, b]) => self.fit(
                (1..*limit)
                    .filter(|k| k % a == 0)
                    .chain((1..*limit).filter(|k| k % b == 0))
                    .map(u128::from)
                    .sum(),
            )?,
            // Summing to n - 1.
            ("sqdiff", [n]) => self.fit(square_difference(n - 1))?,
            // Counting 1 as a prime.
            ("primes", [limit]) => primes_below(*limit) + 1,
            _ => answer + 2,
        };
        // A mistake that happens to be right is not a mistake.
        Ok(if mistake == answer {
            answer + 2
        } else {
            mistake
        })
    }

    fn nonzero(&self, value: u64, what: &str) -> Result<()> {
        if value == 0 {
            return Err(format!("problem {} has a zero {what}", self.id).into());
        }
        Ok(())
    }

    fn fit(&self, value: u128) -> Result<u64> {
        u64::try_from(value)
            .map_err(|_| format!("problem {} has an answer beyond 64 bits", self.id).into())
    }

    fn power(&self, base: u64, exponent: u64) -> Result<u64> {
        u32::try_from(exponent)
            .ok()
            .and_then(|exponent| base.checked_pow(exponent))
            .ok_or_else(|| format!("problem {} has a power beyond 64 bits", self.id).into())
    }
}

/// Decode the problem bank from a `sim.json` value.
///
/// # Errors
///
/// Fails when `problems` is missing, malformed, or any answer is not
/// computable.
pub(crate) fn problems(sim: &Value) -> Result<Vec<Problem>> {
    let problems: Vec<Problem> = serde_json::from_value(
        sim.get("problems")
            .cloned()
            .ok_or("sim.json has no problems")?,
    )?;
    if problems.is_empty() {
        return Err("sim.json has an empty problem bank".into());
    }
    for problem in &problems {
        problem.answer()?;
        problem.classic_mistake()?;
    }
    Ok(problems)
}

/// `C(n, k)`, exact in 128 bits (saturating if even that is exceeded, which
/// [`Problem::fit`] then refuses).
fn binomial(n: u128, k: u64) -> u128 {
    let k = u128::from(k).min(n.saturating_sub(u128::from(k)));
    (1..=k)
        .try_fold(1_u128, |acc, i| acc.checked_mul(n - k + i).map(|v| v / i))
        .unwrap_or(u128::MAX)
}

fn power_sum(n: u64, power: u64, modulus: u64) -> u128 {
    let modulus = u128::from(modulus);
    (1..=n).fold(0, |acc, k| {
        let term = (0..power).fold(1, |value, _| value * u128::from(k) % modulus);
        (acc + term) % modulus
    })
}

fn digit_sum(mut value: u64) -> u64 {
    let mut sum = 0;
    while value > 0 {
        sum += value % 10;
        value /= 10;
    }
    sum
}

fn square_difference(n: u64) -> u128 {
    let sum: u128 = (1..=u128::from(n)).sum();
    let squares: u128 = (1..=u128::from(n)).map(|k| k * k).sum();
    sum * sum - squares
}

fn primes_below(limit: u64) -> u64 {
    (2..limit)
        .filter(|candidate| {
            (2..*candidate)
                .take_while(|d| d * d <= *candidate)
                .all(|d| candidate % d != 0)
        })
        .map(|_| 1)
        .sum()
}
