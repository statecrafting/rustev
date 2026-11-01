//! Pooling and the linear head (spec 011, 3.9.2 and 3.9.3): the arithmetic
//! shared by every encoder.

use crate::manifest::Normalize;

/// A head's weights: `options x hidden` row-major, then `options` biases.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearHead {
    options: usize,
    hidden: usize,
    weights: Vec<f32>,
    biases: Vec<f32>,
}

impl LinearHead {
    /// Parse a weights file (3.9.3): little-endian f32, one row per option
    /// in declared option order, followed by one bias per option. A wrong
    /// length or a non-finite value is refused (`head_invalid`).
    pub fn from_bytes(bytes: &[u8], options: usize, hidden: usize) -> Result<Self, String> {
        let expected = options
            .checked_mul(hidden)
            .and_then(|w| w.checked_add(options))
            .and_then(|n| n.checked_mul(4))
            .ok_or("weights size overflows")?;
        if bytes.len() != expected {
            return Err(format!(
                "{} bytes; {options} x {hidden} weights and {options} biases need {expected}",
                bytes.len()
            ));
        }
        let values: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect();
        if let Some(i) = values.iter().position(|v| !v.is_finite()) {
            let what = if i < options * hidden {
                "weight"
            } else {
                "bias"
            };
            return Err(format!("non-finite {what} at value {i}"));
        }
        let biases = values[options * hidden..].to_vec();
        let mut weights = values;
        weights.truncate(options * hidden);
        Ok(Self {
            options,
            hidden,
            weights,
            biases,
        })
    }

    /// The logit of each option, in declared order: `b_i + sum_j W_ij e_j`
    /// in f64 over exactly converted f32 inputs, summed in index order.
    /// `None` when the embedding has another length or a logit is not
    /// finite (`non_finite`).
    pub fn logits(&self, embedding: &[f32]) -> Option<Vec<f64>> {
        if embedding.len() != self.hidden {
            return None;
        }
        let out: Vec<f64> = (0..self.options)
            .map(|i| {
                let row = &self.weights[i * self.hidden..(i + 1) * self.hidden];
                row.iter()
                    .zip(embedding)
                    .fold(f64::from(self.biases[i]), |acc, (w, e)| {
                        acc + f64::from(*w) * f64::from(*e)
                    })
            })
            .collect();
        out.iter().all(|v| v.is_finite()).then_some(out)
    }
}

/// `mean_masked` pooling (3.9.2) over `tokens x hidden` token vectors:
/// the mask-weighted mean, summed in token order in f32, then divided by
/// the L2 norm under `normalize: l2`. `None` for an empty mask, a zero norm
/// or a non-finite result (`non_finite`).
pub fn pool(
    token_vectors: &[f32],
    mask: &[u32],
    hidden: usize,
    normalize: Normalize,
) -> Option<Vec<f32>> {
    if hidden == 0 || token_vectors.len() != mask.len().checked_mul(hidden)? {
        return None;
    }
    let mut sum = vec![0f32; hidden];
    let mut weight = 0f32;
    for (t, &m) in mask.iter().enumerate() {
        if m == 0 {
            continue;
        }
        let m = m as f32;
        weight += m;
        for (s, v) in sum
            .iter_mut()
            .zip(&token_vectors[t * hidden..(t + 1) * hidden])
        {
            *s += v * m;
        }
    }
    if weight == 0.0 {
        return None;
    }
    for s in &mut sum {
        *s /= weight;
    }
    if normalize == Normalize::L2 {
        let norm = sum.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm == 0.0 || !norm.is_finite() {
            return None;
        }
        for s in &mut sum {
            *s /= norm;
        }
    }
    sum.iter().all(|v| v.is_finite()).then_some(sum)
}
