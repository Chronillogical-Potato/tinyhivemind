//! The problem bank: small problems with answers computed exactly here.
//!
//! "Correct" in every math run means equal to [`Problem::answer`], which is
//! arithmetic in this file rather than a value written into `sim.json`.

use serde::Deserialize;
use serde_json::Value;

use crate::Result;

/// One problem as `sim.json` declares it.
#[derive(Clone, Debug, Deserialize)]
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
    /// Fails for an unknown `kind` or the wrong number of arguments.
    pub(crate) fn answer(&self) -> Result<u64> {
        match (self.kind.as_str(), self.args.as_slice()) {
            ("lattice", [right, up]) => Ok(binomial(right + up, *right)),
            ("modsum", [n, power, modulus]) => Ok(power_sum(*n, *power, *modulus)),
            ("digitsum", [base, exponent]) => Ok(digit_sum(base.pow(u32::try_from(*exponent)?))),
            ("multiples", [limit, a, b]) => Ok((1..*limit).filter(|k| k % a == 0 || k % b == 0).sum()),
            ("sqdiff", [n]) => Ok(square_difference(*n)),
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
            ("lattice", [right, up]) => binomial(right + up - 2, right - 1),
            // Stopping one term early.
            ("modsum", [n, power, modulus]) => power_sum(n - 1, *power, *modulus),
            // One power too few.
            ("digitsum", [base, exponent]) => digit_sum(base.pow(u32::try_from(*exponent)? - 1)),
            // Forgetting inclusion-exclusion: multiples of both counted twice.
            ("multiples", [limit, a, b]) => {
                (1..*limit).filter(|k| k % a == 0).sum::<u64>()
                    + (1..*limit).filter(|k| k % b == 0).sum::<u64>()
            }
            // Summing to n - 1.
            ("sqdiff", [n]) => square_difference(n - 1),
            // Counting 1 as a prime.
            ("primes", [limit]) => primes_below(*limit) + 1,
            _ => answer + 2,
        };
        // A mistake that happens to be right is not a mistake.
        Ok(if mistake == answer { answer + 2 } else { mistake })
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

fn binomial(n: u64, k: u64) -> u64 {
    let k = k.min(n - k);
    (1..=k).fold(1, |acc, i| acc * (n - k + i) / i)
}

fn power_sum(n: u64, power: u64, modulus: u64) -> u64 {
    (1..=n).fold(0, |acc, k| {
        let term = (0..power).fold(1, |value, _| value * k % modulus);
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

fn square_difference(n: u64) -> u64 {
    let sum: u64 = (1..=n).sum();
    let squares: u64 = (1..=n).map(|k| k * k).sum();
    sum * sum - squares
}

fn primes_below(limit: u64) -> u64 {
    (2..limit)
        .filter(|candidate| (2..*candidate).take_while(|d| d * d <= *candidate).all(|d| candidate % d != 0))
        .map(|_| 1)
        .sum()
}
